mod analysis;
mod config;
mod storage;
mod telegram;
mod web;

use std::sync::Arc;
use anyhow::Result;
use tokio::sync::{broadcast, mpsc};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "atento=info".into()),
        )
        .init();

    info!("Loading configuration...");
    let config = config::AppConfig::load()?;

    // Channels
    let (raw_tx, raw_rx) = mpsc::channel::<telegram::RawComment>(256);
    let (analyzed_tx, _) = broadcast::channel::<analysis::AnalyzedComment>(256);

    // App state for web
    let app_state = web::state::AppState::new(analyzed_tx.clone(), config.web.recent_buffer_size);

    // Storage writer
    let storage_writer = storage::StorageWriter::new(&config.storage);
    let storage_rx = analyzed_tx.subscribe();

    // Web state updater
    let state_for_updater = app_state.clone();
    let mut updater_rx = analyzed_tx.subscribe();

    // Gemini analyzer
    let analyzer = Arc::new(analysis::GeminiAnalyzer::new(&config.gemini));

    // Telegram scraper
    let scraper = telegram::TelegramScraper::connect(&config.telegram).await?;

    // Spawn tasks
    let scraper_handle = tokio::spawn(async move {
        if let Err(e) = scraper.run(raw_tx).await {
            tracing::error!("Telegram scraper error: {:#}", e);
        }
    });

    let analyzer_handle = tokio::spawn(async move {
        if let Err(e) = analyzer.run(raw_rx, analyzed_tx).await {
            tracing::error!("Gemini analyzer error: {:#}", e);
        }
    });

    let storage_handle = tokio::spawn(async move {
        if let Err(e) = storage_writer.run(storage_rx).await {
            tracing::error!("Storage writer error: {:#}", e);
        }
    });

    // State updater: keeps AppState in sync with broadcast
    let updater_handle = tokio::spawn(async move {
        loop {
            match updater_rx.recv().await {
                Ok(comment) => {
                    state_for_updater.push_comment(comment).await;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("State updater lagged, skipped {} messages", n);
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Web server
    let router = web::create_router(app_state);
    let addr = format!("{}:{}", config.web.host, config.web.port);
    info!("Starting web server at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let web_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            tracing::error!("Web server error: {:#}", e);
        }
    });

    // Wait for any task to finish (shouldn't under normal operation)
    tokio::select! {
        _ = scraper_handle => info!("Scraper task ended"),
        _ = analyzer_handle => info!("Analyzer task ended"),
        _ = storage_handle => info!("Storage task ended"),
        _ = updater_handle => info!("Updater task ended"),
        _ = web_handle => info!("Web server ended"),
    }

    Ok(())
}
