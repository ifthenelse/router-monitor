use crate::correlation_engine::SuspectedFailureLayer;
use crate::dns_monitor::{DnsResult, DnsStatus};
use crate::environment_monitor::{
    format_humidity_field, format_temperature_field, EnvironmentReading,
};
use crate::http_monitor::{HttpResult, HttpStatus};
use crate::https_monitor::{HttpsResult, HttpsStatus};
use crate::ping::{PingResult, PingStatus};
use crate::tcp_monitor::TcpSample;
use crate::websocket_monitor::WebSocketSample;
use crate::wifi_monitor::WifiSample;
use anyhow::{Context, Result};
use csv::Writer;
use std::fs::{File, OpenOptions};
use std::io::BufWriter;
use std::path::Path;

const HEADER: [&str; 38] = [
    "timestamp",
    "script_version",
    "router_ms",
    "internet_ms",
    "router_http_ms",
    "dns_lookup_ms",
    "https_google_ms",
    "https_cloudflare_ms",
    "outside_temperature_c",
    "outside_relative_humidity",
    "outside_apparent_temperature_c",
    "wifi_signal_dbm",
    "wifi_link_quality",
    "wifi_tx_bitrate",
    "wifi_rx_bitrate",
    "wifi_router_ms",
    "wifi_internet_ms",
    "wifi_disconnect_count",
    "wifi_reconnect_count",
    "wifi_associated_bssid",
    "wifi_channel",
    "tcp_connected",
    "tcp_disconnect_count",
    "tcp_reconnect_count",
    "tcp_session_uptime_seconds",
    "last_tcp_disconnect",
    "websocket_connected",
    "websocket_disconnect_count",
    "websocket_reconnect_count",
    "websocket_uptime_seconds",
    "last_websocket_disconnect",
    "suspected_failure_layer",
    "router_status",
    "internet_status",
    "router_http_status",
    "dns_status",
    "https_google_status",
    "https_cloudflare_status",
];

pub struct CsvSample {
    pub timestamp: String,
    pub router: PingResult,
    pub internet: PingResult,
    pub router_http: HttpResult,
    pub dns_lookup: DnsResult,
    pub https_google: HttpsResult,
    pub https_cloudflare: HttpsResult,
    pub environment: EnvironmentReading,
    pub wifi: WifiSample,
    pub tcp: TcpSample,
    pub websocket: WebSocketSample,
    pub suspected_failure_layer: SuspectedFailureLayer,
}

pub struct CsvLog {
    writer: Writer<BufWriter<File>>,
}

impl CsvLog {
    pub fn create(path: &Path) -> Result<Self> {
        let file = File::create(path)
            .with_context(|| format!("Cannot write to output file '{}'.", path.display()))?;
        let mut writer = Writer::from_writer(BufWriter::new(file));

        writer
            .write_record(HEADER)
            .with_context(|| format!("Cannot write CSV header to '{}'.", path.display()))?;
        writer
            .flush()
            .with_context(|| format!("Cannot flush CSV header to '{}'.", path.display()))?;

        Ok(Self { writer })
    }

