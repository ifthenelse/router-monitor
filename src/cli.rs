use crate::duration::parse_duration;
use crate::monitor::MonitorConfig;
use crate::validation::parse_ipv4_address;
use anyhow::{Context, Result};
use chrono::Local;
use clap::{ArgAction, Parser};
use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

const DEFAULT_ROUTER_IP: &str = "192.168.1.1";
const DEFAULT_INTERNET_IP: &str = "1.1.1.1";

const AFTER_HELP: &str = "\
Duration syntax:
  Values without units are interpreted as seconds.
  Supported units are s, m, h, and d.
  Decimal values are allowed, for example 3.5m or 2.25h.
  Do not mix unitless values with unit-based values.
  Each unit may be specified only once.

Examples:

  router-monitor -v
  router-monitor --version
  router-monitor 15
  router-monitor 15s
  router-monitor 7m
  router-monitor 4m 20s
  router-monitor 1h 30m
  router-monitor 10m --verbose
  router-monitor 10m -o router.csv
  router-monitor 10m -r 192.168.0.1
  router-monitor 10m -i 8.8.8.8
";

#[derive(Debug, Parser)]
#[command(
    name = "router-monitor",
    version,
    disable_version_flag = true,
    about = "Monitor router and Internet connectivity and write measurements to CSV.",
    after_help = AFTER_HELP
)]
struct RawCli {
    /// Monitoring duration, such as 15, 15s, 4m 20s, or 1h 30m.
    #[arg(required_unless_present = "version", value_name = "DURATION", num_args = 1..)]
    duration: Vec<String>,

    /// Router IPv4 address to ping.
    #[arg(short = 'r', long = "router-ip", value_name = "IPv4", value_parser = parse_ipv4_address, default_value = DEFAULT_ROUTER_IP)]
    router_ip: Ipv4Addr,

    /// Internet IPv4 address to ping.
    #[arg(short = 'i', long = "internet-ip", value_name = "IPv4", value_parser = parse_ipv4_address, default_value = DEFAULT_INTERNET_IP)]
    internet_ip: Ipv4Addr,

    /// CSV output file path or directory.
    #[arg(short = 'o', long = "output", value_name = "FILE_OR_DIRECTORY")]
    output: Option<PathBuf>,

    /// Print the application version.
    #[arg(short = 'v', long = "version", action = ArgAction::SetTrue)]
    version: bool,

    /// Print startup, progress, and completion messages.
    #[arg(long = "verbose")]
    verbose: bool,

    /// Emit a terminal bell when monitoring completes.
    #[arg(short = 'b', long = "beep")]
    beep: bool,
}

pub fn parse_args() -> Result<MonitorConfig> {
    let raw = RawCli::parse();

    if raw.version {
        println!("{}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    raw.try_into()
}

impl TryFrom<RawCli> for MonitorConfig {
    type Error = anyhow::Error;

    fn try_from(raw: RawCli) -> Result<Self> {
        let duration = parse_duration(&raw.duration)?;
        let output_path = output_path(raw.output)?;

        Ok(Self {
            duration,
            router_ip: raw.router_ip,
            internet_ip: raw.internet_ip,
            output_path,
            verbose: raw.verbose,
            beep: raw.beep,
        })
    }
}

fn output_path(path: Option<PathBuf>) -> Result<PathBuf> {
    match path {
        Some(path) => resolve_output_path(expand_tilde(path)?),
        None => default_output_path_in_current_directory(),
    }
}

fn resolve_output_path(path: PathBuf) -> Result<PathBuf> {
    if should_treat_as_directory(&path) {
        fs::create_dir_all(&path)
            .with_context(|| format!("Cannot create output directory '{}'.", path.display()))?;

        return Ok(path.join(default_output_filename()));
    }

    Ok(path)
}

fn default_output_path_in_current_directory() -> Result<PathBuf> {
    std::env::current_dir()
        .map(|directory| directory.join(default_output_filename()))
        .context("Cannot determine the current working directory.")
}

fn default_output_filename() -> String {
    format!(
        "router-monitor-{}.csv",
        Local::now().format("%Y%m%d-%H%M%S")
    )
}

fn should_treat_as_directory(path: &Path) -> bool {
    path.is_dir() || ends_with_path_separator(path) || path.extension().is_none()
}

fn ends_with_path_separator(path: &Path) -> bool {
    let text = path.as_os_str().to_string_lossy();
    text.ends_with('/') || text.ends_with('\\')
}

fn expand_tilde(path: PathBuf) -> Result<PathBuf> {
    let text = path.to_string_lossy();

    if text == "~" {
        return home_dir();
    }

    if let Some(rest) = text.strip_prefix("~/") {
        return home_dir().map(|home| home.join(rest));
    }

    Ok(path)
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("Cannot expand '~' because the HOME environment variable is not set.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn explicit_csv_output_remains_a_file_path() {
        let path = PathBuf::from("router.csv");

        assert_eq!(resolve_output_path(path.clone()).unwrap(), path);
    }

    #[test]
    fn existing_directory_receives_default_output_filename() {
        let directory = unique_temp_path("existing-output-dir");
        fs::create_dir_all(&directory).unwrap();

        let resolved = resolve_output_path(directory.clone()).unwrap();

        assert_eq!(resolved.parent(), Some(directory.as_path()));
        assert_default_output_filename(resolved.file_name().unwrap().to_string_lossy().as_ref());

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn missing_directory_is_created_and_receives_default_output_filename() {
        let directory = unique_temp_path("missing-output-dir");

        let resolved = resolve_output_path(directory.clone()).unwrap();

        assert!(directory.is_dir());
        assert_eq!(resolved.parent(), Some(directory.as_path()));
        assert_default_output_filename(resolved.file_name().unwrap().to_string_lossy().as_ref());

        fs::remove_dir_all(directory).unwrap();
    }

    fn unique_temp_path(label: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "router-monitor-{label}-{}-{timestamp}",
            std::process::id()
        ))
    }

    fn assert_default_output_filename(filename: &str) {
        assert!(filename.starts_with("router-monitor-"), "{filename}");
        assert!(filename.ends_with(".csv"), "{filename}");
    }
}
