use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::intent::Intent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzedComment {
    pub channel: String,
    pub post_id: i32,
    pub comment_id: i32,
    pub author: String,
    pub username: Option<String>,
    pub phone: Option<String>,
    pub text: String,
    pub date: DateTime<Utc>,
    pub intent: Intent,
    pub confidence: f32,
    /// Is this a potential lead?
    pub is_lead: bool,
    /// 0.0-1.0, how likely this person needs help/has a problem
    pub lead_score: f32,
    /// Short summary of what the person needs (empty if not a lead)
    pub need_summary: String,
    pub analyzed_at: DateTime<Utc>,
}
