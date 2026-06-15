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
    ping_once_with_interface(host, None)
}

pub fn ping_once_via_interface(host: &str, interface: &str) -> PingResult {
    ping_once_with_interface(host, Some(interface))
}

fn ping_once_with_interface(host: &str, interface: Option<&str>) -> PingResult {
    if interface.is_some() && !interface_ping_supported() {
        return PingResult::timeout();
    }

    let mut command = Command::new("ping");
    command.args(ping_args(host, interface));

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PingPlatform {
    Linux,
    MacOs,
    Windows,
    OtherUnix,
}

fn ping_args(host: &str, interface: Option<&str>) -> Vec<String> {
    ping_args_for_platform(host, interface, current_platform())
}

fn current_platform() -> PingPlatform {
    if cfg!(target_os = "linux") {
        PingPlatform::Linux
    } else if cfg!(target_os = "macos") {
        PingPlatform::MacOs
    } else if cfg!(target_os = "windows") {
        PingPlatform::Windows
    } else {
        PingPlatform::OtherUnix
    }
}

/// Whether ICMP can be bound to a specific network interface on this platform.
/// Linux uses `ping -I <iface>` and macOS uses `ping -b <boundif>`; Windows and
/// other platforms have no equivalent, so WiFi-bound pings are skipped there.
pub fn interface_binding_supported() -> bool {
    interface_ping_supported()
}

fn interface_ping_supported() -> bool {
    matches!(
        current_platform(),
        PingPlatform::Linux | PingPlatform::MacOs
    )
}

fn ping_args_for_platform(
    host: &str,
    interface: Option<&str>,
    platform: PingPlatform,
) -> Vec<String> {
    let mut args = match platform {
        PingPlatform::Linux => vec!["-c", "1", "-n", "-W", "1"],
        // macOS uses milliseconds for the -W timeout value.
        PingPlatform::MacOs => vec!["-c", "1", "-n", "-W", "1000"],
        PingPlatform::Windows => vec!["-n", "1", "-w", "1000"],
        PingPlatform::OtherUnix => vec!["-c", "1"],
    }
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();

    if let Some(interface) = interface {
        // Linux binds with `-I <iface>`; macOS binds with `-b <boundif>`.
        match platform {
            PingPlatform::Linux => {
                args.push("-I".to_string());
                args.push(interface.to_string());
            }
            PingPlatform::MacOs => {
                args.push("-b".to_string());
                args.push(interface.to_string());
            }
            _ => {}
        }
    }

    args.push(host.to_string());
    args
}

fn parse_latency_ms(output: &str) -> Option<f64> {
    output.lines().find_map(|line| {
        parse_latency_from_line(line, "time=").or_else(|| parse_sub_ms_latency(line))
    })
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

fn parse_sub_ms_latency(line: &str) -> Option<f64> {
    if line.contains("time<1ms") || line.contains("time<1 ms") {
        Some(0.5)
    } else {
        None
    }
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
    fn parses_windows_latency_output() {
        let output = "Reply from 1.1.1.1: bytes=32 time=8ms TTL=57";
        assert_eq!(parse_latency_ms(output), Some(8.0));
    }

    #[test]
    fn parses_windows_sub_millisecond_latency_output() {
        let output = "Reply from 192.168.1.1: bytes=32 time<1ms TTL=64";
        assert_eq!(parse_latency_ms(output), Some(0.5));
    }

    #[test]
    fn returns_none_when_latency_is_missing() {
        assert_eq!(parse_latency_ms("Request timeout for icmp_seq 0"), None);
    }

    #[test]
    fn builds_linux_ping_args_with_optional_interface() {
        assert_eq!(
            ping_args_for_platform("1.1.1.1", Some("wlan0"), PingPlatform::Linux),
            vec!["-c", "1", "-n", "-W", "1", "-I", "wlan0", "1.1.1.1"]
        );
    }

    #[test]
    fn builds_macos_ping_args_with_boundif_interface_binding() {
        assert_eq!(
            ping_args_for_platform("1.1.1.1", Some("en0"), PingPlatform::MacOs),
            vec!["-c", "1", "-n", "-W", "1000", "-b", "en0", "1.1.1.1"]
        );
    }

    #[test]
    fn builds_macos_ping_args_without_interface() {
        assert_eq!(
            ping_args_for_platform("1.1.1.1", None, PingPlatform::MacOs),
            vec!["-c", "1", "-n", "-W", "1000", "1.1.1.1"]
        );
    }

    #[test]
    fn builds_windows_ping_args_without_interface_binding() {
        assert_eq!(
            ping_args_for_platform("1.1.1.1", Some("Wi-Fi"), PingPlatform::Windows),
            vec!["-n", "1", "-w", "1000", "1.1.1.1"]
        );
    }
}
