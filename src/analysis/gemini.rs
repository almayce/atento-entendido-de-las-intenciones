use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Semaphore};
use tracing::{error, info, warn};
use std::sync::Arc;
use chrono::Utc;

use crate::config::GeminiConfig;
use crate::telegram::RawComment;
use super::intent::Intent;
use super::types::AnalyzedComment;

pub struct GeminiAnalyzer {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
    semaphore: Arc<Semaphore>,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
    generation_config: GenerationConfig,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Serialize)]
struct GenerationConfig {
    temperature: f32,
    max_output_tokens: u32,
    response_mime_type: String,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Option<Vec<ResponsePart>>,
}

#[derive(Deserialize)]
struct ResponsePart {
    text: Option<String>,
}

#[derive(Deserialize)]
struct IntentResponse {
    intent: String,
    confidence: f32,
    is_lead: bool,
    lead_score: f32,
    need_summary: String,
}

const SYSTEM_PROMPT: &str = r#"You are a B2B lead identification system. You analyze comments in Russian real estate developer Telegram channels to find BUSINESS OWNERS, entrepreneurs, marketers, and executives who could benefit from a "smart Telegram monitoring" service — a tool that automatically scans Telegram channels, finds leads, and analyzes audience activity.

The service helps businesses: find clients in Telegram, monitor competitors, track brand mentions, automate lead generation from public channels.

IMPORTANT: Regular apartment buyers, tenants, and individuals are NOT leads. You are looking for people who represent a business or have a business problem that Telegram monitoring could solve.

