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
    let leads = state.leads.read().await;
    let stats = state.stats.read().await;

    // Show all leads first (from dedicated leads buffer), then recent non-lead comments
    let lead_views: Vec<_> = leads.iter().collect();
    let recent_non_leads: Vec<_> = recent.iter().filter(|c| !c.is_lead).collect();
    let combined: Vec<_> = lead_views.into_iter().chain(recent_non_leads).collect();

    let comments: Vec<CommentView> = combined
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
