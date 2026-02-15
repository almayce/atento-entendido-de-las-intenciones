use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RawComment {
    pub channel: String,
    pub post_id: i32,
    pub comment_id: i32,
    pub author: String,
    pub username: Option<String>,
    pub phone: Option<String>,
    pub text: String,
    pub date: DateTime<Utc>,
}
