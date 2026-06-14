use crate::ping::{self, PingResult};
use std::net::Ipv4Addr;
use std::process::Command;

const DEFAULT_SIGNAL_LOW_THRESHOLD_DBM: i32 = -75;
const DEFAULT_LINK_QUALITY_LOW_THRESHOLD: u8 = 35;

#[derive(Debug, Clone)]
pub struct WifiConfig {
    pub interface: String,
    pub signal_low_threshold_dbm: i32,
    pub link_quality_low_threshold: u8,
}

impl WifiConfig {
    pub fn new(interface: String) -> Self {
        Self {
            interface,
            signal_low_threshold_dbm: DEFAULT_SIGNAL_LOW_THRESHOLD_DBM,
            link_quality_low_threshold: DEFAULT_LINK_QUALITY_LOW_THRESHOLD,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WifiSample {
    pub enabled: bool,
    pub signal_dbm: Option<i32>,
    pub link_quality: Option<u8>,
    pub tx_bitrate_mbps: Option<f64>,
    pub rx_bitrate_mbps: Option<f64>,
    pub router_ping: PingResult,
    pub internet_ping: PingResult,
    pub disconnect_count: u64,
    pub reconnect_count: u64,
    pub associated_bssid: Option<String>,
    pub channel: Option<u32>,
    pub events: Vec<WifiEvent>,
}

impl Default for WifiSample {
    fn default() -> Self {
        Self {
            enabled: false,
            signal_dbm: None,
            link_quality: None,
            tx_bitrate_mbps: None,
            rx_bitrate_mbps: None,
            router_ping: PingResult::timeout(),
            internet_ping: PingResult::timeout(),
            disconnect_count: 0,
            reconnect_count: 0,
            associated_bssid: None,
            channel: None,
            events: Vec::new(),
        }
    }
}

impl WifiSample {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            router_ping: PingResult::timeout(),
            internet_ping: PingResult::timeout(),
            ..Self::default()
        }
    }

    pub fn associated(&self) -> bool {
        self.associated_bssid.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WifiEvent {
    Disconnected,
    Reconnected,
    BssidChanged { previous: String, current: String },
    ChannelChanged { previous: u32, current: u32 },
    SignalLow { signal_dbm: i32 },
    LinkQualityLow { link_quality: u8 },
}

impl WifiEvent {
    pub fn event_text(&self) -> String {
        match self {
            Self::Disconnected => "WIFI_DISCONNECTED".to_string(),
            Self::Reconnected => "WIFI_RECONNECTED".to_string(),
            Self::BssidChanged { previous, current } => {
                format!("WIFI_BSSID_CHANGED previous={previous} current={current}")
            }
            Self::ChannelChanged { previous, current } => {
                format!("WIFI_CHANNEL_CHANGED previous={previous} current={current}")
            }
            Self::SignalLow { signal_dbm } => {
                format!("WIFI_SIGNAL_LOW signal={signal_dbm}dBm")
            }
            Self::LinkQualityLow { link_quality } => {
                format!("WIFI_LINK_QUALITY_LOW quality={link_quality}")
            }
        }
    }
}

#[derive(Debug)]
pub struct WifiMonitor {
    config: WifiConfig,
    previous_associated: Option<bool>,
    previous_bssid: Option<String>,
    previous_channel: Option<u32>,
    disconnect_count: u64,
    reconnect_count: u64,
}

pub trait ClientLinkMonitor {
    fn sample_client_link(&mut self, router_ip: Ipv4Addr, internet_ip: Ipv4Addr) -> WifiSample;
}

impl WifiMonitor {
    pub fn new(config: WifiConfig) -> Self {
        Self {
            config,
            previous_associated: None,
            previous_bssid: None,
            previous_channel: None,
            disconnect_count: 0,
            reconnect_count: 0,
        }
    }

    pub fn sample(&mut self, router_ip: Ipv4Addr, internet_ip: Ipv4Addr) -> WifiSample {
        self.sample_client_link(router_ip, internet_ip)
    }

    fn detect_events(
        &mut self,
        link: &IwLink,
        _link_quality: Option<u8>,
        associated: bool,
    ) -> Vec<WifiEvent> {
        let mut events = Vec::new();

        match self.previous_associated {
            Some(true) if !associated => {
                self.disconnect_count += 1;
                events.push(WifiEvent::Disconnected);
            }
            Some(false) if associated => {
                self.reconnect_count += 1;
                events.push(WifiEvent::Reconnected);
            }
            _ => {}
        }

        if associated {
            if let (Some(previous), Some(current)) =
                (self.previous_bssid.as_ref(), link.associated_bssid.as_ref())
            {
                if previous != current {
                    events.push(WifiEvent::BssidChanged {
                        previous: previous.clone(),
                        current: current.clone(),
                    });
                }
            }

            if let (Some(previous), Some(current)) = (self.previous_channel, link.channel) {
                if previous != current {
                    events.push(WifiEvent::ChannelChanged { previous, current });
                }
            }
        }

        events
    }
}

impl ClientLinkMonitor for WifiMonitor {
    fn sample_client_link(&mut self, router_ip: Ipv4Addr, internet_ip: Ipv4Addr) -> WifiSample {
        if !wifi_platform_supported() {
            return WifiSample::disabled();
        }

        let link = read_iw_link(&self.config.interface).unwrap_or_default();
        let link_quality = read_link_quality(&self.config.interface);
        let associated = link.associated_bssid.is_some();
        let mut events = self.detect_events(&link, link_quality, associated);
        let router_ping =
            ping::ping_once_via_interface(&router_ip.to_string(), &self.config.interface);
        let internet_ping =
            ping::ping_once_via_interface(&internet_ip.to_string(), &self.config.interface);

        self.previous_associated = Some(associated);
        self.previous_bssid = link.associated_bssid.clone();
        self.previous_channel = link.channel;

        if link
            .signal_dbm
            .map(|signal| signal <= self.config.signal_low_threshold_dbm)
            .unwrap_or(false)
        {
            events.push(WifiEvent::SignalLow {
                signal_dbm: link.signal_dbm.unwrap(),
            });
        }

        if link_quality
            .map(|quality| quality <= self.config.link_quality_low_threshold)
            .unwrap_or(false)
        {
            events.push(WifiEvent::LinkQualityLow {
                link_quality: link_quality.unwrap(),
            });
        }

        WifiSample {
            enabled: true,
            signal_dbm: link.signal_dbm,
            link_quality,
            tx_bitrate_mbps: link.tx_bitrate_mbps,
            rx_bitrate_mbps: link.rx_bitrate_mbps,
            router_ping,
            internet_ping,
            disconnect_count: self.disconnect_count,
            reconnect_count: self.reconnect_count,
            associated_bssid: link.associated_bssid,
            channel: link.channel,
            events,
        }
    }
}

fn wifi_platform_supported() -> bool {
    cfg!(target_os = "linux")
}

#[derive(Debug, Clone, Default, PartialEq)]
struct IwLink {
    associated_bssid: Option<String>,
    signal_dbm: Option<i32>,
    tx_bitrate_mbps: Option<f64>,
    rx_bitrate_mbps: Option<f64>,
    channel: Option<u32>,
}

fn read_iw_link(interface: &str) -> Option<IwLink> {
    let output = Command::new("iw")
        .args(["dev", interface, "link"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(parse_iw_link(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_iw_link(output: &str) -> IwLink {
    let mut link = IwLink::default();

    for line in output.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("Connected to ") {
            link.associated_bssid = rest.split_whitespace().next().map(str::to_string);
        } else if let Some(rest) = line.strip_prefix("signal:") {
            link.signal_dbm = parse_first_i32(rest);
        } else if let Some(rest) = line.strip_prefix("tx bitrate:") {
            link.tx_bitrate_mbps = parse_first_f64(rest);
        } else if let Some(rest) = line.strip_prefix("rx bitrate:") {
            link.rx_bitrate_mbps = parse_first_f64(rest);
        } else if let Some(rest) = line.strip_prefix("freq:") {
            link.channel = parse_first_u32(rest).and_then(frequency_to_channel);
        }
    }

    link
}

fn read_link_quality(interface: &str) -> Option<u8> {
    let content = std::fs::read_to_string("/proc/net/wireless").ok()?;
    parse_proc_net_wireless_quality(&content, interface)
}

fn parse_proc_net_wireless_quality(content: &str, interface: &str) -> Option<u8> {
    for line in content.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(&format!("{interface}:")) else {
            continue;
        };
        let fields = rest.split_whitespace().collect::<Vec<_>>();
        let raw_quality = fields.get(1)?.trim_end_matches('.');
        let quality = raw_quality.parse::<f64>().ok()?;

        return Some(((quality / 70.0) * 100.0).round().clamp(0.0, 100.0) as u8);
    }

    None
}

fn parse_first_i32(input: &str) -> Option<i32> {
    input.split_whitespace().next()?.parse().ok()
}

fn parse_first_u32(input: &str) -> Option<u32> {
    input.split_whitespace().next()?.parse().ok()
}

fn parse_first_f64(input: &str) -> Option<f64> {
    input.split_whitespace().next()?.parse().ok()
}

fn frequency_to_channel(frequency_mhz: u32) -> Option<u32> {
    match frequency_mhz {
        2412..=2472 => Some((frequency_mhz - 2407) / 5),
        2484 => Some(14),
        5180..=5885 => Some((frequency_mhz - 5000) / 5),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_connected_iw_link_output() {
        let output = "\
Connected to 70:3a:cb:12:34:56 (on wlan0)
\tSSID: Home
\tfreq: 2412
\tsignal: -61 dBm
\ttx bitrate: 72.2 MBit/s
\trx bitrate: 86.7 MBit/s
";

        let link = parse_iw_link(output);

        assert_eq!(link.associated_bssid.as_deref(), Some("70:3a:cb:12:34:56"));
        assert_eq!(link.signal_dbm, Some(-61));
        assert_eq!(link.tx_bitrate_mbps, Some(72.2));
        assert_eq!(link.rx_bitrate_mbps, Some(86.7));
        assert_eq!(link.channel, Some(1));
    }

    #[test]
    fn parses_disconnected_iw_link_output() {
        assert_eq!(parse_iw_link("Not connected."), IwLink::default());
    }

    #[test]
    fn parses_proc_net_wireless_quality_as_percentage() {
        let content = "\
Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE
 face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22
wlan0: 0000   49.  -61.  -256        0      0      0      0      0        0
";

        assert_eq!(parse_proc_net_wireless_quality(content, "wlan0"), Some(70));
    }

    #[test]
    fn maps_common_wifi_frequencies_to_channels() {
        assert_eq!(frequency_to_channel(2412), Some(1));
        assert_eq!(frequency_to_channel(2462), Some(11));
        assert_eq!(frequency_to_channel(2484), Some(14));
        assert_eq!(frequency_to_channel(5180), Some(36));
        assert_eq!(frequency_to_channel(5500), Some(100));
        assert_eq!(frequency_to_channel(6000), None);
    }

    #[test]
    fn formats_wifi_event_text() {
        assert_eq!(
            WifiEvent::SignalLow { signal_dbm: -81 }.event_text(),
            "WIFI_SIGNAL_LOW signal=-81dBm"
        );
        assert_eq!(
            WifiEvent::BssidChanged {
                previous: "aa:bb:cc:dd:ee:ff".to_string(),
                current: "11:22:33:44:55:66".to_string(),
            }
            .event_text(),
            "WIFI_BSSID_CHANGED previous=aa:bb:cc:dd:ee:ff current=11:22:33:44:55:66"
        );
    }

    #[test]
    fn detects_wifi_state_transitions() {
        let mut monitor = WifiMonitor::new(WifiConfig::new("wlan0".to_string()));
        let connected = IwLink {
            associated_bssid: Some("aa:bb:cc:dd:ee:ff".to_string()),
            channel: Some(1),
            ..IwLink::default()
        };
        let disconnected = IwLink::default();

        assert!(monitor.detect_events(&connected, Some(70), true).is_empty());
        monitor.previous_associated = Some(true);
        monitor.previous_bssid = connected.associated_bssid.clone();
        monitor.previous_channel = connected.channel;
        assert_eq!(
            monitor.detect_events(&disconnected, None, false),
            vec![WifiEvent::Disconnected]
        );
        monitor.previous_associated = Some(false);
        assert_eq!(
            monitor.detect_events(&connected, Some(70), true),
            vec![WifiEvent::Reconnected]
        );
    }
}
