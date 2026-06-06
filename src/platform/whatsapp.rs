use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

use super::{Platform, PlatformError, PlatformResult};

/// WhatsApp via Twilio's Programmable Messaging API.
///
/// Outbound only — inbound messages arrive as Twilio webhooks (see
/// `webhooks::twilio_webhook`). `channel_id` is the counterpart's phone number
/// in E.164 (e.g. `+15551234567`); the `whatsapp:` prefix Twilio expects is
/// added automatically.
pub struct WhatsApp {
    client: Client,
    account_sid: String,
    auth_token: String,
    from: String,
}

impl WhatsApp {
    pub fn from_env() -> Self {
        let account_sid = std::env::var("TWILIO_ACCOUNT_SID")
            .expect("TWILIO_ACCOUNT_SID must be set when WhatsApp is enabled");
        let auth_token = std::env::var("TWILIO_AUTH_TOKEN")
            .expect("TWILIO_AUTH_TOKEN must be set when WhatsApp is enabled");
        let from = to_whatsapp_addr(
            &std::env::var("TWILIO_WHATSAPP_FROM")
                .expect("TWILIO_WHATSAPP_FROM must be set when WhatsApp is enabled"),
        );

        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .expect("Failed to build reqwest client");

        Self { client, account_sid, auth_token, from }
    }
}

/// Ensure a number carries Twilio's `whatsapp:` channel prefix.
fn to_whatsapp_addr(num: &str) -> String {
    let n = num.trim();
    if n.starts_with("whatsapp:") {
        n.to_string()
    } else {
        format!("whatsapp:{n}")
    }
}

#[async_trait]
impl Platform for WhatsApp {
    async fn send_message(
        &self,
        channel_id: &str,
        content: &str,
        _reply_to: Option<&str>,
    ) -> PlatformResult {
        let url = format!(
            "https://api.twilio.com/2010-04-01/Accounts/{}/Messages.json",
            self.account_sid
        );
        let to = to_whatsapp_addr(channel_id);
        let params = [
            ("From", self.from.as_str()),
            ("To", to.as_str()),
            ("Body", content),
        ];

        let resp = self
            .client
            .post(&url)
            .basic_auth(&self.account_sid, Some(&self.auth_token))
            .form(&params)
            .send()
            .await
            .map_err(|e| PlatformError {
                status: 502,
                message: format!("Twilio request failed: {e}"),
            })?;

        let status = resp.status().as_u16();
        let data: Value = resp.json().await.map_err(|e| PlatformError {
            status: 502,
            message: format!("Invalid JSON from Twilio: {e}"),
        })?;

        if !(200..300).contains(&status) {
            let msg = data
                .get("message")
                .and_then(|m| m.as_str())
                .map(String::from)
                .unwrap_or_else(|| data.to_string());
            return Err(PlatformError { status, message: msg });
        }

        Ok(data)
    }

    async fn edit_message(&self, _: &str, _: &str, _: &str) -> PlatformResult {
        Err(PlatformError {
            status: 400,
            message: "WhatsApp (Twilio) does not support editing messages".into(),
        })
    }

    async fn add_reaction(&self, _: &str, _: &str, _: &str) -> PlatformResult {
        Err(PlatformError {
            status: 400,
            message: "WhatsApp (Twilio) does not support reactions".into(),
        })
    }

    async fn get_messages(&self, _: &str, _: u64) -> PlatformResult {
        Err(PlatformError {
            status: 400,
            message: "WhatsApp (Twilio) does not support fetching channel history".into(),
        })
    }

    async fn get_channel_info(&self, _: &str) -> PlatformResult {
        Err(PlatformError {
            status: 400,
            message: "WhatsApp (Twilio) has no channel info".into(),
        })
    }
}
