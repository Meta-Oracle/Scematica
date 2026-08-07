use scematica_core::config::AlertsConfig;
use tracing::{debug, warn};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
// CREATE_NO_WINDOW: suppresses the console window at the Win32 CreateProcess level,
// preventing any focus-steal / minimize of the foreground TUI window.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000_0000;

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
        // Use System.Windows.Forms.NotifyIcon (balloon tip) — no WinRT dependency,
        // works on all Windows 10/11 versions without type-loading issues.
        // Escape single-quotes for PowerShell string literals.
        let title_safe = title.replace('\'', "''");
        let body_safe  = body.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $n = New-Object System.Windows.Forms.NotifyIcon; \
             $n.Icon = [System.Drawing.SystemIcons]::Application; \
             $n.BalloonTipIcon = 'Info'; \
             $n.BalloonTipTitle = '{}'; \
             $n.BalloonTipText = '{}'; \
             $n.Visible = $true; \
             $n.ShowBalloonTip(4000); \
             Start-Sleep -Milliseconds 4500; \
             $n.Dispose()",
            title_safe, body_safe
        );
        // -NoProfile skips profile scripts (faster startup).
        // CREATE_NO_WINDOW (0x08000000) suppresses the console window at the
        // Win32 CreateProcess level — this is what was causing the TUI to
        // minimize on every buy. -WindowStyle Hidden only hides the window
        // after it's created; CREATE_NO_WINDOW prevents it from ever existing.
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(target_os = "windows")]
        cmd.creation_flags(CREATE_NO_WINDOW);
        let _ = cmd.spawn();
    }
}