    pub fn write_sample(&mut self, sample: &CsvSample) -> Result<()> {
        let router_ms = latency_field(sample.router.latency_ms);
        let internet_ms = latency_field(sample.internet.latency_ms);
        let router_http_ms = latency_field(sample.router_http.latency_ms);
        let dns_lookup_ms = latency_field(sample.dns_lookup.latency_ms);
        let https_google_ms = latency_field(sample.https_google.latency_ms);
        let https_cloudflare_ms = latency_field(sample.https_cloudflare.latency_ms);
        let outside_temperature_c =
            format_temperature_field(sample.environment.outside_temperature_c);
        let outside_relative_humidity =
            format_humidity_field(sample.environment.outside_relative_humidity);
        let outside_apparent_temperature_c =
            format_temperature_field(sample.environment.outside_apparent_temperature_c);
        let wifi_signal_dbm = optional_display(sample.wifi.signal_dbm);
        let wifi_link_quality = optional_display(sample.wifi.link_quality);
        let wifi_tx_bitrate = optional_float_field(sample.wifi.tx_bitrate_mbps);
        let wifi_rx_bitrate = optional_float_field(sample.wifi.rx_bitrate_mbps);
        let wifi_router_ms = latency_field(sample.wifi.router_ping.latency_ms);
        let wifi_internet_ms = latency_field(sample.wifi.internet_ping.latency_ms);
        let wifi_disconnect_count =
            enabled_count_field(sample.wifi.enabled, sample.wifi.disconnect_count);
        let wifi_reconnect_count =
            enabled_count_field(sample.wifi.enabled, sample.wifi.reconnect_count);
        let wifi_channel = optional_display(sample.wifi.channel);
        let tcp_connected = bool_field(sample.tcp.connected);
        let tcp_disconnect_count = sample.tcp.disconnect_count.to_string();
        let tcp_reconnect_count = sample.tcp.reconnect_count.to_string();
        let tcp_session_uptime_seconds = optional_display(sample.tcp.session_uptime_seconds);
        let websocket_connected =
            enabled_bool_field(sample.websocket.enabled, sample.websocket.connected);
        let websocket_disconnect_count =
            enabled_count_field(sample.websocket.enabled, sample.websocket.disconnect_count);
        let websocket_reconnect_count =
            enabled_count_field(sample.websocket.enabled, sample.websocket.reconnect_count);
        let websocket_uptime_seconds = if sample.websocket.enabled {
            optional_display(sample.websocket.uptime_seconds)
        } else {
            String::new()
        };
        let router_status = status_field(sample.router.status);
        let internet_status = status_field(sample.internet.status);
        let router_http_status = http_status_field(sample.router_http.status);
        let dns_status = dns_status_field(sample.dns_lookup.status);
        let https_google_status = https_status_field(sample.https_google.status);
        let https_cloudflare_status = https_status_field(sample.https_cloudflare.status);

        self.writer.write_record([
            sample.timestamp.as_str(),
            env!("CARGO_PKG_VERSION"),
            router_ms.as_str(),
            internet_ms.as_str(),
            router_http_ms.as_str(),
            dns_lookup_ms.as_str(),
            https_google_ms.as_str(),
            https_cloudflare_ms.as_str(),
            outside_temperature_c.as_str(),
            outside_relative_humidity.as_str(),
            outside_apparent_temperature_c.as_str(),
            wifi_signal_dbm.as_str(),
            wifi_link_quality.as_str(),
            wifi_tx_bitrate.as_str(),
            wifi_rx_bitrate.as_str(),
            wifi_router_ms.as_str(),
            wifi_internet_ms.as_str(),
            wifi_disconnect_count.as_str(),
            wifi_reconnect_count.as_str(),
            sample.wifi.associated_bssid.as_deref().unwrap_or_default(),
            wifi_channel.as_str(),
            tcp_connected.as_str(),
            tcp_disconnect_count.as_str(),
            tcp_reconnect_count.as_str(),
            tcp_session_uptime_seconds.as_str(),
            sample.tcp.last_disconnect.as_deref().unwrap_or_default(),
            websocket_connected.as_str(),
            websocket_disconnect_count.as_str(),
            websocket_reconnect_count.as_str(),
            websocket_uptime_seconds.as_str(),
            sample
                .websocket
                .last_disconnect
                .as_deref()
                .unwrap_or_default(),
            sample.suspected_failure_layer.as_csv_value(),
            router_status,
            internet_status,
            router_http_status,
            dns_status,
            https_google_status,
            https_cloudflare_status,
        ])?;
        self.writer.flush()?;

        Ok(())
    }
}

pub fn verify_writable(path: &Path) -> Result<()> {
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)
        .with_context(|| format!("Cannot write to output file '{}'.", path.display()))?;

    Ok(())
}

fn latency_field(value: Option<f64>) -> String {
    value
        .map(|latency| format!("{latency:.2}"))
        .unwrap_or_default()
}

fn optional_float_field(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.1}")).unwrap_or_default()
}

fn optional_display<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn bool_field(value: bool) -> String {
    value.to_string()
}

fn enabled_bool_field(enabled: bool, value: bool) -> String {
    if enabled {
        bool_field(value)
    } else {
        String::new()
    }
}

fn enabled_count_field(enabled: bool, value: u64) -> String {
    if enabled {
        value.to_string()
    } else {
        String::new()
    }
}

fn status_field(status: PingStatus) -> &'static str {
    status.as_csv_value()
}

fn http_status_field(status: HttpStatus) -> &'static str {
    status.as_csv_value()
}