Intent categories (classify the comment's primary intent):
- business_owner: Person identifies as owner, co-founder, CEO, entrepreneur, runs a business or agency
- marketer: Person works in marketing, sales, lead generation, CRM, advertising — mentions campaigns, funnels, conversions
- realtor_agency: Person is a realtor, broker, or represents a real estate agency — sells or rents multiple properties
- investor: Person buys multiple properties, manages a portfolio, discusses investment at scale
- it_business: Person builds products, works in tech, SaaS, automation — could be a partner or referral
- pain_signal: Person expresses a clear business pain that Telegram monitoring could solve (e.g. "can't find clients", "need to track competitors", "tired of manual monitoring")
- individual: Regular person — buying/renting for themselves, discussing their own apartment
- neutral: General comment, reaction, no business context
- spam: Spam, bots, ads

Lead identification — be STRICT. is_lead=true ONLY when:
1. Person is clearly a business owner, marketer, agency owner, or entrepreneur (not an individual)
2. OR person expresses a pain point that Telegram monitoring directly solves

is_lead=false for:
- Individuals buying/renting for personal use
- Residents complaining about their apartment
- General questions about infrastructure, prices for personal purchase
- Neutral reactions, jokes, emojis

lead_score: 0.0-1.0 reflecting fit for the Telegram monitoring service:
- 0.8-1.0: Business owner or marketer explicitly discussing lead generation, client acquisition, competitor monitoring, or automation in Telegram
- 0.5-0.7: Realtor/agency or entrepreneur who likely needs client acquisition tools
- 0.3-0.5: Investor at scale or person with a pain signal around finding clients/monitoring
- 0.0-0.2: Individual, not a business lead

need_summary: One sentence in Russian describing the person's business role and potential need (empty string if not a lead)

Respond ONLY with JSON:
{"intent": "<category>", "confidence": <0.0-1.0>, "is_lead": <true/false>, "lead_score": <0.0-1.0>, "need_summary": "<string>"}"#;

impl GeminiAnalyzer {
    pub fn new(config: &GeminiConfig) -> Self {
        Self {
            client: Client::new(),
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            semaphore: Arc::new(Semaphore::new(config.max_concurrent)),
        }
    }

    pub async fn run(
        self: Arc<Self>,
        mut rx: mpsc::Receiver<RawComment>,
        tx: tokio::sync::broadcast::Sender<AnalyzedComment>,
    ) -> Result<()> {
        info!("Gemini analyzer started (max_concurrent: {})", self.semaphore.available_permits());

        while let Some(comment) = rx.recv().await {
            let permit = self.semaphore.clone().acquire_owned().await?;
            let analyzer = self.clone();
            let tx = tx.clone();

            tokio::spawn(async move {
                let analyzed = analyzer.analyze(&comment).await;
                drop(permit);

                match analyzed {
                    Ok(result) => {
                        if result.is_lead {
                            info!(
                                "LEAD found in @{}: [{}] {} — \"{}\"",
                                result.channel, result.intent, result.author, result.need_summary
                            );
                        }
                        if tx.send(result).is_err() {
                            warn!("No active receivers for analyzed comments");
                        }
                    }
                    Err(e) => {
                        error!("Failed to analyze comment: {:#}", e);
                        let fallback = AnalyzedComment {
                            channel: comment.channel,
                            post_id: comment.post_id,
                            comment_id: comment.comment_id,
                            author: comment.author,
                            username: comment.username,
                            phone: comment.phone,
                            text: comment.text,
                            date: comment.date,
                            intent: Intent::Neutral,
                            confidence: 0.0,
                            is_lead: false,
                            lead_score: 0.0,
                            need_summary: String::new(),
                            analyzed_at: Utc::now(),
                        };
                        let _ = tx.send(fallback);
                    }
                }
            });
        }

        Ok(())
    }

    async fn analyze(&self, comment: &RawComment) -> Result<AnalyzedComment> {
        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, self.model, self.api_key
        );

        let prompt = format!(
            "{}\n\nComment from @{} in channel @{}:\n\"{}\"",
            SYSTEM_PROMPT, comment.author, comment.channel, comment.text
        );

        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![Part { text: prompt }],
            }],
            generation_config: GenerationConfig {
                temperature: 0.1,
                max_output_tokens: 200,
                response_mime_type: "application/json".to_string(),
            },
        };

        let mut attempt = 0u32;
        let max_retries = 4u32;
        let response = loop {
            let resp = self
                .client
                .post(&url)
                .json(&request)
                .send()
                .await
                .context("Gemini API request failed")?;

            if resp.status() != reqwest::StatusCode::TOO_MANY_REQUESTS {
                break resp;
            }

            let _ = resp.text().await; // drain body
            if attempt >= max_retries {
                anyhow::bail!("Gemini API 429 after {} retries", max_retries);
            }
            let wait_secs = 5u64 * 2u64.pow(attempt);
            warn!("Gemini 429, retry {}/{} in {}s", attempt + 1, max_retries, wait_secs);
            tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;
            attempt += 1;
        };

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API returned {}: {}", status, body);
        }

        let gemini_resp: GeminiResponse = response
            .json()
            .await
            .context("Failed to parse Gemini response")?;

        let text = gemini_resp
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|c| c.content.as_ref())
            .and_then(|c| c.parts.as_ref())
            .and_then(|p| p.first())
            .and_then(|p| p.text.as_ref())
            .context("Empty Gemini response")?;

        let parsed: IntentResponse =
            serde_json::from_str(text).context("Failed to parse intent JSON from Gemini")?;

        let intent = match parsed.intent.to_lowercase().as_str() {
            "business_owner" => Intent::BusinessOwner,
            "marketer" => Intent::Marketer,
            "realtor_agency" => Intent::RealtorAgency,
            "investor" => Intent::Investor,
            "it_business" => Intent::ItBusiness,
            "pain_signal" => Intent::PainSignal,
            "individual" => Intent::Individual,
            "spam" => Intent::Spam,
            _ => Intent::Neutral,
        };

        Ok(AnalyzedComment {
            channel: comment.channel.clone(),
            post_id: comment.post_id,
            comment_id: comment.comment_id,
            author: comment.author.clone(),
            username: comment.username.clone(),
            phone: comment.phone.clone(),
            text: comment.text.clone(),
            date: comment.date,
            intent,
            confidence: parsed.confidence,
            is_lead: parsed.is_lead,
            lead_score: parsed.lead_score,
            need_summary: parsed.need_summary,
            analyzed_at: Utc::now(),
        })
    }
}
