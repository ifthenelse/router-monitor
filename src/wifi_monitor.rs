use crate::ping::{self, PingResult};
use std::fs;
use std::net::Ipv4Addr;
use std::path::Path;
use std::process::Command;

const DEFAULT_SIGNAL_LOW_THRESHOLD_DBM: i32 = -75;
const DEFAULT_LINK_QUALITY_LOW_THRESHOLD: u8 = 35;

#[derive(Debug, Clone)]
pub struct WifiConfig {
    pub interface: WifiInterfaceSelection,
    pub signal_low_threshold_dbm: i32,
    pub link_quality_low_threshold: u8,
}

impl WifiConfig {
    pub fn new(interface: String) -> Self {
        Self {
            interface: WifiInterfaceSelection::Explicit(interface),
            signal_low_threshold_dbm: DEFAULT_SIGNAL_LOW_THRESHOLD_DBM,
            link_quality_low_threshold: DEFAULT_LINK_QUALITY_LOW_THRESHOLD,
        }
    }

    pub fn auto() -> Self {
        Self {
            interface: WifiInterfaceSelection::Auto,
            signal_low_threshold_dbm: DEFAULT_SIGNAL_LOW_THRESHOLD_DBM,
            link_quality_low_threshold: DEFAULT_LINK_QUALITY_LOW_THRESHOLD,
        }
    }

    pub fn explicit_interface(&self) -> Option<&str> {
        match &self.interface {
            WifiInterfaceSelection::Auto => None,
            WifiInterfaceSelection::Explicit(interface) => Some(interface),
        }
    }

