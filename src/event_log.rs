use crate::environment_monitor::{
    format_humidity_field, format_temperature_field, EnvironmentReading,
};
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

pub struct EventLog {
    writer: BufWriter<File>,
}

impl EventLog {
    pub fn create(path: &Path) -> Result<Self> {
        let file = File::create(path)
            .with_context(|| format!("Cannot write to event log '{}'.", path.display()))?;

        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    pub fn write_high_heat_conditions(
        &mut self,
        timestamp: &str,
        reading: EnvironmentReading,
    ) -> Result<()> {
        writeln!(
            self.writer,
            "{timestamp} HIGH_HEAT_CONDITIONS temperature={} humidity={} apparent={}",
            format_temperature_field(reading.outside_temperature_c),
            format_humidity_field(reading.outside_relative_humidity),
            format_temperature_field(reading.outside_apparent_temperature_c)
        )?;
        self.writer.flush()?;

        Ok(())
    }

    pub fn write_connectivity_event(
        &mut self,
        timestamp: &str,
        event: &str,
        reading: EnvironmentReading,
    ) -> Result<()> {
        writeln!(self.writer, "{timestamp} {event}")?;
        write_environment_lines(&mut self.writer, reading)?;
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
        .with_context(|| format!("Cannot write to event log '{}'.", path.display()))?;

    Ok(())
}

pub fn event_log_path(csv_path: &Path) -> PathBuf {
    csv_path.with_extension("events.log")
}

fn write_environment_lines<W: Write>(writer: &mut W, reading: EnvironmentReading) -> Result<()> {
    writeln!(
        writer,
        "Temperature: {}",
        format_temperature_for_event(reading.outside_temperature_c)
    )?;
    writeln!(
        writer,
        "Humidity: {}",
        format_humidity_for_event(reading.outside_relative_humidity)
    )?;
    writeln!(
        writer,
        "Apparent temperature: {}",
        format_temperature_for_event(reading.outside_apparent_temperature_c)
    )?;

    if let Some(temperature) = reading.raspberry_pi_temperature_c {
        writeln!(writer, "Raspberry Pi temperature: {temperature:.1}C")?;
    }

    Ok(())
}

fn format_temperature_for_event(value: Option<f64>) -> String {
    value
        .map(|temperature| format!("{temperature:.1}C"))
        .unwrap_or_else(|| "unavailable".to_string())
}

fn format_humidity_for_event(value: Option<f64>) -> String {
    value
        .map(|humidity| format!("{humidity:.0}%"))
        .unwrap_or_else(|| "unavailable".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn builds_sidecar_event_log_path() {
        assert_eq!(
            event_log_path(Path::new("router.csv")),
            PathBuf::from("router.events.log")
        );
    }

    #[test]
    fn writes_environment_values_with_connectivity_events() {
        let path = unique_temp_path();
        let mut log = EventLog::create(&path).unwrap();

        log.write_connectivity_event(
            "2026-06-13 20:15:41",
            "TCP disconnect detected",
            EnvironmentReading {
                outside_temperature_c: Some(32.1),
                outside_relative_humidity: Some(71.0),
                outside_apparent_temperature_c: Some(38.5),
                raspberry_pi_temperature_c: Some(54.2),
            },
        )
        .unwrap();

        let content = fs::read_to_string(&path).unwrap();

        assert_eq!(
            content,
            "2026-06-13 20:15:41 TCP disconnect detected\nTemperature: 32.1C\nHumidity: 71%\nApparent temperature: 38.5C\nRaspberry Pi temperature: 54.2C\n"
        );

        fs::remove_file(path).unwrap();
    }

    fn unique_temp_path() -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "router-monitor-event-log-test-{}-{timestamp}.log",
            std::process::id()
        ))
    }
}
