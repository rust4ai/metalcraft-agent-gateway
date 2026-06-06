pub mod discord;
pub mod slack;
pub mod whatsapp;

use async_trait::async_trait;
use serde_json::Value;

/// Uniform result from every platform call.
pub type PlatformResult = Result<Value, PlatformError>;

#[derive(Debug)]
pub struct PlatformError {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {} — {}", self.status, self.message)
    }
}

/// Every chat platform must implement this trait.
#[async_trait]
pub trait Platform: Send + Sync {
    async fn send_message(
        &self,
        channel_id: &str,
        content: &str,
        reply_to: Option<&str>,
    ) -> PlatformResult;

    async fn edit_message(
        &self,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> PlatformResult;

    async fn add_reaction(
        &self,
        channel_id: &str,
        message_id: &str,
        emoji: &str,
    ) -> PlatformResult;

    async fn get_messages(
        &self,
        channel_id: &str,
        limit: u64,
    ) -> PlatformResult;

    async fn get_channel_info(&self, channel_id: &str) -> PlatformResult;
}
