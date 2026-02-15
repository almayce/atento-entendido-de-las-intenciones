use askama::Template;
use axum::extract::State;
use axum::response::Html;

use crate::analysis::Intent;
use super::state::AppState;

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate {
    comments: Vec<CommentView>,
    total: usize,
    leads: usize,
    lead_rate: String,
    stats: Vec<(String, usize)>,
}

struct CommentView {
    is_lead: bool,
    lead_score: String,
    need_summary: String,
    channel: String,
    author: String,
    username: String,
    phone: String,
    text: String,
    intent: String,
    intent_css: String,
    confidence: String,
    date: String,
}

pub async fn dashboard(State(state): State<AppState>) -> Html<String> {
    let recent = state.recent.read().await;
    let stats = state.stats.read().await;

    // Leads first, then by date descending
    let mut sorted: Vec<_> = recent.iter().collect();
    sorted.sort_by(|a, b| {
        b.is_lead.cmp(&a.is_lead)
            .then(b.lead_score.partial_cmp(&a.lead_score).unwrap_or(std::cmp::Ordering::Equal))
            .then(b.date.cmp(&a.date))
    });

    let comments: Vec<CommentView> = sorted
        .iter()
        .map(|c| CommentView {
            is_lead: c.is_lead,
            lead_score: format!("{:.0}%", c.lead_score * 100.0),
            need_summary: c.need_summary.clone(),
            channel: format!("@{}", c.channel),
            author: c.author.clone(),
            username: c.username.as_deref().map(|u| format!("@{}", u)).unwrap_or_default(),
            phone: c.phone.clone().unwrap_or_default(),
            text: c.text.clone(),
            intent: c.intent.to_string(),
            intent_css: c.intent.css_class().to_string(),
            confidence: format!("{:.0}%", c.confidence * 100.0),
            date: c.date.format("%H:%M:%S").to_string(),
        })
        .collect();

    let lead_rate = if stats.total > 0 {
        format!("{:.0}%", (stats.leads as f64 / stats.total as f64) * 100.0)
    } else {
        "—".to_string()
    };

    let mut intent_stats: Vec<(String, usize)> = Intent::all()
        .iter()
        .filter_map(|intent| {
            let count = stats.by_intent.get(intent).copied().unwrap_or(0);
            if count > 0 {
                Some((intent.to_string(), count))
            } else {
                None
            }
        })
        .collect();
    intent_stats.sort_by(|a, b| b.1.cmp(&a.1));

    let template = DashboardTemplate {
        comments,
        total: stats.total,
        leads: stats.leads,
        lead_rate,
        stats: intent_stats,
    };

    Html(template.render().unwrap_or_else(|e| format!("Template error: {}", e)))
}
