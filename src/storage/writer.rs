use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info};

use crate::analysis::{AnalyzedComment, Intent};
use crate::config::StorageConfig;

#[derive(Debug, Serialize)]
struct LeadEntry {
    rank: usize,
    lead_score: f32,
    author: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    phone: Option<String>,
    channel: String,
    post_id: i32,
    comment_id: i32,
    intent: Intent,
    need_summary: String,
    text: String,
    date: DateTime<Utc>,
    post_url: String,
}

#[derive(Debug, Serialize)]
struct LeadsReport {
    generated_at: DateTime<Utc>,
    total_leads: usize,
    leads: Vec<LeadEntry>,
}

#[derive(Debug, Default)]
struct ChannelStat {
    has_comments: Option<bool>,
    comments_total: usize,
    leads_total: usize,
}

#[derive(Debug, Serialize)]
struct ChannelEntry {
    name: String,
    has_comments: bool,
    comments_collected: usize,
    leads_found: usize,
    lead_rate: f64,
}

#[derive(Debug, Serialize)]
struct ChannelsReport {
    generated_at: DateTime<Utc>,
    channels: Vec<ChannelEntry>,
}

pub struct StorageWriter {
    data_dir: PathBuf,
    format: String,
    leads: Vec<AnalyzedComment>,
    channel_stats: HashMap<String, ChannelStat>,
    channel_status_rx: mpsc::Receiver<(String, bool)>,
}

impl StorageWriter {
    pub fn new(config: &StorageConfig, channel_status_rx: mpsc::Receiver<(String, bool)>) -> Self {
        Self {
            data_dir: config.data_dir.clone(),
            format: config.format.clone(),
            leads: Vec::new(),
            channel_stats: HashMap::new(),
            channel_status_rx,
        }
    }

    pub async fn run(mut self, mut rx: broadcast::Receiver<AnalyzedComment>) -> Result<()> {
        info!("Storage writer started (format: {})", self.format);

        std::fs::create_dir_all(&self.data_dir)
            .context("Failed to create data directory")?;

        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(comment) => {
                            let stat = self.channel_stats.entry(comment.channel.clone()).or_default();
                            stat.comments_total += 1;
                            if comment.is_lead {
                                stat.leads_total += 1;
                            }

                            if let Err(e) = self.write(&comment).await {
                                error!("Failed to write comment: {:#}", e);
                            }
                            if comment.is_lead {
                                self.leads.push(comment);
                                if let Err(e) = self.write_leads_report().await {
                                    error!("Failed to write leads report: {:#}", e);
                                }
                            }
                            if let Err(e) = self.write_channels_report().await {
                                error!("Failed to write channels report: {:#}", e);
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("Storage writer lagged, skipped {} messages", n);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            info!("Broadcast channel closed, storage writer stopping");
                            break;
                        }
                    }
                }

                status = self.channel_status_rx.recv() => {
                    if let Some((channel, has_comments)) = status {
                        self.channel_stats.entry(channel).or_default().has_comments = Some(has_comments);
                        if let Err(e) = self.write_channels_report().await {
                            error!("Failed to write channels report: {:#}", e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn write_channels_report(&self) -> Result<()> {
        let mut entries: Vec<ChannelEntry> = self.channel_stats
            .iter()
            .map(|(name, stat)| {
                let lead_rate = if stat.comments_total > 0 {
                    stat.leads_total as f64 / stat.comments_total as f64
                } else {
                    0.0
                };
                ChannelEntry {
                    name: name.clone(),
                    has_comments: stat.has_comments.unwrap_or(false),
                    comments_collected: stat.comments_total,
                    leads_found: stat.leads_total,
                    lead_rate,
                }
            })
            .collect();

        entries.sort_by(|a, b| b.comments_collected.cmp(&a.comments_collected));

        let report = ChannelsReport {
            generated_at: Utc::now(),
            channels: entries,
        };

        let json = serde_json::to_string_pretty(&report)
            .context("Failed to serialize channels report")?;

        let path = self.data_dir.join("channels.json");
        tokio::fs::write(&path, json.as_bytes())
            .await
            .context("Failed to write channels.json")?;

        Ok(())
    }

    async fn write_leads_report(&self) -> Result<()> {
        let mut sorted = self.leads.clone();
        sorted.sort_by(|a, b| b.lead_score.partial_cmp(&a.lead_score).unwrap_or(std::cmp::Ordering::Equal));

        let entries: Vec<LeadEntry> = sorted
            .iter()
            .enumerate()
            .map(|(i, c)| LeadEntry {
                rank: i + 1,
                lead_score: c.lead_score,
                author: c.author.clone(),
                username: c.username.clone(),
                phone: c.phone.clone(),
                channel: c.channel.clone(),
                post_id: c.post_id,
                comment_id: c.comment_id,
                intent: c.intent,
                need_summary: c.need_summary.clone(),
                text: c.text.clone(),
                date: c.date,
                post_url: format!("https://t.me/{}/{}", c.channel, c.post_id),
            })
            .collect();

        let report = LeadsReport {
            generated_at: Utc::now(),
            total_leads: entries.len(),
            leads: entries,
        };

        let json = serde_json::to_string_pretty(&report)
            .context("Failed to serialize leads report")?;

        let path = self.data_dir.join("leads.json");
        tokio::fs::write(&path, json.as_bytes())
            .await
            .context("Failed to write leads.json")?;

        info!("leads.json updated ({} leads)", report.total_leads);
        Ok(())
    }

    async fn write(&self, comment: &AnalyzedComment) -> Result<()> {
        let date_str = Utc::now().format("%Y-%m-%d").to_string();
        let filename = format!("comments_{}.{}", date_str, self.format);
        let path = self.data_dir.join(filename);

        match self.format.as_str() {
            "jsonl" => self.write_jsonl(&path, comment).await,
            "csv" => self.write_csv(&path, comment).await,
            _ => anyhow::bail!("Unknown format: {}", self.format),
        }
    }

    async fn write_jsonl(&self, path: &PathBuf, comment: &AnalyzedComment) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        let json = serde_json::to_string(comment).context("Failed to serialize comment")?;
        let line = format!("{}\n", json);

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .context("Failed to open JSONL file")?;

        file.write_all(line.as_bytes())
            .await
            .context("Failed to write to JSONL file")?;

        Ok(())
    }

    async fn write_csv(&self, path: &PathBuf, comment: &AnalyzedComment) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        let exists = path.exists();

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .context("Failed to open CSV file")?;

        if !exists {
            let header = "channel,post_id,comment_id,author,text,date,intent,confidence,analyzed_at\n";
            file.write_all(header.as_bytes()).await?;
        }

        let escaped_text = comment.text.replace('"', "\"\"");
        let line = format!(
            "{},{},{},\"{}\",\"{}\",{},{},{:.2},{}\n",
            comment.channel,
            comment.post_id,
            comment.comment_id,
            comment.author.replace('"', "\"\""),
            escaped_text,
            comment.date.to_rfc3339(),
            comment.intent,
            comment.confidence,
            comment.analyzed_at.to_rfc3339(),
        );

        file.write_all(line.as_bytes()).await?;
        Ok(())
    }
}
