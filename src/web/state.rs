use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::analysis::{AnalyzedComment, Intent};

#[derive(Clone)]
pub struct AppState {
    pub tx: broadcast::Sender<AnalyzedComment>,
    pub recent: Arc<RwLock<Vec<AnalyzedComment>>>,
    pub leads: Arc<RwLock<Vec<AnalyzedComment>>>,
    pub stats: Arc<RwLock<Stats>>,
    pub buffer_size: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub total: usize,
    pub leads: usize,
    pub by_intent: HashMap<Intent, usize>,
}

impl AppState {
    pub fn new(tx: broadcast::Sender<AnalyzedComment>, buffer_size: usize) -> Self {
        Self {
            tx,
            recent: Arc::new(RwLock::new(Vec::with_capacity(buffer_size))),
            leads: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(Stats::default())),
            buffer_size,
        }
    }

    pub async fn push_comment(&self, comment: AnalyzedComment) {
        {
            let mut stats = self.stats.write().await;
            stats.total += 1;
            if comment.is_lead {
                stats.leads += 1;
            }
            *stats.by_intent.entry(comment.intent).or_insert(0) += 1;
        }

        if comment.is_lead {
            let mut leads = self.leads.write().await;
            leads.push(comment.clone());
            leads.sort_by(|a, b| b.lead_score.partial_cmp(&a.lead_score).unwrap_or(std::cmp::Ordering::Equal));
        }

        {
            let mut recent = self.recent.write().await;
            if recent.len() >= self.buffer_size {
                recent.remove(0);
            }
            recent.push(comment);
        }
    }
}
