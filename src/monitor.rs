use crate::csv_writer::{CsvLog, CsvSample};
use crate::duration::DurationSpec;
use crate::http_monitor::{self, HttpMonitor, HttpResult};
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
    pub output_path: PathBuf,
    pub run_in_background: bool,
    pub show_progress: bool,
    pub verbose: bool,
    pub beep: bool,
}

pub fn run(config: MonitorConfig) -> Result<()> {
    let mut csv = CsvLog::create(&config.output_path)?;
    let http_monitor = HttpMonitor::new()?;
    let total_samples = config.duration.sample_seconds();
    let finish_time = finish_time(config.duration.total());

    if config.show_progress {
        print_startup(&config, total_samples, &finish_time);
    }

    let started_at = Instant::now();

    for sample_index in 0..total_samples {
        sleep_until_next_second(started_at, sample_index);

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let (router, internet, router_http) =
            measure_targets(config.router_ip, config.internet_ip, &http_monitor);
        let sample = CsvSample {
            timestamp,
            router,
            internet,
            router_http,
        };

        csv.write_sample(&sample)?;

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
) -> (PingResult, PingResult, HttpResult) {
    let router = router_ip.to_string();
    let internet = internet_ip.to_string();
    let http_monitor = http_monitor.clone();

    thread::scope(|scope| {
        let router_handle = scope.spawn(|| ping::ping_once(&router));
        let internet_handle = scope.spawn(|| ping::ping_once(&internet));
        let router_http_handle = scope.spawn(|| http_monitor.measure_router(router_ip));

        let router_result = router_handle
            .join()
            .unwrap_or_else(|_| PingResult::timeout());
        let internet_result = internet_handle
            .join()
            .unwrap_or_else(|_| PingResult::timeout());
        let router_http_result = router_http_handle
            .join()
            .unwrap_or_else(|_| HttpResult::timeout());

        (router_result, internet_result, router_http_result)
    })
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

fn print_startup(config: &MonitorConfig, total_samples: u64, finish_time: &str) {
    println!("Monitoring started");
    println!("Router IP: {}", config.router_ip);
    println!("Internet IP: {}", config.internet_ip);
    println!(
        "Router HTTP: {}",
        http_monitor::router_http_url(config.router_ip)
    );
    println!("Duration: {}", config.duration.display());
    println!("Samples: {total_samples}");
    println!("Will finish at: {finish_time}");
    println!("Output file: {}", config.output_path.display());
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
            " | router={} internet={} router_http={}",
            status_text(sample.router),
            status_text(sample.internet),
            http_status_text(sample.router_http)
        )
    } else {
        String::new()
    };

    print!(
        "\r\x1b[2K{spinner} Monitoring... {:>5.1}% ({sample_number}/{total_samples}) | finishes at {finish_time}{detail}",
        percent,
    );
    io::stdout().flush()?;

    Ok(())
}

fn http_status_text(result: HttpResult) -> String {
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
