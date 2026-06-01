use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PingStatus {
    Ok,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PingResult {
    pub latency_ms: Option<f64>,
    pub status: PingStatus,
}

impl PingResult {
    pub fn timeout() -> Self {
        Self {
            latency_ms: None,
            status: PingStatus::Timeout,
        }
    }

    pub fn ok(latency_ms: f64) -> Self {
        Self {
            latency_ms: Some(latency_ms),
            status: PingStatus::Ok,
        }
    }
}

impl PingStatus {
    pub fn as_csv_value(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Timeout => "timeout",
        }
    }
}

pub fn ping_once(host: &str) -> PingResult {
    let mut command = Command::new("ping");
    command.args(ping_args(host));

    let output = match command.output() {
        Ok(output) => output,
        Err(_) => return PingResult::timeout(),
    };

    if !output.status.success() {
        return PingResult::timeout();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    parse_latency_ms(&stdout)
        .map(PingResult::ok)
        .unwrap_or_else(PingResult::timeout)
}

#[cfg(target_os = "linux")]
fn ping_args(host: &str) -> Vec<&str> {
    // Linux and Raspberry Pi OS use seconds for the -W timeout value.
    vec!["-c", "1", "-n", "-W", "1", host]
}

#[cfg(target_os = "macos")]
fn ping_args(host: &str) -> Vec<&str> {
    // macOS uses milliseconds for the -W timeout value.
    vec!["-c", "1", "-n", "-W", "1000", host]
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn ping_args(host: &str) -> Vec<&str> {
    vec!["-c", "1", host]
}

fn parse_latency_ms(output: &str) -> Option<f64> {
    output
        .lines()
        .find_map(|line| parse_latency_from_line(line, "time="))
}

fn parse_latency_from_line(line: &str, marker: &str) -> Option<f64> {
    let start = line.find(marker)? + marker.len();
    let value = line[start..].trim_start();
    let end = value
        .char_indices()
        .take_while(|(_, character)| character.is_ascii_digit() || *character == '.')
        .map(|(index, character)| index + character.len_utf8())
        .last()?;

    value[..end].parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_latency_output() {
        let output = "64 bytes from 1.1.1.1: icmp_seq=1 ttl=57 time=8.21 ms";
        assert_eq!(parse_latency_ms(output), Some(8.21));
    }

    #[test]
    fn parses_macos_latency_output() {
        let output = "64 bytes from 1.1.1.1: icmp_seq=0 ttl=57 time=8.210 ms";
        assert_eq!(parse_latency_ms(output), Some(8.210));
    }

    #[test]
    fn returns_none_when_latency_is_missing() {
        assert_eq!(parse_latency_ms("Request timeout for icmp_seq 0"), None);
    }
}
