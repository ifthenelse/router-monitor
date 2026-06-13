use crate::csv_writer::{CsvLog, CsvSample};
use crate::dns_monitor::{self, DnsMonitor, DnsResult};
use crate::duration::DurationSpec;
use crate::environment_monitor::{
    self, format_humidity_field, format_temperature_field, EnvironmentMonitor, EnvironmentReading,
};
use crate::event_log::{event_log_path, EventLog};
use crate::http_monitor::{self, HttpMonitor, HttpResult};
use crate::https_monitor::{self, HttpsMonitor, HttpsResult};
use crate::ping::{self, PingResult};
use anyhow::Result;
use chrono::{Duration as ChronoDuration, Local};
use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub duration: DurationSpec,
    pub duration_parts: Vec<String>,
    pub router_ip: Ipv4Addr,
    pub internet_ip: Ipv4Addr,
    pub environment: crate::environment_monitor::EnvironmentConfig,
    pub output_path: PathBuf,
    pub run_in_background: bool,
    pub show_progress: bool,
    pub verbose: bool,
    pub beep: bool,
}

pub fn run(config: MonitorConfig) -> Result<()> {
    let mut csv = CsvLog::create(&config.output_path)?;
    let event_log_path = event_log_path(&config.output_path);
    let mut event_log = EventLog::create(&event_log_path)?;
    let http_monitor = HttpMonitor::new()?;
    let dns_monitor = DnsMonitor::new();
    let https_monitor = HttpsMonitor::new()?;
    let mut environment_monitor = EnvironmentMonitor::new(config.environment);
    let mut previous_statuses = None;
    let total_samples = config.duration.sample_seconds();
    let finish_time = finish_time(config.duration.total());

    if config.show_progress {
        print_startup(&config, total_samples, &finish_time, &environment_monitor);
    }

    let started_at = Instant::now();

    for sample_index in 0..total_samples {
        sleep_until_next_second(started_at, sample_index);

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let environment_sample = environment_monitor.sample(Instant::now());
        let (router, internet, router_http, dns_lookup, https_google, https_cloudflare) =
            measure_targets(
                config.router_ip,
                config.internet_ip,
                &http_monitor,
                dns_monitor,
                &https_monitor,
            );
        let sample = CsvSample {
            timestamp: timestamp.clone(),
            router,
            internet,
            router_http,
            dns_lookup,
            https_google,
            https_cloudflare,
            environment: environment_sample.reading,
        };

        csv.write_sample(&sample)?;

        if environment_sample.refreshed
            && environment_monitor::has_high_heat_conditions(environment_sample.reading)
        {
            event_log.write_high_heat_conditions(&timestamp, environment_sample.reading)?;
        }

        record_connectivity_events(&mut event_log, &timestamp, &sample, &mut previous_statuses)?;

        if config.show_progress {
            print_progress(
                sample_index + 1,
                total_samples,
                &finish_time,
                &sample,
                config.verbose,
            )?;
        }
    }

    sleep_until_duration_expires(started_at, config.duration.total());

    if config.show_progress {
        clear_progress_line()?;
        println!("Monitoring completed");
        println!("Elapsed: {:.1}s", started_at.elapsed().as_secs_f64());
    }

    if config.beep {
        print!("\x07");
        io::stdout().flush()?;
    }

    Ok(())
}

