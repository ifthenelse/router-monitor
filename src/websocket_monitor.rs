use std::net::TcpStream;
use std::time::{Duration, Instant};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

const WEBSOCKET_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
pub const DEFAULT_WEBSOCKET_URL: &str = "wss://echo.websocket.org";

#[derive(Debug, Clone)]
pub struct WebSocketConfig {
    pub url: Option<String>,
}

impl Default for WebSocketConfig {
    fn default() -> Self {
        Self {
            url: Some(DEFAULT_WEBSOCKET_URL.to_string()),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WebSocketSample {
    pub enabled: bool,
    pub connected: bool,
    pub disconnect_count: u64,
    pub reconnect_count: u64,
    pub uptime_seconds: Option<u64>,
    pub last_disconnect: Option<String>,
    pub events: Vec<WebSocketEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSocketEvent {
    Disconnected,
    Reconnected,
}

impl WebSocketEvent {
    pub fn event_text(&self) -> &'static str {
        match self {
            Self::Disconnected => "WEBSOCKET_DISCONNECTED",
            Self::Reconnected => "WEBSOCKET_RECONNECTED",
        }
    }
}

pub struct WebSocketMonitor {
    config: WebSocketConfig,
    socket: Option<WebSocket<MaybeTlsStream<TcpStream>>>,
    connected_since: Option<Instant>,
    last_heartbeat: Option<Instant>,
    disconnect_count: u64,
    reconnect_count: u64,
    ever_connected: bool,
    last_disconnect: Option<String>,
}

impl WebSocketMonitor {
    pub fn new(config: WebSocketConfig) -> Self {
        Self {
            config,
            socket: None,
            connected_since: None,
            last_heartbeat: None,
            disconnect_count: 0,
            reconnect_count: 0,
            ever_connected: false,
            last_disconnect: None,
        }
    }

    pub fn sample(&mut self, timestamp: &str, now: Instant) -> WebSocketSample {
        let Some(url) = self.config.url.clone() else {
            return WebSocketSample::default();
        };
        let mut events = Vec::new();

        if self.socket.is_none() {
            self.connect(&url, now, &mut events);
        }

        if self.should_send_heartbeat(now) {
            if let Some(socket) = self.socket.as_mut() {
                if socket.send(Message::Ping(Vec::new().into())).is_err() {
                    self.record_disconnect(timestamp, &mut events);
                } else {
                    self.last_heartbeat = Some(now);
                }
            }
        }

        WebSocketSample {
            enabled: true,
            connected: self.socket.is_some(),
            disconnect_count: self.disconnect_count,
            reconnect_count: self.reconnect_count,
            uptime_seconds: self
                .connected_since
                .filter(|_| self.socket.is_some())
                .map(|connected_since| now.duration_since(connected_since).as_secs()),
            last_disconnect: self.last_disconnect.clone(),
            events,
        }
    }

    fn connect(&mut self, url: &str, now: Instant, events: &mut Vec<WebSocketEvent>) {
        match connect(url) {
            Ok((socket, _)) => {
                self.socket = Some(socket);
                self.connected_since = Some(now);
                self.last_heartbeat = None;

                if self.ever_connected {
                    self.reconnect_count += 1;
                    events.push(WebSocketEvent::Reconnected);
                }

                self.ever_connected = true;
            }
            Err(_) => {
                if self.ever_connected {
                    self.record_disconnect("", events);
                }
            }
        }
    }

    fn should_send_heartbeat(&self, now: Instant) -> bool {
        self.socket.is_some()
            && self
                .last_heartbeat
                .map(|last_heartbeat| {
                    now.duration_since(last_heartbeat) >= WEBSOCKET_HEARTBEAT_INTERVAL
                })
                .unwrap_or(true)
    }

    fn record_disconnect(&mut self, timestamp: &str, events: &mut Vec<WebSocketEvent>) {
        if self.socket.take().is_some() || self.connected_since.take().is_some() {
            self.disconnect_count += 1;
            self.last_disconnect = if timestamp.is_empty() {
                None
            } else {
                Some(timestamp.to_string())
            };
            events.push(WebSocketEvent::Disconnected);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_websocket_url_uses_public_echo_server() {
        assert_eq!(
            WebSocketConfig::default().url.as_deref(),
            Some(DEFAULT_WEBSOCKET_URL)
        );
    }

    #[test]
    fn websocket_monitor_can_be_disabled_without_url() {
        let mut monitor = WebSocketMonitor::new(WebSocketConfig { url: None });
        let sample = monitor.sample("2026-06-14 01:00:00", Instant::now());

        assert!(!sample.enabled);
        assert!(!sample.connected);
    }

    #[test]
    fn websocket_event_text_is_stable() {
        assert_eq!(
            WebSocketEvent::Disconnected.event_text(),
            "WEBSOCKET_DISCONNECTED"
        );
        assert_eq!(
            WebSocketEvent::Reconnected.event_text(),
            "WEBSOCKET_RECONNECTED"
        );
    }

    #[test]
    fn websocket_monitor_with_bad_url_stays_enabled_but_disconnected() {
        let mut monitor = WebSocketMonitor::new(WebSocketConfig {
            url: Some("not-a-websocket-url".to_string()),
        });
        let sample = monitor.sample("2026-06-14 01:00:00", Instant::now());

        assert!(sample.enabled);
        assert!(!sample.connected);
        assert_eq!(sample.disconnect_count, 0);
    }
}
