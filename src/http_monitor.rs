use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

const HTTP_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpStatus {
    Ok,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HttpResult {
    pub latency_ms: Option<f64>,
    pub status: HttpStatus,
}

#[derive(Debug, Clone)]
pub struct HttpMonitor {
    client: reqwest::blocking::Client,
}

impl HttpMonitor {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = reqwest::blocking::Client::builder()
            // Router login pages commonly redirect. Redirects are useful in a
            // browser, but here the first valid response is the signal we need.
            .redirect(reqwest::redirect::Policy::none())
            // Router checks must be direct. CI and user shells may define HTTP
            // proxy variables that would otherwise intercept local/router URLs.
            .no_proxy()
            .connect_timeout(HTTP_TIMEOUT)
            .timeout(HTTP_TIMEOUT)
            .build()?;

        Ok(Self { client })
    }

    pub fn measure_router(&self, router_ip: Ipv4Addr) -> HttpResult {
        self.measure_url(&router_http_url(router_ip))
    }

    fn measure_url(&self, url: &str) -> HttpResult {
        let started_at = Instant::now();

        match self.client.get(url).send() {
            Ok(_) => HttpResult::ok(started_at.elapsed().as_secs_f64() * 1_000.0),
            Err(_) => HttpResult::timeout(),
        }
    }
}

impl HttpResult {
    pub fn timeout() -> Self {
        Self {
            latency_ms: None,
            status: HttpStatus::Timeout,
        }
    }

    pub fn ok(latency_ms: f64) -> Self {
        Self {
            latency_ms: Some(latency_ms),
            status: HttpStatus::Ok,
        }
    }
}

impl HttpStatus {
    pub fn as_csv_value(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Timeout => "timeout",
        }
    }
}

pub fn router_http_url(router_ip: Ipv4Addr) -> String {
    format!("http://{router_ip}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpListener};
    use std::thread;

    #[test]
    fn builds_default_router_http_url_from_router_ip() {
        assert_eq!(
            router_http_url(Ipv4Addr::new(192, 168, 0, 1)),
            "http://192.168.0.1"
        );
    }

    #[test]
    fn treats_any_http_response_as_success() {
        let listener = match TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipping loopback HTTP test because listener bind is denied");
                return;
            }
            Err(error) => panic!("failed to bind loopback listener: {error}"),
        };
        let address = listener.local_addr().unwrap();

        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));

            let mut request = [0_u8; 512];
            let _ = stream.read(&mut request);

            stream
                .write_all(
                    b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
            stream.flush().unwrap();
        });

        let monitor = HttpMonitor::new().unwrap();
        let result = monitor.measure_url(&format!("http://{address}"));

        assert_eq!(result.status, HttpStatus::Ok);
        assert!(result.latency_ms.unwrap() >= 0.0);
    }
}