fn dns_status_field(status: DnsStatus) -> &'static str {
    status.as_csv_value()
}

fn https_status_field(status: HttpsStatus) -> &'static str {
    status.as_csv_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns_monitor::DnsResult;
    use crate::http_monitor::HttpResult;
    use crate::https_monitor::HttpsResult;
    use crate::ping::PingResult;
    use crate::tcp_monitor::TcpSample;
    use crate::websocket_monitor::WebSocketSample;
    use crate::wifi_monitor::WifiSample;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn formats_failed_latency_as_empty_string() {
        assert_eq!(latency_field(None), "");
    }

    #[test]
    fn formats_successful_latency_as_numeric_text() {
        assert_eq!(latency_field(Some(8.214)), "8.21");
    }

    #[test]
    fn writes_all_latency_and_status_columns() {
        let path = unique_temp_path();
        let sample = CsvSample {
            timestamp: "2026-06-01 18:00:03".to_string(),
            router: PingResult::ok(0.704),
            internet: PingResult::ok(8.2),
            router_http: HttpResult::ok(2.91),
            dns_lookup: DnsResult::ok(7.93),
            https_google: HttpsResult::timeout(),
            https_cloudflare: HttpsResult::ok(18.11),
            environment: EnvironmentReading {
                outside_temperature_c: Some(31.4),
                outside_relative_humidity: Some(68.0),
                outside_apparent_temperature_c: Some(37.8),
                raspberry_pi_temperature_c: Some(54.2),
            },
            wifi: WifiSample {
                enabled: true,
                signal_dbm: Some(-61),
                link_quality: Some(70),
                tx_bitrate_mbps: Some(72.2),
                rx_bitrate_mbps: Some(86.7),
                router_ping: PingResult::ok(1.8),
                internet_ping: PingResult::ok(9.4),
                disconnect_count: 1,
                reconnect_count: 1,
                associated_bssid: Some("70:3a:cb:12:34:56".to_string()),
                channel: Some(1),
                events: Vec::new(),
            },
            tcp: TcpSample {
                connected: true,
                disconnect_count: 2,
                reconnect_count: 2,
                session_uptime_seconds: Some(42),
                last_disconnect: Some("2026-06-01 17:59:50".to_string()),
                events: Vec::new(),
            },
            websocket: WebSocketSample {
                enabled: true,
                connected: false,
                disconnect_count: 3,
                reconnect_count: 2,
                uptime_seconds: None,
                last_disconnect: Some("2026-06-01 18:00:01".to_string()),
                events: Vec::new(),
            },
            suspected_failure_layer: SuspectedFailureLayer::Session,
        };

        {
            let mut log = CsvLog::create(&path).unwrap();
            log.write_sample(&sample).unwrap();
        }

        let content = fs::read_to_string(&path).unwrap();

        assert_eq!(
            content,
            format!(
                "timestamp,script_version,router_ms,internet_ms,router_http_ms,dns_lookup_ms,https_google_ms,https_cloudflare_ms,outside_temperature_c,outside_relative_humidity,outside_apparent_temperature_c,wifi_signal_dbm,wifi_link_quality,wifi_tx_bitrate,wifi_rx_bitrate,wifi_router_ms,wifi_internet_ms,wifi_disconnect_count,wifi_reconnect_count,wifi_associated_bssid,wifi_channel,tcp_connected,tcp_disconnect_count,tcp_reconnect_count,tcp_session_uptime_seconds,last_tcp_disconnect,websocket_connected,websocket_disconnect_count,websocket_reconnect_count,websocket_uptime_seconds,last_websocket_disconnect,suspected_failure_layer,router_status,internet_status,router_http_status,dns_status,https_google_status,https_cloudflare_status\n2026-06-01 18:00:03,{},0.70,8.20,2.91,7.93,,18.11,31.4,68,37.8,-61,70,72.2,86.7,1.80,9.40,1,1,70:3a:cb:12:34:56,1,true,2,2,42,2026-06-01 17:59:50,false,3,2,,2026-06-01 18:00:01,session,ok,ok,ok,ok,timeout,ok\n",
                env!("CARGO_PKG_VERSION")
            )
        );

        fs::remove_file(path).unwrap();
    }

    fn unique_temp_path() -> std::path::PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "router-monitor-csv-test-{}-{timestamp}.csv",
            std::process::id()
        ))
    }
}
