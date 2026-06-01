use crate::ping::{PingResult, PingStatus};
use anyhow::{Context, Result};
use csv::Writer;
use std::fs::{File, OpenOptions};
use std::io::BufWriter;
use std::path::Path;

const HEADER: [&str; 5] = [
    "timestamp",
    "router_ms",
    "internet_ms",
    "router_status",
    "internet_status",
];

pub struct CsvSample {
    pub timestamp: String,
    pub router: PingResult,
    pub internet: PingResult,
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
        let router_status = status_field(sample.router.status);
        let internet_status = status_field(sample.internet.status);

        self.writer.write_record([
            sample.timestamp.as_str(),
            router_ms.as_str(),
            internet_ms.as_str(),
            router_status,
            internet_status,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_failed_latency_as_empty_string() {
        assert_eq!(latency_field(None), "");
    }

    #[test]
    fn formats_successful_latency_as_numeric_text() {
        assert_eq!(latency_field(Some(8.214)), "8.21");
    }
}
