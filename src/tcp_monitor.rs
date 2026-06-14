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
    connected: bool,
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
            connected: false,
            connected_since: None,
            disconnect_count: 0,
            reconnect_count: 0,
            ever_connected: false,
            last_disconnect: None,
        }
    }

    pub fn sample(&mut self, timestamp: &str, now: Instant) -> TcpSample {
        let mut events = Vec::new();
        let connected = connect_tcp(&self.config.target).is_some();

        if connected {
            if !self.connected {
                self.connected_since = Some(now);

                if self.ever_connected {
                    self.reconnect_count += 1;
                    events.push(TcpEvent::Reconnected);
                }
            }

            self.connected = true;
            self.ever_connected = true;
        } else if self.connected {
            self.record_disconnect(timestamp, &mut events);
        }

        TcpSample {
            connected: self.connected,
            disconnect_count: self.disconnect_count,
            reconnect_count: self.reconnect_count,
            session_uptime_seconds: self
                .connected_since
                .filter(|_| self.connected)
                .map(|connected_since| now.duration_since(connected_since).as_secs()),
            last_disconnect: self.last_disconnect.clone(),
            events,
        }
    }

    fn record_disconnect(&mut self, timestamp: &str, events: &mut Vec<TcpEvent>) {
        if self.connected || self.connected_since.take().is_some() {
            self.connected = false;
            self.disconnect_count += 1;
            self.last_disconnect = Some(timestamp.to_string());
            events.push(TcpEvent::Disconnected);
        }
    }
}

fn connect_tcp(target: &str) -> Option<TcpStream> {
    let address = resolve_first_address(target)?;
    let stream = TcpStream::connect_timeout(&address, TCP_CONNECT_TIMEOUT).ok()?;

    Some(stream)
}

fn resolve_first_address(target: &str) -> Option<SocketAddr> {
    target.to_socket_addrs().ok()?.next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, TcpListener};
    use std::thread;

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

    #[test]
    fn successful_tcp_reachability_keeps_uptime_without_periodic_disconnects() {
        let listener = match local_listener() {
            Some(listener) => listener,
            None => return,
        };
        let target = listener.local_addr().unwrap().to_string();
        let handle = accept_connections(listener, 2);
        let mut monitor = TcpMonitor::new(TcpConfig { target });
        let started_at = Instant::now();

        let first = monitor.sample("2026-06-14 12:00:00", started_at);
        let later = monitor.sample("2026-06-14 12:00:16", started_at + Duration::from_secs(16));

        assert!(first.connected);
        assert!(later.connected);
        assert_eq!(later.disconnect_count, 0);
        assert_eq!(later.reconnect_count, 0);
        assert_eq!(later.session_uptime_seconds, Some(16));
        assert!(later.events.is_empty());

        handle.join().unwrap();
    }

    #[test]
    fn failed_tcp_reachability_after_success_records_one_disconnect() {
        let listener = match local_listener() {
            Some(listener) => listener,
            None => return,
        };
        let target = listener.local_addr().unwrap().to_string();
        let handle = accept_connections(listener, 1);
        let mut monitor = TcpMonitor::new(TcpConfig { target });
        let started_at = Instant::now();

        let connected = monitor.sample("2026-06-14 12:00:00", started_at);
        handle.join().unwrap();
        let disconnected =
            monitor.sample("2026-06-14 12:00:01", started_at + Duration::from_secs(1));
        let still_disconnected =
            monitor.sample("2026-06-14 12:00:02", started_at + Duration::from_secs(2));

        assert!(connected.connected);
        assert!(!disconnected.connected);
        assert_eq!(disconnected.disconnect_count, 1);
        assert_eq!(disconnected.events, vec![TcpEvent::Disconnected]);
        assert_eq!(
            disconnected.last_disconnect.as_deref(),
            Some("2026-06-14 12:00:01")
        );
        assert!(!still_disconnected.connected);
        assert_eq!(still_disconnected.disconnect_count, 1);
        assert!(still_disconnected.events.is_empty());
    }

    fn local_listener() -> Option<TcpListener> {
        match TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) {
            Ok(listener) => Some(listener),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => None,
            Err(error) => panic!("failed to bind local TCP listener: {error}"),
        }
    }

    fn accept_connections(listener: TcpListener, count: usize) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            for _ in 0..count {
                let _ = listener.accept().unwrap();
            }
        })
    }
}
