use scematica_core::config::AlertsConfig;
use tracing::{debug, warn};

/// Sends trade alerts via Telegram, Discord, and/or desktop notifications.
#[derive(Clone)]
pub struct AlertManager {
    config: AlertsConfig,
    http: reqwest::Client,
}

impl AlertManager {
    pub fn new(config: AlertsConfig) -> Self {
        Self { config, http: reqwest::Client::new() }
    }

    pub async fn send(&self, title: &str, body: &str) {
        if !self.config.telegram_bot_token.is_empty() && !self.config.telegram_chat_id.is_empty() {
            self.send_telegram(title, body).await;
        }
        if !self.config.discord_webhook_url.is_empty() {
            self.send_discord(title, body).await;
        }
        if self.config.desktop_notifications {
            self.send_desktop(title, body);
        }
    }

    async fn send_telegram(&self, title: &str, body: &str) {
        let text = format!("*{}*\n{}", title, body);
        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.config.telegram_bot_token
        );
        let payload = serde_json::json!({
            "chat_id": self.config.telegram_chat_id,
            "text": text,
            "parse_mode": "Markdown"
        });
        match self.http.post(&url).json(&payload).send().await {
            Ok(r) if r.status().is_success() => debug!("Telegram alert sent"),
            Ok(r) => warn!("Telegram alert failed: {}", r.status()),
            Err(e) => warn!("Telegram alert error: {}", e),
        }
    }

    async fn send_discord(&self, title: &str, body: &str) {
        let payload = serde_json::json!({
            "embeds": [{
                "title": title,
                "description": body,
                "color": 0x00ff88
            }]
        });
        match self.http.post(&self.config.discord_webhook_url).json(&payload).send().await {
            Ok(r) if r.status().is_success() => debug!("Discord alert sent"),
            Ok(r) => warn!("Discord alert failed: {}", r.status()),
            Err(e) => warn!("Discord alert error: {}", e),
        }
    }

    fn send_desktop(&self, title: &str, body: &str) {
        // Windows toast via PowerShell — fire-and-forget, non-blocking
        let script = format!(
            r#"[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null; $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); $t.GetElementsByTagName('text')[0].AppendChild($t.CreateTextNode('{}')); $t.GetElementsByTagName('text')[1].AppendChild($t.CreateTextNode('{}')); [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('Scematica').Show([Windows.UI.Notifications.ToastNotification]::new($t))"#,
            title.replace('\'', "\\'"),
            body.replace('\'', "\\'")
        );
        let _ = std::process::Command::new("powershell")
            .args(["-WindowStyle", "Hidden", "-NonInteractive", "-Command", &script])
            .spawn();
    }
}
