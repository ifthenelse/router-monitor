use crate::csv_writer::{CsvLog, CsvSample};
use crate::duration::DurationSpec;
use crate::ping::{self, PingResult};
use anyhow::Result;
use chrono::Local;
use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub duration: DurationSpec,
    pub router_ip: Ipv4Addr,
    pub internet_ip: Ipv4Addr,
    pub output_path: PathBuf,
    pub verbose: bool,
    pub beep: bool,
}

pub fn run(config: MonitorConfig) -> Result<()> {
    let mut csv = CsvLog::create(&config.output_path)?;
    let total_samples = config.duration.sample_seconds();

    if config.verbose {
        print_startup(&config, total_samples);
    }

    let started_at = Instant::now();

    for sample_index in 0..total_samples {
        sleep_until_next_second(started_at, sample_index);

        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let (router, internet) = ping_targets(config.router_ip, config.internet_ip);
        let sample = CsvSample {
            timestamp,
            router,
            internet,
        };

        csv.write_sample(&sample)?;

        if config.verbose {
            print_sample_status(sample_index + 1, total_samples, &sample);
        }
    }

    sleep_until_duration_expires(started_at, config.duration.total());

    if config.verbose {
        println!("Monitoring completed");
        println!("Elapsed: {:.1}s", started_at.elapsed().as_secs_f64());
    }

    if config.beep {
        print!("\x07");
        io::stdout().flush()?;
    }

    Ok(())
}

fn ping_targets(router_ip: Ipv4Addr, internet_ip: Ipv4Addr) -> (PingResult, PingResult) {
    let router = router_ip.to_string();
    let internet = internet_ip.to_string();

    thread::scope(|scope| {
        let router_handle = scope.spawn(|| ping::ping_once(&router));
        let internet_handle = scope.spawn(|| ping::ping_once(&internet));

        let router_result = router_handle
            .join()
            .unwrap_or_else(|_| PingResult::timeout());
        let internet_result = internet_handle
            .join()
            .unwrap_or_else(|_| PingResult::timeout());

        (router_result, internet_result)
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

fn print_startup(config: &MonitorConfig, total_samples: u64) {
    println!("Monitoring started");
    println!("Router IP: {}", config.router_ip);
    println!("Internet IP: {}", config.internet_ip);
    println!("Duration: {}", config.duration.display());
    println!("Samples: {total_samples}");
    println!("Output file: {}", config.output_path.display());
}

fn print_sample_status(sample_number: u64, total_samples: u64, sample: &CsvSample) {
    println!(
        "Sample {sample_number}/{total_samples} at {}: router={}, internet={}",
        sample.timestamp,
        status_text(sample.router),
        status_text(sample.internet)
    );
}

fn status_text(result: PingResult) -> String {
    match result.latency_ms {
        Some(latency) => format!("ok ({latency:.2} ms)"),
        None => "timeout".to_string(),
    }
}
