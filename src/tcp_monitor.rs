use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct TcpConfig {
    pub target: String,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            target: "1.1.1.1:443".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TcpSample {
    pub connected: bool,
    pub disconnect_count: u64,
    pub reconnect_count: u64,
    pub session_uptime_seconds: Option<u64>,
    pub last_disconnect: Option<String>,
    pub events: Vec<TcpEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpEvent {
    Disconnected,
    Reconnected,
}

impl TcpEvent {
    pub fn event_text(&self) -> &'static str {
        match self {
            Self::Disconnected => "TCP_DISCONNECTED",
            Self::Reconnected => "TCP_RECONNECTED",
        }
    }
}

#[derive(Debug)]
pub struct TcpMonitor {
    config: TcpConfig,
    stream: Option<TcpStream>,
    connected_since: Option<Instant>,
    disconnect_count: u64,
    reconnect_count: u64,
    ever_connected: bool,
    last_disconnect: Option<String>,
}

impl TcpMonitor {
    pub fn new(config: TcpConfig) -> Self {
        Self {
            config,
            stream: None,
            connected_since: None,
            disconnect_count: 0,
            reconnect_count: 0,
            ever_connected: false,
            last_disconnect: None,
        }
    }

    pub fn sample(&mut self, timestamp: &str, now: Instant) -> TcpSample {
        let mut events = Vec::new();

        if self.stream.as_ref().is_some_and(stream_disconnected) {
            self.record_disconnect(timestamp, &mut events);
        }

        if self.stream.is_none() {
            match connect_tcp(&self.config.target) {
                Some(stream) => {
                    self.stream = Some(stream);
                    self.connected_since = Some(now);

                    if self.ever_connected {
                        self.reconnect_count += 1;
                        events.push(TcpEvent::Reconnected);
                    }

                    self.ever_connected = true;
                }
                None if self.ever_connected => {
                    self.record_disconnect(timestamp, &mut events);
                }
                None => {}
            }
        }

        TcpSample {
            connected: self.stream.is_some(),
            disconnect_count: self.disconnect_count,
            reconnect_count: self.reconnect_count,
            session_uptime_seconds: self
                .connected_since
                .filter(|_| self.stream.is_some())
                .map(|connected_since| now.duration_since(connected_since).as_secs()),
            last_disconnect: self.last_disconnect.clone(),
            events,
        }
    }

    fn record_disconnect(&mut self, timestamp: &str, events: &mut Vec<TcpEvent>) {
        if self.stream.take().is_some() || self.connected_since.take().is_some() {
            self.disconnect_count += 1;
            self.last_disconnect = Some(timestamp.to_string());
            events.push(TcpEvent::Disconnected);
        }
    }
}

fn connect_tcp(target: &str) -> Option<TcpStream> {
    let address = resolve_first_address(target)?;
    let stream = TcpStream::connect_timeout(&address, TCP_CONNECT_TIMEOUT).ok()?;

    let _ = stream.set_nonblocking(true);
    Some(stream)
}

fn resolve_first_address(target: &str) -> Option<SocketAddr> {
    target.to_socket_addrs().ok()?.next()
}

fn stream_disconnected(stream: &TcpStream) -> bool {
    let mut buffer = [0u8; 1];

    match stream.peek(&mut buffer) {
        Ok(0) => true,
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => false,
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tcp_target_is_cloudflare_https() {
        assert_eq!(TcpConfig::default().target, "1.1.1.1:443");
    }

    #[test]
    fn resolves_socket_address() {
        assert!(resolve_first_address("127.0.0.1:443").is_some());
    }

    #[test]
    fn tcp_event_text_is_stable() {
        assert_eq!(TcpEvent::Disconnected.event_text(), "TCP_DISCONNECTED");
        assert_eq!(TcpEvent::Reconnected.event_text(), "TCP_RECONNECTED");
    }

    #[test]
    fn invalid_tcp_target_yields_disconnected_sample_without_initial_event() {
        let mut monitor = TcpMonitor::new(TcpConfig {
            target: "not a socket address".to_string(),
        });
        let sample = monitor.sample("2026-06-14 12:00:00", Instant::now());

        assert!(!sample.connected);
        assert_eq!(sample.disconnect_count, 0);
        assert!(sample.events.is_empty());
    }
}
