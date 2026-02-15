use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::analysis::AnalyzedComment;
use super::state::AppState;

pub async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx);

    let stream = stream.filter_map(|result| {
        match result {
            Ok(comment) => {
                let row_html = render_comment_row(&comment);
                let event = Event::default()
                    .event("comment")
                    .data(row_html);
                Some(Ok(event))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn render_comment_row(c: &AnalyzedComment) -> String {
    let lead_class = if c.is_lead { "is-lead" } else { "" };
    let lead_badge = if c.is_lead {
        format!(
            r#"<span class="lead-badge" title="{}">LEAD {:.0}%</span>"#,
            html_escape(&c.need_summary),
            c.lead_score * 100.0
        )
    } else {
        String::new()
    };

    let need = if c.is_lead {
        format!(r#"<div class="need-summary">{}</div>"#, html_escape(&c.need_summary))
    } else {
        String::new()
    };

    let username = c.username.as_deref().map(|u| format!("@{}", html_escape(u))).unwrap_or_default();
    let phone = c.phone.as_deref().map(|p| html_escape(p)).unwrap_or_default();

    format!(
        r#"<tr class="comment-row {} {}">
  <td class="lead-cell">{}</td>
  <td class="channel">@{}</td>
  <td class="author">{}</td>
  <td class="username">{}</td>
  <td class="phone">{}</td>
  <td class="text">{}{}</td>
  <td class="intent"><span class="badge {}">{}</span></td>
  <td class="confidence">{:.0}%</td>
  <td class="date">{}</td>
</tr>"#,
        c.intent.css_class(),
        lead_class,
        lead_badge,
        html_escape(&c.channel),
        html_escape(&c.author),
        username,
        phone,
        html_escape(&c.text),
        need,
        c.intent.css_class(),
        c.intent,
        c.confidence * 100.0,
        c.date.format("%H:%M:%S"),
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
