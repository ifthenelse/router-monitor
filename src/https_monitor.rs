use std::time::{Duration, Instant};

pub const HTTPS_GOOGLE_URL: &str = "https://www.google.com";
pub const HTTPS_CLOUDFLARE_URL: &str = "https://1.1.1.1";
const HTTPS_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpsStatus {
    Ok,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HttpsResult {
    pub latency_ms: Option<f64>,
    pub status: HttpsStatus,
}

#[derive(Debug, Clone)]
pub struct HttpsMonitor {
    client: reqwest::blocking::Client,
}

impl HttpsMonitor {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(HTTPS_TIMEOUT)
            .timeout(HTTPS_TIMEOUT)
            .build()?;

        Ok(Self { client })
    }

    pub fn measure_google(&self) -> HttpsResult {
        self.measure_url(HTTPS_GOOGLE_URL)
    }

    pub fn measure_cloudflare(&self) -> HttpsResult {
        self.measure_url(HTTPS_CLOUDFLARE_URL)
    }

    fn measure_url(&self, url: &str) -> HttpsResult {
        let started_at = Instant::now();

        match self.client.get(url).send() {
            Ok(_) => HttpsResult::ok(started_at.elapsed().as_secs_f64() * 1_000.0),
            Err(_) => HttpsResult::timeout(),
        }
    }
}

impl HttpsResult {
    pub fn timeout() -> Self {
        Self {
            latency_ms: None,
            status: HttpsStatus::Timeout,
        }
    }

    pub fn ok(latency_ms: f64) -> Self {
        Self {
            latency_ms: Some(latency_ms),
            status: HttpsStatus::Ok,
        }
    }
}

impl HttpsStatus {
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
    fn records_successful_https_result() {
        let result = HttpsResult::ok(24.31);

        assert_eq!(result.status, HttpsStatus::Ok);
        assert_eq!(result.latency_ms, Some(24.31));
    }

    #[test]
    fn records_https_timeout_result() {
        let monitor = HttpsMonitor::new().unwrap();
        let result = monitor.measure_url("https://127.0.0.1:9");

        assert_eq!(result.status, HttpsStatus::Timeout);
        assert_eq!(result.latency_ms, None);
    }

    #[test]
    fn https_status_values_are_csv_safe() {
        assert_eq!(HttpsStatus::Ok.as_csv_value(), "ok");
        assert_eq!(HttpsStatus::Timeout.as_csv_value(), "timeout");
    }
}
