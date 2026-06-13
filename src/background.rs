use crate::csv_writer;
use crate::event_log::{self, event_log_path};
use crate::monitor::{self, MonitorConfig};
use anyhow::{Context, Result};
use std::process::{Command, Stdio};

const BACKGROUND_CHILD_ENV: &str = "ROUTER_MONITOR_BACKGROUND_CHILD";

pub fn is_background_child() -> bool {
    std::env::var_os(BACKGROUND_CHILD_ENV).is_some()
}

pub fn spawn(config: MonitorConfig) -> Result<()> {
    csv_writer::verify_writable(&config.output_path)?;
    let event_log_path = event_log_path(&config.output_path);
    event_log::verify_writable(&event_log_path)?;

    let finish_time = monitor::finish_time(config.duration.total());
    let mut command = background_command(&config)?;
    let child = command
        .spawn()
        .context("Cannot start router-monitor in the background.")?;

    println!("Monitoring started in background");
    println!("Process ID: {}", child.id());
    println!("Will finish at: {finish_time}");
    println!("Output file: {}", config.output_path.display());
    println!("Event log: {}", event_log_path.display());

    Ok(())
}

fn background_command(config: &MonitorConfig) -> Result<Command> {
    let executable =
        std::env::current_exe().context("Cannot determine the current executable path.")?;
    let mut command = Command::new(executable);

    command
        .args(&config.duration_parts)
        .arg("--router-ip")
        .arg(config.router_ip.to_string())
        .arg("--internet-ip")
        .arg(config.internet_ip.to_string())
        .arg("--output")
        .arg(&config.output_path)
        .env(BACKGROUND_CHILD_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    if let Some(location) = config.environment.location {
        command
            .arg("--latitude")
            .arg(location.latitude.to_string())
            .arg("--longitude")
            .arg(location.longitude.to_string());
    }

    if config.beep {
        command.arg("--beep");
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        command.process_group(0);
    }

    Ok(command)
}
