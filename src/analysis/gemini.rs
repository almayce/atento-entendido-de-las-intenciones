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

const SYSTEM_PROMPT: &str = r#"You are a lead identification system for real estate developer Telegram channels. You analyze comments to find people who are genuinely looking to BUY property, need a specific service, or have a real problem that a sales team could help with.

IMPORTANT: Most questions and comments are NOT leads. A lead is someone who shows real buying intent or a concrete need that can be addressed by a sales team. Curiosity, general discussion, jokes, opinions — these are NOT leads.

Intent categories:
- problem: Person describes a real problem with a purchase, apartment, mortgage, delivery dates, defects
- question: Person asks a general or informational question (NOT a lead by default)
- help_request: Person explicitly looking for help with buying, choosing an apartment, mortgage, trade-in
- complaint: Person expresses dissatisfaction with quality, service, management company
- buying_intent: Person shows clear interest in purchasing — asks about prices, availability, specific layouts, start of sales, booking
- feedback: Person gives feedback or suggestions, shares experience
- neutral: General comment, reaction, meme, emoji, no actionable need
- spam: Spam, ads, bots, irrelevant

Lead identification — be STRICT:
- is_lead=true ONLY for:
  - buying_intent: person is actively looking to buy or asking about specific properties/prices/availability
  - help_request: person explicitly needs help choosing, buying, getting a mortgage
  - problem: person has a concrete problem that a sales/support team can resolve
- is_lead=false for:
  - question: general curiosity, asking about news, neighborhood, infrastructure (NOT a lead)
  - complaint: venting without seeking resolution (NOT a lead unless asking for help)
  - feedback, neutral, spam: never leads

- lead_score: 0.0-1.0 reflecting buying intent strength
  - 0.8-1.0: Asking about specific apartment/price/availability, ready to buy, asking how to book
  - 0.5-0.7: Interested in buying but early stage — asking about start of sales, comparing options
  - 0.3-0.5: Has a problem that could lead to a new purchase (e.g. quality issues, wants to move)
  - 0.0-0.2: Not a lead
- need_summary: One sentence in Russian describing what the person needs (empty string if not a lead)

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

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .context("Gemini API request failed")?;

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
            "problem" => Intent::Problem,
            "question" => Intent::Question,
            "help_request" => Intent::HelpRequest,
            "complaint" => Intent::Complaint,
            "buying_intent" => Intent::BuyingIntent,
            "feedback" => Intent::Feedback,
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