    pub fn description(&self) -> String {
        match &self.interface {
            WifiInterfaceSelection::Auto => "auto".to_string(),
            WifiInterfaceSelection::Explicit(interface) => interface.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WifiInterfaceSelection {
    Auto,
    Explicit(String),
}

#[derive(Debug, Clone)]
pub struct WifiSample {
    pub enabled: bool,
    pub interface: Option<String>,
    pub signal_dbm: Option<i32>,
    pub link_quality: Option<u8>,
    pub tx_bitrate_mbps: Option<f64>,
    pub rx_bitrate_mbps: Option<f64>,
    pub router_ping: PingResult,
    pub internet_ping: PingResult,
    /// Whether the router/internet pings were bound to the WiFi interface. When
    /// the platform cannot bind ICMP to a specific interface (e.g. Windows),
    /// the bound pings are skipped and must not be treated as link evidence.
    pub bound_pings_supported: bool,
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
            interface: None,
            signal_dbm: None,
            link_quality: None,
            tx_bitrate_mbps: None,
            rx_bitrate_mbps: None,
            router_ping: PingResult::timeout(),
            internet_ping: PingResult::timeout(),
            bound_pings_supported: true,
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
            || (self.enabled
                && self.bound_pings_supported
                && (self.router_ping.status == crate::ping::PingStatus::Ok
                    || self.internet_ping.status == crate::ping::PingStatus::Ok))
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
    current_interface: Option<String>,
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
            current_interface: None,
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

    fn resolve_interface(&self) -> Option<String> {
        match &self.config.interface {
            WifiInterfaceSelection::Explicit(interface) => Some(interface.clone()),
            WifiInterfaceSelection::Auto => detect_connected_wifi_interface(),
        }
    }

    fn reset_state_for_interface(&mut self, interface: &str) {
        if self.current_interface.as_deref() == Some(interface) {
            return;
        }

        self.current_interface = Some(interface.to_string());
        self.previous_associated = None;
        self.previous_bssid = None;
        self.previous_channel = None;
        self.disconnect_count = 0;
        self.reconnect_count = 0;
    }

    fn detect_events(&mut self, link: &WifiLink, associated: bool) -> Vec<WifiEvent> {
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

        let Some(interface) = self.resolve_interface() else {
            return WifiSample::disabled();
        };

        self.reset_state_for_interface(&interface);

        let link = read_wifi_link(&interface);
        let associated = link.associated_bssid.is_some();
        let mut events = self.detect_events(&link, associated);
        let bound_pings_supported = ping::interface_binding_supported();
        let router_ping = ping::ping_once_via_interface(&router_ip.to_string(), &interface);
        let internet_ping = ping::ping_once_via_interface(&internet_ip.to_string(), &interface);

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

        if link
            .link_quality
            .map(|quality| quality <= self.config.link_quality_low_threshold)
            .unwrap_or(false)
        {
            events.push(WifiEvent::LinkQualityLow {
                link_quality: link.link_quality.unwrap(),
            });
        }

        WifiSample {
            enabled: true,
            interface: Some(interface),
            signal_dbm: link.signal_dbm,
            link_quality: link.link_quality,
            tx_bitrate_mbps: link.tx_bitrate_mbps,
            rx_bitrate_mbps: link.rx_bitrate_mbps,
            router_ping,
            internet_ping,
            bound_pings_supported,
            disconnect_count: self.disconnect_count,
            reconnect_count: self.reconnect_count,
            associated_bssid: link.associated_bssid,
            channel: link.channel,
            events,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WifiPlatform {
    Linux,
    MacOs,
    Windows,
    Unsupported,
}

fn current_wifi_platform() -> WifiPlatform {
    if cfg!(target_os = "linux") {
        WifiPlatform::Linux
    } else if cfg!(target_os = "macos") {
        WifiPlatform::MacOs
    } else if cfg!(target_os = "windows") {
        WifiPlatform::Windows
    } else {
        WifiPlatform::Unsupported
    }
}

fn wifi_platform_supported() -> bool {
    !matches!(current_wifi_platform(), WifiPlatform::Unsupported)
}

/// Platform-neutral snapshot of a client WiFi link. Each platform fills in the
/// fields its native tooling can report; unavailable fields stay `None`.
#[derive(Debug, Clone, Default, PartialEq)]
struct WifiLink {
    associated_bssid: Option<String>,
    signal_dbm: Option<i32>,
    link_quality: Option<u8>,
    tx_bitrate_mbps: Option<f64>,
    rx_bitrate_mbps: Option<f64>,
    channel: Option<u32>,
}

impl WifiLink {
    /// Fills any field that is still `None` from `other`, preserving values that
    /// a higher-priority source already provided.
    fn fill_missing_from(&mut self, other: WifiLink) {
        self.associated_bssid = self.associated_bssid.take().or(other.associated_bssid);
        self.signal_dbm = self.signal_dbm.or(other.signal_dbm);
        self.link_quality = self.link_quality.or(other.link_quality);
        self.tx_bitrate_mbps = self.tx_bitrate_mbps.or(other.tx_bitrate_mbps);
        self.rx_bitrate_mbps = self.rx_bitrate_mbps.or(other.rx_bitrate_mbps);
        self.channel = self.channel.or(other.channel);
    }
}

fn read_wifi_link(interface: &str) -> WifiLink {
    match current_wifi_platform() {
        WifiPlatform::Linux => read_linux_link(interface),
        WifiPlatform::MacOs => read_macos_link(interface).unwrap_or_default(),
        WifiPlatform::Windows => read_windows_link(interface).unwrap_or_default(),
        WifiPlatform::Unsupported => WifiLink::default(),
    }
}

fn detect_connected_wifi_interface() -> Option<String> {
    match current_wifi_platform() {
        WifiPlatform::Linux => select_connected_wifi_interface(
            read_linux_wireless_interface_names(),
            read_linux_link_opt,
            interface_appears_active,
        ),
        WifiPlatform::MacOs => select_connected_wifi_interface(
            read_macos_wifi_interface_names(),
            read_macos_link,
            |_| true,
        ),
        WifiPlatform::Windows => select_connected_wifi_interface(
            read_windows_wifi_interface_names(),
            read_windows_link,
            |_| true,
        ),
        WifiPlatform::Unsupported => None,
    }
}

fn select_connected_wifi_interface(
    candidates: Vec<String>,
    read_link: impl Fn(&str) -> Option<WifiLink>,
    is_active: impl Fn(&str) -> bool,
) -> Option<String> {
    let mut active_fallback = None;

    for interface in candidates {
        if read_link(&interface)
            .map(|link| link.associated_bssid.is_some())
            .unwrap_or(false)
        {
            return Some(interface);
        }

        if active_fallback.is_none() && is_active(&interface) {
            active_fallback = Some(interface);
        }
    }

    active_fallback
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.contains(&value) {
        values.push(value);
    }
}

// ------------------------------- Linux -------------------------------

fn read_linux_wireless_interface_names() -> Vec<String> {
    let mut interfaces = Vec::new();

    fs::read_to_string("/proc/net/wireless")
        .map(|content| parse_proc_net_wireless_interfaces(&content))
        .unwrap_or_default()
        .into_iter()
        .for_each(|interface| push_unique(&mut interfaces, interface));

    read_sysfs_wireless_interfaces(Path::new("/sys/class/net"))
        .into_iter()
        .for_each(|interface| push_unique(&mut interfaces, interface));

    interfaces
}

fn parse_proc_net_wireless_interfaces(content: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| line.trim().split_once(':'))
        .map(|(interface, _)| interface.trim().to_string())
        .filter(|interface| !interface.is_empty())
        .collect()
}

fn read_sysfs_wireless_interfaces(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };

    let mut interfaces = entries
        .flatten()
        .filter(|entry| entry.path().join("wireless").is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect::<Vec<_>>();

    interfaces.sort();
    interfaces
}

fn interface_appears_active(interface: &str) -> bool {
    sysfs_interface_appears_active(&Path::new("/sys/class/net").join(interface))
}

fn sysfs_interface_appears_active(interface_path: &Path) -> bool {
    let carrier_up = fs::read_to_string(interface_path.join("carrier"))
        .map(|carrier| carrier.trim() == "1")
        .unwrap_or(false);

    if carrier_up {
        return true;
    }

    fs::read_to_string(interface_path.join("operstate"))
        .map(|state| matches!(state.trim(), "up" | "dormant" | "unknown"))
        .unwrap_or(false)
}

/// Reads a Linux client link by layering progressively more available sources:
/// `iw` (richest), then `iwconfig`, then `/proc/net/wireless`. Each source only
/// fills the fields earlier sources could not provide, so a missing `iw` binary
/// no longer wipes out signal/quality data.
fn read_linux_link(interface: &str) -> WifiLink {
    let mut link = read_iw_link(interface).unwrap_or_default();

    if link.needs_linux_fallback() {
        if let Some(iwconfig) = read_iwconfig_link(interface) {
            link.fill_missing_from(iwconfig);
        }
    }

    if link.needs_linux_fallback() {
        if let Some(proc) = read_proc_net_wireless_link(interface) {
            link.fill_missing_from(proc);
        }
    }

    link
}

fn read_linux_link_opt(interface: &str) -> Option<WifiLink> {
    let link = read_linux_link(interface);
    if link == WifiLink::default() {
        None
    } else {
        Some(link)
    }
}

impl WifiLink {
    fn needs_linux_fallback(&self) -> bool {
        self.associated_bssid.is_none() || self.signal_dbm.is_none() || self.link_quality.is_none()
    }
}

fn read_iw_link(interface: &str) -> Option<WifiLink> {
    let output = Command::new("iw")
        .args(["dev", interface, "link"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(parse_iw_link(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_iw_link(output: &str) -> WifiLink {
    let mut link = WifiLink::default();

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

fn read_iwconfig_link(interface: &str) -> Option<WifiLink> {
    let output = Command::new("iwconfig").arg(interface).output().ok()?;

    if !output.status.success() {
        return None;
    }

    Some(parse_iwconfig_link(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// Parses legacy `iwconfig <iface>` output for signal level, link quality, BSSID,
/// bit rate, and frequency.
fn parse_iwconfig_link(output: &str) -> WifiLink {
    let mut link = WifiLink::default();

    for line in output.lines().map(str::trim) {
        if let Some(bssid) = extract_after_marker(line, "Access Point:") {
            let bssid = bssid.split_whitespace().next().unwrap_or_default();
            if is_bssid(bssid) {
                link.associated_bssid = Some(bssid.to_ascii_lowercase());
            }
        }

        if let Some(level) = extract_after_marker(line, "Signal level=") {
            link.signal_dbm = parse_first_i32(level);
        }

        if let Some(quality) = extract_after_marker(line, "Link Quality=") {
            link.link_quality = parse_fraction_percentage(quality);
        }

        if let Some(rate) = extract_after_marker(line, "Bit Rate=") {
            link.tx_bitrate_mbps = parse_first_f64(rate);
        }

        if let Some(freq) = extract_after_marker(line, "Frequency:") {
            link.channel = parse_first_f64(freq)
                .map(|ghz| (ghz * 1000.0).round() as u32)
                .and_then(frequency_to_channel);
        }
    }

    link
}

fn read_proc_net_wireless_link(interface: &str) -> Option<WifiLink> {
    let content = fs::read_to_string("/proc/net/wireless").ok()?;
    parse_proc_net_wireless_link(&content, interface)
}

/// Parses one interface row of `/proc/net/wireless`. The columns are
/// `status link level noise ...`; `link` maps to a 0-100 quality and `level`
/// is the signal in dBm.
fn parse_proc_net_wireless_link(content: &str, interface: &str) -> Option<WifiLink> {
    for line in content.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(&format!("{interface}:")) else {
            continue;
        };
        let fields = rest.split_whitespace().collect::<Vec<_>>();

        let link_quality = fields
            .get(1)
            .and_then(|raw| raw.trim_end_matches('.').parse::<f64>().ok())
            .map(|quality| ((quality / 70.0) * 100.0).round().clamp(0.0, 100.0) as u8);

        let signal_dbm = fields
            .get(2)
            .and_then(|raw| raw.trim_end_matches('.').parse::<f64>().ok())
            .map(|level| level.round() as i32);

        return Some(WifiLink {
            link_quality,
            signal_dbm,
            ..WifiLink::default()
        });
    }

    None
}

// ------------------------------- macOS -------------------------------

fn read_macos_wifi_interface_names() -> Vec<String> {
    let output = Command::new("networksetup")
        .arg("-listallhardwareports")
        .output()
        .ok();

    output
        .filter(|output| output.status.success())
        .map(|output| parse_macos_wifi_interface_names(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

/// Extracts the device names of Wi-Fi hardware ports from
/// `networksetup -listallhardwareports`.
fn parse_macos_wifi_interface_names(output: &str) -> Vec<String> {
    let mut interfaces = Vec::new();
    let mut current_is_wifi = false;

    for line in output.lines().map(str::trim) {
        if let Some(port) = line.strip_prefix("Hardware Port:") {
            let port = port.trim();
            current_is_wifi =
                port.eq_ignore_ascii_case("Wi-Fi") || port.eq_ignore_ascii_case("AirPort");
        } else if let Some(device) = line.strip_prefix("Device:") {
            if current_is_wifi {
                push_unique(&mut interfaces, device.trim().to_string());
            }
        }
    }

    interfaces
}

fn read_macos_link(interface: &str) -> Option<WifiLink> {
    let output = Command::new("ipconfig")
        .args(["getsummary", interface])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(parse_macos_link(&String::from_utf8_lossy(&output.stdout)))
}

/// Parses `ipconfig getsummary <iface>`. Modern macOS only exposes the BSSID and
/// link-active flag without elevated privileges; RSSI/bitrate/channel require a
/// multi-second `system_profiler` call or root, so they are intentionally left
/// `None` here.
fn parse_macos_link(output: &str) -> WifiLink {
    let mut bssid = None;
    let mut link_active = false;

    for line in output.lines().map(str::trim) {
        if let Some(value) = extract_ipconfig_value(line, "BSSID") {
            // Keep the first (top-level, active) BSSID; later lines may list
            // neighbor APs on some macOS versions.
            if bssid.is_none() && is_bssid(&value) {
                bssid = Some(value.to_ascii_lowercase());
            }
        } else if let Some(value) = extract_ipconfig_value(line, "LinkStatusActive") {
            link_active = value.eq_ignore_ascii_case("TRUE");
        }
    }

    WifiLink {
        associated_bssid: bssid.filter(|_| link_active),
        ..WifiLink::default()
    }
}

/// Reads `<Key> : <value>` pairs from `ipconfig getsummary` output.
fn extract_ipconfig_value(line: &str, key: &str) -> Option<String> {
    let (raw_key, value) = line.split_once(':')?;
    if raw_key.trim() == key {
        Some(value.trim().to_string())
    } else {
        None
    }
}

// ------------------------------ Windows ------------------------------

fn read_windows_interfaces_output() -> Option<String> {
    let output = Command::new("netsh")
        .args(["wlan", "show", "interfaces"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn read_windows_wifi_interface_names() -> Vec<String> {
    read_windows_interfaces_output()
        .map(|output| parse_netsh_interface_names(&output))
        .unwrap_or_default()
}

fn read_windows_link(interface: &str) -> Option<WifiLink> {
    let output = read_windows_interfaces_output()?;
    parse_netsh_link(&output, interface)
}

/// Extracts the `Name :` values from `netsh wlan show interfaces`.
fn parse_netsh_interface_names(output: &str) -> Vec<String> {
    let mut interfaces = Vec::new();

    for line in output.lines().map(str::trim) {
        if let Some(name) = extract_netsh_value(line, "Name") {
            push_unique(&mut interfaces, name);
        }
    }

    interfaces
}

/// Parses the block of `netsh wlan show interfaces` belonging to `interface`.
/// `netsh` reports signal as a percentage (mapped to link quality), plus BSSID,
/// channel, and receive/transmit rates in Mbps.
fn parse_netsh_link(output: &str, interface: &str) -> Option<WifiLink> {
    let mut in_block = false;
    let mut link = WifiLink::default();
    let mut found = false;

    for line in output.lines().map(str::trim) {
        if let Some(name) = extract_netsh_value(line, "Name") {
            if in_block {
                break;
            }
            in_block = name.eq_ignore_ascii_case(interface);
            found = in_block;
            continue;
        }

        if !in_block {
            continue;
        }

        if let Some(bssid) = extract_netsh_value(line, "BSSID") {
            if is_bssid(&bssid) {
                link.associated_bssid = Some(bssid.to_ascii_lowercase());
            }
        } else if let Some(signal) = extract_netsh_value(line, "Signal") {
            link.link_quality = parse_percentage(&signal);
        } else if let Some(channel) = extract_netsh_value(line, "Channel") {
            link.channel = parse_first_u32(&channel);
        } else if let Some(rate) = extract_netsh_value(line, "Receive rate (Mbps)") {
            link.rx_bitrate_mbps = parse_first_f64(&rate);
        } else if let Some(rate) = extract_netsh_value(line, "Transmit rate (Mbps)") {
            link.tx_bitrate_mbps = parse_first_f64(&rate);
        }
    }

    found.then_some(link)
}

/// Reads `<Key> : <value>` pairs from `netsh` output, tolerating the variable
/// whitespace netsh uses for column alignment.
fn extract_netsh_value(line: &str, key: &str) -> Option<String> {
    let (raw_key, value) = line.split_once(':')?;
    if raw_key.trim() == key {
        Some(value.trim().to_string())
    } else {
        None
    }
}

// ----------------------------- Shared parsing ------------------------

fn extract_after_marker<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let index = line.find(marker)?;
    Some(line[index + marker.len()..].trim_start())
}

fn is_bssid(value: &str) -> bool {
    let octets = value.split(':').collect::<Vec<_>>();
    octets.len() == 6
        && octets
            .iter()
            .all(|octet| octet.len() == 2 && octet.chars().all(|c| c.is_ascii_hexdigit()))
}

fn parse_first_i32(input: &str) -> Option<i32> {
    let token = input.split_whitespace().next()?;
    token
        .parse()
        .ok()
        .or_else(|| parse_first_f64(input).map(|value| value.round() as i32))
}

fn parse_first_u32(input: &str) -> Option<u32> {
    input.split_whitespace().next()?.parse().ok()
}

fn parse_first_f64(input: &str) -> Option<f64> {
    input.split_whitespace().next()?.parse().ok()
}

/// Parses a percentage token such as `95%` or `95` into a 0-100 value.
fn parse_percentage(input: &str) -> Option<u8> {
    let token = input.split_whitespace().next()?.trim_end_matches('%');
    token
        .parse::<f64>()
        .ok()
        .map(|value| value.round().clamp(0.0, 100.0) as u8)
}

/// Parses an `iwconfig` fraction such as `Link Quality=58/70` into a percentage.
fn parse_fraction_percentage(input: &str) -> Option<u8> {
    let token = input.split_whitespace().next()?;
    let (numerator, denominator) = token.split_once('/')?;
    let numerator = numerator.parse::<f64>().ok()?;
    let denominator = denominator.parse::<f64>().ok()?;
    if denominator == 0.0 {
        return None;
    }

    Some(
        ((numerator / denominator) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8,
    )
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
    use std::time::{SystemTime, UNIX_EPOCH};

    // ------------------------------- Linux -------------------------------

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
        assert_eq!(parse_iw_link("Not connected."), WifiLink::default());
    }

    #[test]
    fn parses_iwconfig_signal_quality_and_bssid() {
        let output = "\
wlan0     IEEE 802.11  ESSID:\"Home\"
          Mode:Managed  Frequency:2.412 GHz  Access Point: 70:3A:CB:12:34:56
          Bit Rate=72.2 Mb/s   Tx-Power=20 dBm
          Retry short limit:7   RTS thr:off   Fragment thr:off
          Power Management:on
          Link Quality=58/70  Signal level=-52 dBm
          Rx invalid nwid:0  Rx invalid crypt:0  Rx invalid frag:0
";

        let link = parse_iwconfig_link(output);

        assert_eq!(link.associated_bssid.as_deref(), Some("70:3a:cb:12:34:56"));
        assert_eq!(link.signal_dbm, Some(-52));
        assert_eq!(link.link_quality, Some(83));
        assert_eq!(link.tx_bitrate_mbps, Some(72.2));
        assert_eq!(link.channel, Some(1));
    }

    #[test]
    fn iwconfig_not_associated_has_no_bssid() {
        let output = "\
wlan0     IEEE 802.11  ESSID:off/any
          Mode:Managed  Access Point: Not-Associated   Tx-Power=20 dBm
";

        assert_eq!(parse_iwconfig_link(output).associated_bssid, None);
    }

    #[test]
    fn parses_proc_net_wireless_signal_and_quality() {
        let content = "\
Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE
 face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22
wlan0: 0000   49.  -61.  -256        0      0      0      0      0        0
";

        let link = parse_proc_net_wireless_link(content, "wlan0").unwrap();

        assert_eq!(link.link_quality, Some(70));
        assert_eq!(link.signal_dbm, Some(-61));
    }

    #[test]
    fn returns_none_for_unknown_proc_net_wireless_interface() {
        let content = "wlan0: 0000   49.  -61.  -256        0\n";
        assert_eq!(parse_proc_net_wireless_link(content, "wlp2s0"), None);
    }

    #[test]
    fn parses_wireless_interface_names_from_proc_net_wireless() {
        let content = "\
Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE
 face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22
wlan0: 0000   49.  -61.  -256        0      0      0      0      0        0
wlp2s0: 0000   62.  -48.  -256        0      0      0      0      0        0
";

        assert_eq!(
            parse_proc_net_wireless_interfaces(content),
            vec!["wlan0".to_string(), "wlp2s0".to_string()]
        );
    }

    #[test]
    fn reads_wireless_interface_names_from_sysfs() {
        let root = unique_temp_path();
        fs::create_dir_all(root.join("eth0")).unwrap();
        fs::create_dir_all(root.join("wlan0/wireless")).unwrap();
        fs::create_dir_all(root.join("wlp2s0/wireless")).unwrap();

        assert_eq!(
            read_sysfs_wireless_interfaces(&root),
            vec!["wlan0".to_string(), "wlp2s0".to_string()]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn layers_linux_sources_filling_only_missing_fields() {
        // iw only reported association + channel (no signal/quality).
        let mut link = WifiLink {
            associated_bssid: Some("70:3a:cb:12:34:56".to_string()),
            channel: Some(36),
            ..WifiLink::default()
        };
        assert!(link.needs_linux_fallback());

        // /proc/net/wireless fills the gaps without clobbering the channel.
        link.fill_missing_from(WifiLink {
            signal_dbm: Some(-61),
            link_quality: Some(70),
            ..WifiLink::default()
        });

        assert_eq!(link.associated_bssid.as_deref(), Some("70:3a:cb:12:34:56"));
        assert_eq!(link.channel, Some(36));
        assert_eq!(link.signal_dbm, Some(-61));
        assert_eq!(link.link_quality, Some(70));
        assert!(!link.needs_linux_fallback());
    }

    #[test]
    fn treats_carrier_up_sysfs_interface_as_active() {
        let root = unique_temp_path();
        let interface = root.join("wlan0");
        fs::create_dir_all(&interface).unwrap();
        fs::write(interface.join("carrier"), "1\n").unwrap();

        assert!(sysfs_interface_appears_active(&interface));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn treats_dormant_sysfs_interface_as_active() {
        let root = unique_temp_path();
        let interface = root.join("wlan0");
        fs::create_dir_all(&interface).unwrap();
        fs::write(interface.join("operstate"), "dormant\n").unwrap();

        assert!(sysfs_interface_appears_active(&interface));

        fs::remove_dir_all(root).unwrap();
    }

    // ------------------------------- macOS -------------------------------

    #[test]
    fn parses_macos_wifi_device_from_hardware_ports() {
        let output = "\
Hardware Port: Ethernet
Device: en3
Ethernet Address: 80:a9:97:18:61:00

Hardware Port: Wi-Fi
Device: en0
Ethernet Address: 80:a9:97:18:61:a2

Hardware Port: Thunderbolt Bridge
Device: bridge0
Ethernet Address: 36:a9:97:18:61:a2
";

        assert_eq!(
            parse_macos_wifi_interface_names(output),
            vec!["en0".to_string()]
        );
    }

    #[test]
    fn parses_legacy_airport_hardware_port_name() {
        let output = "Hardware Port: AirPort\nDevice: en1\nEthernet Address: 80:a9:97:18:61:a2\n";
        assert_eq!(
            parse_macos_wifi_interface_names(output),
            vec!["en1".to_string()]
        );
    }

    #[test]
    fn parses_macos_link_bssid_when_link_is_active() {
        let output = "\
en0 : flags = 0x8863
  BSSID : 70:3A:CB:12:34:56
  InterfaceType : WiFi
  LinkStatusActive : TRUE
  SSID : Home
";

        let link = parse_macos_link(output);

        assert_eq!(link.associated_bssid.as_deref(), Some("70:3a:cb:12:34:56"));
        // RSSI/bitrate/channel are unavailable without privileged tooling.
        assert_eq!(link.signal_dbm, None);
        assert_eq!(link.tx_bitrate_mbps, None);
        assert_eq!(link.channel, None);
    }

    #[test]
    fn ignores_macos_bssid_when_link_is_inactive() {
        let output = "\
en0 : flags = 0x8863
  BSSID : 70:3a:cb:12:34:56
  LinkStatusActive : FALSE
";

        assert_eq!(parse_macos_link(output).associated_bssid, None);
    }

    #[test]
    fn ignores_macos_redacted_bssid_without_location_permission() {
        // Modern macOS returns the literal `<redacted>` for the BSSID unless the
        // process holds Location Services authorization. It must not be mistaken
        // for a real address; association then relies on bound pings instead.
        let output = "\
en0 : flags = 0x8863
  BSSID : <redacted>
  LinkStatusActive : TRUE
";

        assert_eq!(parse_macos_link(output).associated_bssid, None);
    }

    #[test]
    fn keeps_first_macos_bssid_when_neighbors_are_listed() {
        let output = "\
en0 : flags = 0x8863
  BSSID : 70:3a:cb:12:34:56
  LinkStatusActive : TRUE
  BSSID : aa:bb:cc:dd:ee:ff
";

        assert_eq!(
            parse_macos_link(output).associated_bssid.as_deref(),
            Some("70:3a:cb:12:34:56")
        );
    }

    // ------------------------------ Windows ------------------------------

    fn netsh_two_interfaces() -> &'static str {
        "\
There are 2 interfaces on the system:

    Name                   : Wi-Fi
    Description            : Intel(R) Wi-Fi 6 AX201 160MHz
    GUID                   : 00000000-0000-0000-0000-000000000000
    Physical address       : 80:a9:97:18:61:a2
    State                  : connected
    SSID                   : Home
    BSSID                  : 70:3A:CB:12:34:56
    Radio type             : 802.11ax
    Channel                : 44
    Receive rate (Mbps)    : 866.7
    Transmit rate (Mbps)   : 780.5
    Signal                 : 92%
    Profile                : Home

    Name                   : Wi-Fi 2
    Description            : Secondary Adapter
    State                  : disconnected
    Signal                 : 0%
"
    }

    #[test]
    fn parses_netsh_interface_names() {
        assert_eq!(
            parse_netsh_interface_names(netsh_two_interfaces()),
            vec!["Wi-Fi".to_string(), "Wi-Fi 2".to_string()]
        );
    }

    #[test]
    fn parses_netsh_link_for_named_interface() {
        let link = parse_netsh_link(netsh_two_interfaces(), "Wi-Fi").unwrap();

        assert_eq!(link.associated_bssid.as_deref(), Some("70:3a:cb:12:34:56"));
        assert_eq!(link.link_quality, Some(92));
        assert_eq!(link.channel, Some(44));
        assert_eq!(link.rx_bitrate_mbps, Some(866.7));
        assert_eq!(link.tx_bitrate_mbps, Some(780.5));
    }

    #[test]
    fn netsh_link_block_is_isolated_per_interface() {
        let link = parse_netsh_link(netsh_two_interfaces(), "Wi-Fi 2").unwrap();

        // The second adapter is disconnected: no BSSID leaks from the first block.
        assert_eq!(link.associated_bssid, None);
        assert_eq!(link.link_quality, Some(0));
    }

    #[test]
    fn netsh_link_returns_none_for_unknown_interface() {
        assert_eq!(parse_netsh_link(netsh_two_interfaces(), "Ethernet"), None);
    }

    // --------------------------- Cross-platform --------------------------

    #[test]
    fn selects_first_connected_wifi_interface() {
        let candidates = vec!["wlan0".to_string(), "wlp2s0".to_string()];

        assert_eq!(
            select_connected_wifi_interface(
                candidates,
                |interface| {
                    if interface == "wlp2s0" {
                        Some(WifiLink {
                            associated_bssid: Some("aa:bb:cc:dd:ee:ff".to_string()),
                            ..WifiLink::default()
                        })
                    } else {
                        Some(WifiLink::default())
                    }
                },
                |_| false
            ),
            Some("wlp2s0".to_string())
        );
    }

    #[test]
    fn falls_back_to_active_wifi_interface_when_link_metadata_is_unavailable() {
        let candidates = vec!["wlan0".to_string()];

        assert_eq!(
            select_connected_wifi_interface(candidates, |_| None, |interface| interface == "wlan0"),
            Some("wlan0".to_string())
        );
    }

    #[test]
    fn bound_ping_success_can_prove_wifi_association_when_link_metadata_is_missing() {
        let sample = WifiSample {
            enabled: true,
            router_ping: PingResult::ok(1.0),
            associated_bssid: None,
            bound_pings_supported: true,
            ..WifiSample::default()
        };

        assert!(sample.associated());
    }

    #[test]
    fn bound_ping_success_does_not_prove_association_without_interface_binding() {
        // On platforms that cannot bind ICMP (Windows), the pings are skipped and
        // association must come from real link metadata instead.
        let sample = WifiSample {
            enabled: true,
            router_ping: PingResult::ok(1.0),
            associated_bssid: None,
            bound_pings_supported: false,
            ..WifiSample::default()
        };

        assert!(!sample.associated());
    }

    #[test]
    fn link_metadata_proves_association_on_every_platform() {
        let sample = WifiSample {
            enabled: true,
            associated_bssid: Some("70:3a:cb:12:34:56".to_string()),
            bound_pings_supported: false,
            ..WifiSample::default()
        };

        assert!(sample.associated());
    }

    #[test]
    fn wifi_config_defaults_to_auto_detection() {
        let config = WifiConfig::auto();

        assert_eq!(config.interface, WifiInterfaceSelection::Auto);
        assert_eq!(config.explicit_interface(), None);
        assert_eq!(config.description(), "auto");
    }

    #[test]
    fn recognizes_bssid_strings() {
        assert!(is_bssid("70:3a:cb:12:34:56"));
        assert!(is_bssid("AA:BB:CC:DD:EE:FF"));
        assert!(!is_bssid("Not-Associated"));
        assert!(!is_bssid("70:3a:cb:12:34"));
        assert!(!is_bssid("zz:3a:cb:12:34:56"));
    }

    #[test]
    fn parses_signal_percentages_and_fractions() {
        assert_eq!(parse_percentage("92%"), Some(92));
        assert_eq!(parse_percentage("100"), Some(100));
        assert_eq!(parse_fraction_percentage("58/70"), Some(83));
        assert_eq!(parse_fraction_percentage("70/70"), Some(100));
        assert_eq!(parse_fraction_percentage("0/0"), None);
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
        let connected = WifiLink {
            associated_bssid: Some("aa:bb:cc:dd:ee:ff".to_string()),
            channel: Some(1),
            ..WifiLink::default()
        };
        let disconnected = WifiLink::default();

        assert!(monitor.detect_events(&connected, true).is_empty());
        monitor.previous_associated = Some(true);
        monitor.previous_bssid = connected.associated_bssid.clone();
        monitor.previous_channel = connected.channel;
        assert_eq!(
            monitor.detect_events(&disconnected, false),
            vec![WifiEvent::Disconnected]
        );
        monitor.previous_associated = Some(false);
        assert_eq!(
            monitor.detect_events(&connected, true),
            vec![WifiEvent::Reconnected]
        );
    }

    #[test]
    fn detects_bssid_roaming_event() {
        let mut monitor = WifiMonitor::new(WifiConfig::new("wlan0".to_string()));
        monitor.previous_associated = Some(true);
        monitor.previous_bssid = Some("aa:bb:cc:dd:ee:ff".to_string());

        let roamed = WifiLink {
            associated_bssid: Some("11:22:33:44:55:66".to_string()),
            ..WifiLink::default()
        };

        assert_eq!(
            monitor.detect_events(&roamed, true),
            vec![WifiEvent::BssidChanged {
                previous: "aa:bb:cc:dd:ee:ff".to_string(),
                current: "11:22:33:44:55:66".to_string(),
            }]
        );
    }

    fn unique_temp_path() -> std::path::PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "router-monitor-wifi-test-{}-{timestamp}",
            std::process::id()
        ))
    }
}
