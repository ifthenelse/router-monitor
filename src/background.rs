use crate::csv_writer;
use crate::event_log::{self, event_log_path};
use crate::monitor::{self, MonitorConfig};
use crate::stop_hint;
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
    println!("{}", stop_hint::background_stop_hint(child.id()));
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

    if let Some(interface) = config.wifi.explicit_interface() {
        command.arg("--wifi-interface").arg(interface);
    }

    command.arg("--tcp-target").arg(&config.tcp.target);

    if let Some(url) = &config.websocket.url {
        command.arg("--websocket-url").arg(url);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duration::parse_duration;
    use crate::environment_monitor::EnvironmentConfig;
    use crate::tcp_monitor::TcpConfig;
    use crate::websocket_monitor::{WebSocketConfig, DEFAULT_WEBSOCKET_URL};
    use crate::wifi_monitor::WifiConfig;
    use std::ffi::OsStr;
    use std::net::Ipv4Addr;
    use std::path::PathBuf;

    #[test]
    fn background_command_keeps_explicit_wifi_interface_override() {
        let config = test_config(WifiConfig::new("wlan0".to_string()));
        let command = background_command(&config).unwrap();
        let args = command_args(&command);

        assert!(args.contains(&"--wifi-interface".to_string()));
        assert!(args.contains(&"wlan0".to_string()));
    }

    #[test]
    fn background_command_keeps_wifi_auto_detection_implicit() {
        let config = test_config(WifiConfig::auto());
        let command = background_command(&config).unwrap();
        let args = command_args(&command);

        assert!(!args.contains(&"--wifi-interface".to_string()));
    }

    #[test]
    fn background_command_marks_child_process() {
        let config = test_config(WifiConfig::auto());
        let command = background_command(&config).unwrap();

        assert_eq!(
            command.get_envs().find_map(|(name, value)| {
                (name == OsStr::new(BACKGROUND_CHILD_ENV))
                    .then(|| value.map(|value| value.to_string_lossy().into_owned()))
                    .flatten()
            }),
            Some("1".to_string())
        );
    }

    fn test_config(wifi: WifiConfig) -> MonitorConfig {
        MonitorConfig {
            duration: parse_duration(&["10m".to_string()]).unwrap(),
            duration_parts: vec!["10m".to_string()],
            router_ip: Ipv4Addr::new(192, 168, 1, 1),
            internet_ip: Ipv4Addr::new(1, 1, 1, 1),
            environment: EnvironmentConfig { location: None },
            wifi,
            tcp: TcpConfig {
                target: "1.1.1.1:443".to_string(),
            },
            websocket: WebSocketConfig {
                url: Some(DEFAULT_WEBSOCKET_URL.to_string()),
            },
            output_path: PathBuf::from("router-monitor-test.csv"),
            run_in_background: true,
            show_progress: false,
            verbose: false,
            beep: false,
        }
    }

    fn command_args(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }
}
