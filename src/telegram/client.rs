use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use grammers_client::Client;
use grammers_session::storages::MemorySession;
use grammers_tl_types as tl;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tracing::{error, info, warn};

use crate::config::TelegramConfig;
use super::types::RawComment;

pub struct TelegramScraper {
    client: Client,
    channels: Vec<String>,
    poll_interval: std::time::Duration,
    /// Tracks the last seen comment ID per (channel, post_id) to avoid duplicates
    seen: HashMap<(String, i32), i32>,
    /// Cache: channel_name → has linked discussion group (comments enabled)
    channel_has_comments: HashMap<String, bool>,
    /// Sends (channel_name, has_comments) to storage for channels.json
    channel_status_tx: mpsc::Sender<(String, bool)>,
}

impl TelegramScraper {
    pub async fn connect(
        config: &TelegramConfig,
        channel_status_tx: mpsc::Sender<(String, bool)>,
    ) -> Result<Self> {
        let session = Arc::new(MemorySession::default());

        let pool = grammers_client::sender::SenderPool::new(
            session,
            config.api_id,
        );

        let client = Client::new(pool.handle);

        // Run the sender pool in background
        tokio::spawn(async move {
            pool.runner.run().await;
        });

        if !client.is_authorized().await? {
            info!("Not authorized. Starting interactive sign-in...");
            Self::interactive_login(&client, &config.api_hash).await?;
        }

        info!("Telegram client connected and authorized");

        Ok(Self {
            client,
            channels: config.channels.clone(),
            poll_interval: std::time::Duration::from_secs(config.poll_interval_secs),
            seen: HashMap::new(),
            channel_has_comments: HashMap::new(),
            channel_status_tx,
        })
    }

    async fn interactive_login(client: &Client, api_hash: &str) -> Result<()> {
        let mut phone = String::new();
        println!("Enter your phone number (international format, e.g. +1234567890):");
        std::io::stdin().read_line(&mut phone)?;
        let phone = phone.trim();

        let token = client.request_login_code(phone, api_hash).await?;
        let mut code = String::new();
        println!("Enter the code you received:");
        std::io::stdin().read_line(&mut code)?;
        let code = code.trim();

        match client.sign_in(&token, code).await {
            Ok(_) => {}
            Err(grammers_client::SignInError::PasswordRequired(password_token)) => {
                let mut password = String::new();
                println!("2FA password required. Enter your password:");
                std::io::stdin().read_line(&mut password)?;
                let password = password.trim();
                client
                    .check_password(password_token, password.to_string())
                    .await?;
            }
            Err(e) => return Err(e.into()),
        }

        info!("Successfully signed in");
        Ok(())
    }

