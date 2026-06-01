use crate::http_monitor::{HttpResult, HttpStatus};
use crate::ping::{PingResult, PingStatus};
use anyhow::{Context, Result};
use csv::Writer;
use std::fs::{File, OpenOptions};
use std::io::BufWriter;
use std::path::Path;

const HEADER: [&str; 7] = [
    "timestamp",
    "router_ms",
    "internet_ms",
    "router_http_ms",
    "router_status",
    "internet_status",
    "router_http_status",
];

pub struct CsvSample {
    pub timestamp: String,
    pub router: PingResult,
    pub internet: PingResult,
    pub router_http: HttpResult,
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
        let router_status = status_field(sample.router.status);
        let internet_status = status_field(sample.internet.status);
        let router_http_status = http_status_field(sample.router_http.status);

        self.writer.write_record([
            sample.timestamp.as_str(),
            router_ms.as_str(),
            internet_ms.as_str(),
            router_http_ms.as_str(),
            router_status,
            internet_status,
            router_http_status,
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

fn status_field(status: PingStatus) -> &'static str {
    status.as_csv_value()
}

fn http_status_field(status: HttpStatus) -> &'static str {
    status.as_csv_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_monitor::HttpResult;
    use crate::ping::PingResult;
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
    fn writes_http_latency_and_status_columns() {
        let path = unique_temp_path();
        let sample = CsvSample {
            timestamp: "2026-06-01 18:00:03".to_string(),
            router: PingResult::ok(0.704),
            internet: PingResult::ok(8.2),
            router_http: HttpResult::timeout(),
        };

        {
            let mut log = CsvLog::create(&path).unwrap();
            log.write_sample(&sample).unwrap();
        }

        let content = fs::read_to_string(&path).unwrap();

        assert_eq!(
            content,
            "timestamp,router_ms,internet_ms,router_http_ms,router_status,internet_status,router_http_status\n2026-06-01 18:00:03,0.70,8.20,,ok,ok,timeout\n"
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
