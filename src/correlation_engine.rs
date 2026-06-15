use crate::csv_writer::CsvSample;
use crate::dns_monitor::DnsStatus;
use crate::http_monitor::HttpStatus;
use crate::https_monitor::HttpsStatus;
use crate::ping::PingStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspectedFailureLayer {
    None,
    Wifi,
    Session,
    Internet,
    Router,
    Unknown,
}

impl SuspectedFailureLayer {
    pub fn as_csv_value(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Wifi => "wifi",
            Self::Session => "session",
            Self::Internet => "internet",
            Self::Router => "router",
            Self::Unknown => "unknown",
        }
    }

    pub fn event_text(self) -> Option<&'static str> {
        match self {
            Self::Wifi => Some("POSSIBLE_WIFI_LAYER_FAILURE"),
            Self::Session => Some("POSSIBLE_SESSION_LAYER_FAILURE"),
            Self::Internet => Some("POSSIBLE_INTERNET_LAYER_FAILURE"),
            Self::Router => Some("POSSIBLE_ROUTER_LAYER_FAILURE"),
            Self::Unknown => Some("POSSIBLE_UNKNOWN_LAYER_FAILURE"),
            Self::None => None,
        }
    }
}

pub fn classify_sample(sample: &CsvSample) -> SuspectedFailureLayer {
    let baseline_router_healthy =
        sample.router.status == PingStatus::Ok && sample.router_http.status == HttpStatus::Ok;
    let baseline_internet_healthy = sample.internet.status == PingStatus::Ok
        && sample.dns_lookup.status == DnsStatus::Ok
        && sample.https_google.status == HttpsStatus::Ok
        && sample.https_cloudflare.status == HttpsStatus::Ok;
    let baseline_healthy = baseline_router_healthy && baseline_internet_healthy;
    let wifi_failure = sample.wifi.enabled
        && (!sample.wifi.associated()
            || sample.wifi.router_ping.status == PingStatus::Timeout
            || sample.wifi.internet_ping.status == PingStatus::Timeout);
    let session_failure =
        !sample.tcp.connected || (sample.websocket.enabled && !sample.websocket.connected);

    if wifi_failure && baseline_healthy {
        SuspectedFailureLayer::Wifi
    } else if session_failure && baseline_healthy {
        SuspectedFailureLayer::Session
    } else if (wifi_failure || session_failure) && !baseline_healthy {
        SuspectedFailureLayer::Unknown
    } else if sample.router.status == PingStatus::Timeout
        && sample.router_http.status == HttpStatus::Timeout
    {
        SuspectedFailureLayer::Router
    } else if !baseline_internet_healthy && baseline_router_healthy {
        SuspectedFailureLayer::Internet
    } else {
        SuspectedFailureLayer::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csv_writer::CsvSample;
    use crate::dns_monitor::DnsResult;
    use crate::environment_monitor::EnvironmentReading;
    use crate::http_monitor::HttpResult;
    use crate::https_monitor::HttpsResult;
    use crate::ping::PingResult;
    use crate::tcp_monitor::TcpSample;
    use crate::websocket_monitor::WebSocketSample;
    use crate::wifi_monitor::WifiSample;

    #[test]
    fn classifies_wifi_failure_when_baseline_is_healthy() {
        let mut sample = healthy_sample();
        sample.wifi.enabled = true;
        sample.wifi.router_ping = PingResult::timeout();

        assert_eq!(classify_sample(&sample), SuspectedFailureLayer::Wifi);
    }

    #[test]
    fn does_not_classify_wifi_failure_when_bound_wifi_pings_are_healthy() {
        let mut sample = healthy_sample();
        sample.wifi.enabled = true;
        sample.wifi.interface = Some("wlan0".to_string());
        sample.wifi.router_ping = PingResult::ok(1.0);
        sample.wifi.internet_ping = PingResult::ok(5.0);

        assert_eq!(classify_sample(&sample), SuspectedFailureLayer::None);
    }

    #[test]
    fn classifies_session_failure_when_baseline_is_healthy() {
        let mut sample = healthy_sample();
        sample.tcp.connected = false;

        assert_eq!(classify_sample(&sample), SuspectedFailureLayer::Session);
    }

    #[test]
    fn classifies_router_failure() {
        let mut sample = healthy_sample();
        sample.router = PingResult::timeout();
        sample.router_http = HttpResult::timeout();

        assert_eq!(classify_sample(&sample), SuspectedFailureLayer::Router);
    }

    #[test]
    fn classifies_internet_failure_when_router_is_healthy() {
        let mut sample = healthy_sample();
        sample.internet = PingResult::timeout();

        assert_eq!(classify_sample(&sample), SuspectedFailureLayer::Internet);
    }

    #[test]
    fn classifies_unknown_when_wifi_failure_overlaps_baseline_failure() {
        let mut sample = healthy_sample();
        sample.internet = PingResult::timeout();
        sample.wifi.enabled = true;
        sample.wifi.router_ping = PingResult::timeout();

        assert_eq!(classify_sample(&sample), SuspectedFailureLayer::Unknown);
    }

    #[test]
    fn classifies_none_for_healthy_sample() {
        assert_eq!(
            classify_sample(&healthy_sample()),
            SuspectedFailureLayer::None
        );
    }

    #[test]
    fn maps_failure_layers_to_csv_and_event_values() {
        assert_eq!(SuspectedFailureLayer::Wifi.as_csv_value(), "wifi");
        assert_eq!(
            SuspectedFailureLayer::Session.event_text(),
            Some("POSSIBLE_SESSION_LAYER_FAILURE")
        );
        assert_eq!(SuspectedFailureLayer::None.event_text(), None);
    }

    fn healthy_sample() -> CsvSample {
        CsvSample {
            timestamp: "2026-06-14 01:00:00".to_string(),
            router: PingResult::ok(1.0),
            internet: PingResult::ok(5.0),
            router_http: HttpResult::ok(3.0),
            dns_lookup: DnsResult::ok(4.0),
            https_google: HttpsResult::ok(30.0),
            https_cloudflare: HttpsResult::ok(20.0),
            environment: EnvironmentReading::default(),
            wifi: WifiSample::disabled(),
            tcp: TcpSample {
                connected: true,
                ..TcpSample::default()
            },
            websocket: WebSocketSample::default(),
            suspected_failure_layer: SuspectedFailureLayer::None,
        }
    }
}