    pub async fn run(mut self, tx: mpsc::Sender<RawComment>) -> Result<()> {
        info!("Starting Telegram scraper for channels: {:?}", self.channels);

        loop {
            for channel_name in &self.channels.clone() {
                info!("Polling @{}", channel_name);
                let poll_future = self.poll_channel(channel_name, &tx);
                match timeout(std::time::Duration::from_secs(300), poll_future).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => error!("Error polling @{}: {:#}", channel_name, e),
                    Err(_) => error!("Global timeout polling @{} (>300s), skipping", channel_name),
                }
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn poll_channel(&mut self, channel_name: &str, tx: &mpsc::Sender<RawComment>) -> Result<()> {
        let channel = timeout(
            std::time::Duration::from_secs(15),
            self.client.resolve_username(channel_name),
        )
        .await
        .context("Timeout resolving channel username")?
        .context(format!("Channel @{} not found", channel_name))?
        .context(format!("Channel @{} not found", channel_name))?;

        let peer_ref = timeout(
            std::time::Duration::from_secs(10),
            channel.to_ref(),
        )
        .await
        .context("Timeout getting peer ref")?
        .context("Cannot get peer ref for channel")?;

        // Check once per channel if it has a linked discussion group
        let has_comments = if let Some(&cached) = self.channel_has_comments.get(channel_name) {
            cached
        } else {
            let result = self.check_has_comments(peer_ref.clone()).await;
            info!("Channel @{}: comments enabled = {}", channel_name, result);
            self.channel_has_comments.insert(channel_name.to_string(), result);
            let _ = self.channel_status_tx.send((channel_name.to_string(), result)).await;
            result
        };

        if !has_comments {
            return Ok(());
        }

        // Get recent messages (posts) from the channel
        let mut messages = self.client.iter_messages(peer_ref.clone()).limit(200);

        let mut posts = Vec::new();
        while let Some(msg) = timeout(std::time::Duration::from_secs(15), messages.next())
            .await
            .context("Timeout fetching messages")?
            .context("Error fetching messages")?
        {
            posts.push(msg);
        }

        for post in &posts {
            let post_id = post.id();

            let replies_result = timeout(
                std::time::Duration::from_secs(5),
                self.get_replies(peer_ref.clone(), post_id),
            )
            .await;

            let reply_messages_opt = match replies_result {
                Ok(Ok(msgs)) => Some(msgs),
                Ok(Err(e)) => {
                    warn!("Error getting replies for post {} in {}: {:#}", post_id, channel_name, e);
                    None
                }
                Err(_) => {
                    warn!("Timeout getting replies for post {} in {}", post_id, channel_name);
                    None
                }
            };

            if let Some(mut reply_messages) = reply_messages_opt {
                let last_seen = self
                    .seen
                    .get(&(channel_name.to_string(), post_id))
                    .copied()
                    .unwrap_or(0);

                let mut max_id = last_seen;

                for (comment_id, author, username, phone, text, date) in reply_messages.drain(..) {
                    if comment_id <= last_seen {
                        continue;
                    }
                    max_id = max_id.max(comment_id);

                    let comment = RawComment {
                        channel: channel_name.to_string(),
                        post_id,
                        comment_id,
                        author,
                        username,
                        phone,
                        text,
                        date,
                    };

                    if tx.send(comment).await.is_err() {
                        return Ok(());
                    }
                }

                if max_id > last_seen {
                    self.seen
                        .insert((channel_name.to_string(), post_id), max_id);
                }
            }
        }

        Ok(())
    }

    async fn check_has_comments(&self, peer_ref: grammers_session::types::PeerRef) -> bool {
        let input_peer: tl::enums::InputPeer = peer_ref.into();
        let input_channel = match input_peer {
            tl::enums::InputPeer::Channel(c) => {
                tl::enums::InputChannel::Channel(tl::types::InputChannel {
                    channel_id: c.channel_id,
                    access_hash: c.access_hash,
                })
            }
            _ => return false,
        };

        let request = tl::functions::channels::GetFullChannel { channel: input_channel };

        match timeout(std::time::Duration::from_secs(10), self.client.invoke(&request)).await {
            Ok(Ok(tl::enums::messages::ChatFull::Full(full))) => match full.full_chat {
                tl::enums::ChatFull::ChannelFull(cf) => cf.linked_chat_id.is_some(),
                _ => false,
            },
            Ok(Err(e)) => {
                warn!("GetFullChannel error: {:#}", e);
                false
            }
            Err(_) => {
                warn!("GetFullChannel timeout");
                false
            }
        }
    }

    async fn get_replies(
        &self,
        peer_ref: grammers_session::types::PeerRef,
        post_id: i32,
    ) -> Result<Vec<(i32, String, Option<String>, Option<String>, String, DateTime<Utc>)>> {
        let input_peer: tl::enums::InputPeer = peer_ref.clone().into();

        let request = tl::functions::messages::GetReplies {
            peer: input_peer,
            msg_id: post_id,
            offset_id: 0,
            offset_date: 0,
            add_offset: 0,
            limit: 50,
            max_id: 0,
            min_id: 0,
            hash: 0,
        };

        let response = match self.client.invoke(&request).await {
            Ok(r) => r,
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("MSG_ID_INVALID") || msg.contains("CHANNEL_PRIVATE") {
                    return Ok(vec![]);
                }
                return Err(e.into());
            }
        };

        let mut results = Vec::new();

        match response {
            tl::enums::messages::Messages::Messages(msgs) => {
                Self::extract_comments(&msgs.messages, &msgs.users, &mut results);
            }
            tl::enums::messages::Messages::Slice(msgs) => {
                Self::extract_comments(&msgs.messages, &msgs.users, &mut results);
            }
            tl::enums::messages::Messages::ChannelMessages(msgs) => {
                Self::extract_comments(&msgs.messages, &msgs.users, &mut results);
            }
            _ => {}
        }

        Ok(results)
    }

    fn extract_comments(
        messages: &[tl::enums::Message],
        users: &[tl::enums::User],
        results: &mut Vec<(i32, String, Option<String>, Option<String>, String, DateTime<Utc>)>,
    ) {
        struct UserInfo {
            name: String,
            username: Option<String>,
            phone: Option<String>,
        }

        let user_map: HashMap<i64, UserInfo> = users
            .iter()
            .filter_map(|u| match u {
                tl::enums::User::User(user) => {
                    let name = user
                        .first_name
                        .as_deref()
                        .unwrap_or("Unknown")
                        .to_string();
                    let username = user.username.clone();
                    let phone = user.phone.clone();
                    Some((user.id, UserInfo { name, username, phone }))
                }
                _ => None,
            })
            .collect();

        for msg in messages {
            if let tl::enums::Message::Message(m) = msg {
                let text = m.message.clone();
                if text.is_empty() {
                    continue;
                }

                let author_id = match &m.from_id {
                    Some(tl::enums::Peer::User(u)) => u.user_id,
                    _ => 0,
                };
                let info = user_map.get(&author_id);
                let author = info
                    .map(|i| i.name.clone())
                    .unwrap_or_else(|| "Anonymous".to_string());
                let username = info.and_then(|i| i.username.clone());
                let phone = info.and_then(|i| i.phone.clone());

                let date = DateTime::from_timestamp(m.date as i64, 0)
                    .unwrap_or_default();

                results.push((m.id, author, username, phone, text, date));
            }
        }
    }
}
