use std::net::ToSocketAddrs;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub const DNS_HOST: &str = "google.com";
const DNS_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsStatus {
    Ok,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DnsResult {
    pub latency_ms: Option<f64>,
    pub status: DnsStatus,
}

#[derive(Debug, Clone, Copy)]
pub struct DnsMonitor {
    timeout: Duration,
}

impl DnsMonitor {
    pub fn new() -> Self {
        Self {
            timeout: DNS_TIMEOUT,
        }
    }

    pub fn measure_google(&self) -> DnsResult {
        self.measure_host(DNS_HOST)
    }

    fn measure_host(&self, host: &str) -> DnsResult {
        let host = host.to_string();

        self.measure_with(move || {
            (host.as_str(), 443)
                .to_socket_addrs()
                .map(|mut addresses| addresses.next().is_some())
                .unwrap_or(false)
        })
    }

    fn measure_with<F>(&self, lookup: F) -> DnsResult
    where
        F: FnOnce() -> bool + Send + 'static,
    {
        let started_at = Instant::now();
        let (sender, receiver) = mpsc::channel();

        thread::spawn(move || {
            let _ = sender.send(lookup());
        });

        match receiver.recv_timeout(self.timeout) {
            Ok(true) => DnsResult::ok(started_at.elapsed().as_secs_f64() * 1_000.0),
            Ok(false) | Err(_) => DnsResult::timeout(),
        }
    }
}

impl DnsResult {
    pub fn timeout() -> Self {
        Self {
            latency_ms: None,
            status: DnsStatus::Timeout,
        }
    }

    pub fn ok(latency_ms: f64) -> Self {
        Self {
            latency_ms: Some(latency_ms),
            status: DnsStatus::Ok,
        }
    }
}

impl DnsStatus {
    pub fn as_csv_value(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Timeout => "timeout",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_successful_dns_resolution() {
        let monitor = DnsMonitor {
            timeout: Duration::from_millis(50),
        };
        let result = monitor.measure_with(|| true);

        assert_eq!(result.status, DnsStatus::Ok);
        assert!(result.latency_ms.unwrap() >= 0.0);
    }

    #[test]
    fn records_dns_lookup_timeout() {
        let monitor = DnsMonitor {
            timeout: Duration::from_millis(10),
        };
        let result = monitor.measure_with(|| {
            thread::sleep(Duration::from_millis(50));
            true
        });

        assert_eq!(result, DnsResult::timeout());
    }

    #[test]
    fn dns_status_values_are_csv_safe() {
        assert_eq!(DnsStatus::Ok.as_csv_value(), "ok");
        assert_eq!(DnsStatus::Timeout.as_csv_value(), "timeout");
    }
}