pub fn finish_time(duration: Duration) -> String {
    let chrono_duration =
        ChronoDuration::from_std(duration).unwrap_or_else(|_| ChronoDuration::MAX);

    (Local::now() + chrono_duration)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn measure_targets(
    router_ip: Ipv4Addr,
    internet_ip: Ipv4Addr,
    http_monitor: &HttpMonitor,
    dns_monitor: DnsMonitor,
    https_monitor: &HttpsMonitor,
) -> (
    PingResult,
    PingResult,
    HttpResult,
    DnsResult,
    HttpsResult,
    HttpsResult,
) {
    let router = router_ip.to_string();
    let internet = internet_ip.to_string();
    let http_monitor = http_monitor.clone();
    let https_monitor = https_monitor.clone();

    thread::scope(|scope| {
        let router_handle = scope.spawn(|| ping::ping_once(&router));
        let internet_handle = scope.spawn(|| ping::ping_once(&internet));
        let router_http_handle = scope.spawn(|| http_monitor.measure_router(router_ip));
        let dns_lookup_handle = scope.spawn(move || dns_monitor.measure_google());
        let https_google_handle = scope.spawn(|| https_monitor.measure_google());
        let https_cloudflare_handle = scope.spawn(|| https_monitor.measure_cloudflare());

        let router_result = router_handle
            .join()
            .unwrap_or_else(|_| PingResult::timeout());
        let internet_result = internet_handle
            .join()
            .unwrap_or_else(|_| PingResult::timeout());
        let router_http_result = router_http_handle
            .join()
            .unwrap_or_else(|_| HttpResult::timeout());
        let dns_lookup_result = dns_lookup_handle
            .join()
            .unwrap_or_else(|_| DnsResult::timeout());
        let https_google_result = https_google_handle
            .join()
            .unwrap_or_else(|_| HttpsResult::timeout());
        let https_cloudflare_result = https_cloudflare_handle
            .join()
            .unwrap_or_else(|_| HttpsResult::timeout());

        (
            router_result,
            internet_result,
            router_http_result,
            dns_lookup_result,
            https_google_result,
            https_cloudflare_result,
        )
    })
}

#[derive(Debug, Clone, Copy)]
struct SampleStatuses {
    router_timeout: bool,
    internet_timeout: bool,
    router_http_timeout: bool,
    dns_timeout: bool,
    https_google_timeout: bool,
    https_cloudflare_timeout: bool,
}

impl SampleStatuses {
    fn from_sample(sample: &CsvSample) -> Self {
        Self {
            router_timeout: sample.router.latency_ms.is_none(),
            internet_timeout: sample.internet.latency_ms.is_none(),
            router_http_timeout: sample.router_http.latency_ms.is_none(),
            dns_timeout: sample.dns_lookup.latency_ms.is_none(),
            https_google_timeout: sample.https_google.latency_ms.is_none(),
            https_cloudflare_timeout: sample.https_cloudflare.latency_ms.is_none(),
        }
    }
}

fn record_connectivity_events(
    event_log: &mut EventLog,
    timestamp: &str,
    sample: &CsvSample,
    previous_statuses: &mut Option<SampleStatuses>,
) -> Result<()> {
    let current = SampleStatuses::from_sample(sample);
    let previous = previous_statuses.unwrap_or(SampleStatuses {
        router_timeout: false,
        internet_timeout: false,
        router_http_timeout: false,
        dns_timeout: false,
        https_google_timeout: false,
        https_cloudflare_timeout: false,
    });

    write_timeout_transition(
        event_log,
        timestamp,
        "ROUTER_PING_TIMEOUT detected",
        sample.environment,
        previous.router_timeout,
        current.router_timeout,
    )?;
    write_timeout_transition(
        event_log,
        timestamp,
        "INTERNET_PING_TIMEOUT detected",
        sample.environment,
        previous.internet_timeout,
        current.internet_timeout,
    )?;
    write_timeout_transition(
        event_log,
        timestamp,
        "ROUTER_HTTP_TIMEOUT detected",
        sample.environment,
        previous.router_http_timeout,
        current.router_http_timeout,
    )?;
    write_timeout_transition(
        event_log,
        timestamp,
        "DNS_LOOKUP_TIMEOUT detected",
        sample.environment,
        previous.dns_timeout,
        current.dns_timeout,
    )?;
    write_timeout_transition(
        event_log,
        timestamp,
        "HTTPS_GOOGLE_TIMEOUT detected",
        sample.environment,
        previous.https_google_timeout,
        current.https_google_timeout,
    )?;
    write_timeout_transition(
        event_log,
        timestamp,
        "HTTPS_CLOUDFLARE_TIMEOUT detected",
        sample.environment,
        previous.https_cloudflare_timeout,
        current.https_cloudflare_timeout,
    )?;

    *previous_statuses = Some(current);

    Ok(())
}

fn write_timeout_transition(
    event_log: &mut EventLog,
    timestamp: &str,
    event: &str,
    reading: EnvironmentReading,
    was_timeout: bool,
    is_timeout: bool,
) -> Result<()> {
    if is_timeout && !was_timeout {
        event_log.write_connectivity_event(timestamp, event, reading)?;
    }

    Ok(())
}

fn sleep_until_next_second(started_at: Instant, sample_index: u64) {
    let scheduled_at = started_at + Duration::from_secs(sample_index);
    sleep_until(scheduled_at);
}

fn sleep_until_duration_expires(started_at: Instant, duration: Duration) {
    sleep_until(started_at + duration);
}

fn sleep_until(deadline: Instant) {
    let now = Instant::now();

    if deadline > now {
        thread::sleep(deadline - now);
    }
}

fn print_startup(
    config: &MonitorConfig,
    total_samples: u64,
    finish_time: &str,
    environment_monitor: &EnvironmentMonitor,
) {
    println!("Monitoring started");
    println!("Router IP: {}", config.router_ip);
    println!("Internet IP: {}", config.internet_ip);
    println!(
        "Router HTTP: {}",
        http_monitor::router_http_url(config.router_ip)
    );
    println!("DNS Lookup: {}", dns_monitor::DNS_HOST);
    println!("HTTPS Google: {}", https_monitor::HTTPS_GOOGLE_URL);
    println!("HTTPS Cloudflare: {}", https_monitor::HTTPS_CLOUDFLARE_URL);
    match environment_monitor.location() {
        Some(location) => println!("Weather: enabled ({})", location.display_label()),
        None => println!("Weather: disabled (location unavailable)"),
    }
    println!("Duration: {}", config.duration.display());
    println!("Samples: {total_samples}");
    println!("Will finish at: {finish_time}");
    println!("Output file: {}", config.output_path.display());
    println!(
        "Event log: {}",
        event_log_path(&config.output_path).display()
    );
}

fn status_text(result: PingResult) -> String {
    match result.latency_ms {
        Some(latency) => format!("ok ({latency:.2} ms)"),
        None => "timeout".to_string(),
    }
}

fn print_progress(
    sample_number: u64,
    total_samples: u64,
    finish_time: &str,
    sample: &CsvSample,
    verbose: bool,
) -> Result<()> {
    let spinner = ["|", "/", "-", "\\"][(sample_number as usize) % 4];
    let percent = (sample_number as f64 / total_samples as f64 * 100.0).min(100.0);
    let detail = if verbose {
        format!(
            " | router={} internet={} router_http={} dns={} https_google={} https_cloudflare={}",
            status_text(sample.router),
            status_text(sample.internet),
            http_status_text(sample.router_http),
            dns_status_text(sample.dns_lookup),
            https_status_text(sample.https_google),
            https_status_text(sample.https_cloudflare),
        )
    } else {
        String::new()
    };
    let environment_detail = if verbose {
        format!(
            " weather=temp:{} humidity:{} apparent:{}",
            environment_text(sample.environment.outside_temperature_c),
            humidity_text(sample.environment.outside_relative_humidity),
            environment_text(sample.environment.outside_apparent_temperature_c)
        )
    } else {
        String::new()
    };

    print!(
        "\r\x1b[2K{spinner} Monitoring... {:>5.1}% ({sample_number}/{total_samples}) | finishes at {finish_time}{detail}{environment_detail}",
        percent,
    );
    io::stdout().flush()?;

    Ok(())
}

fn environment_text(value: Option<f64>) -> String {
    let text = format_temperature_field(value);

    if text.is_empty() {
        "unavailable".to_string()
    } else {
        format!("{text}C")
    }
}

fn humidity_text(value: Option<f64>) -> String {
    let text = format_humidity_field(value);

    if text.is_empty() {
        "unavailable".to_string()
    } else {
        format!("{text}%")
    }
}

fn http_status_text(result: HttpResult) -> String {
    match result.latency_ms {
        Some(latency) => format!("ok ({latency:.2} ms)"),
        None => "timeout".to_string(),
    }
}

fn dns_status_text(result: DnsResult) -> String {
    match result.latency_ms {
        Some(latency) => format!("ok ({latency:.2} ms)"),
        None => "timeout".to_string(),
    }
}

fn https_status_text(result: HttpsResult) -> String {
    match result.latency_ms {
        Some(latency) => format!("ok ({latency:.2} ms)"),
        None => "timeout".to_string(),
    }
}

fn clear_progress_line() -> Result<()> {
    print!("\r\x1b[2K");
    io::stdout().flush()?;

    Ok(())
}
