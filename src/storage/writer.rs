use anyhow::{Context, Result};
use chrono::Utc;
use std::path::PathBuf;
use tokio::sync::broadcast;
use tracing::{error, info};

use crate::analysis::AnalyzedComment;
use crate::config::StorageConfig;

pub struct StorageWriter {
    data_dir: PathBuf,
    format: String,
}

impl StorageWriter {
    pub fn new(config: &StorageConfig) -> Self {
        Self {
            data_dir: config.data_dir.clone(),
            format: config.format.clone(),
        }
    }

    pub async fn run(self, mut rx: broadcast::Receiver<AnalyzedComment>) -> Result<()> {
        info!("Storage writer started (format: {})", self.format);

        std::fs::create_dir_all(&self.data_dir)
            .context("Failed to create data directory")?;

        loop {
            match rx.recv().await {
                Ok(comment) => {
                    if let Err(e) = self.write(&comment).await {
                        error!("Failed to write comment: {:#}", e);
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
