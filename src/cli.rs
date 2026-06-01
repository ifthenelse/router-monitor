use crate::duration::parse_duration;
use crate::monitor::MonitorConfig;
use crate::validation::parse_ipv4_address;
use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;
use std::net::Ipv4Addr;
use std::path::PathBuf;

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

  router-monitor 15
  router-monitor 15s
  router-monitor 7m
  router-monitor 4m 20s
  router-monitor 1h 30m
  router-monitor 10m -v
  router-monitor 10m -o router.csv
  router-monitor 10m -r 192.168.0.1
  router-monitor 10m -i 8.8.8.8
";

#[derive(Debug, Parser)]
#[command(
    name = "router-monitor",
    about = "Monitor router and Internet connectivity and write measurements to CSV.",
    after_help = AFTER_HELP
)]
struct RawCli {
    /// Monitoring duration, such as 15, 15s, 4m 20s, or 1h 30m.
    #[arg(required = true, value_name = "DURATION", num_args = 1..)]
    duration: Vec<String>,

    /// Router IPv4 address to ping.
    #[arg(short = 'r', long = "router-ip", value_name = "IPv4", value_parser = parse_ipv4_address, default_value = DEFAULT_ROUTER_IP)]
    router_ip: Ipv4Addr,

    /// Internet IPv4 address to ping.
    #[arg(short = 'i', long = "internet-ip", value_name = "IPv4", value_parser = parse_ipv4_address, default_value = DEFAULT_INTERNET_IP)]
    internet_ip: Ipv4Addr,

    /// CSV output file path.
    #[arg(short = 'o', long = "output", value_name = "FILE")]
    output: Option<PathBuf>,

    /// Print startup, progress, and completion messages.
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

    /// Emit a terminal bell when monitoring completes.
    #[arg(short = 'b', long = "beep")]
    beep: bool,
}

pub fn parse_args() -> Result<MonitorConfig> {
    let raw = RawCli::parse();
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
        Some(path) => expand_tilde(path),
        None => {
            let filename = format!(
                "router-monitor-{}.csv",
                Local::now().format("%Y%m%d-%H%M%S")
            );

            std::env::current_dir()
                .map(|directory| directory.join(filename))
                .context("Cannot determine the current working directory.")
        }
    }
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
