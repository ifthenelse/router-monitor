# router-monitor

`router-monitor` is a Rust CLI utility that samples home-router, router HTTP, DNS, HTTPS, and Internet connectivity once per second and writes the measurements to CSV.

[Download prebuilt executables from GitHub Releases.](../../releases)

Choose the release that matches the operating system and CPU architecture of the device where the monitor will run. On Unix-like systems, `uname -m` can help identify the architecture.

[![CI](https://github.com/ifthenelse/router-monitor/actions/workflows/ci.yml/badge.svg)](https://github.com/ifthenelse/router-monitor/actions/workflows/ci.yml)

## Build

```bash
cargo build --release
```

The release binary will be available at:

```bash
target/release/router-monitor
```

## Usage

```bash
router-monitor 15
router-monitor 15s
router-monitor 7m
router-monitor 4m 20s
router-monitor 1h 30m
router-monitor -v
router-monitor --version
router-monitor 10m --foreground
router-monitor 10m --foreground --verbose
router-monitor 10m -o router.csv
router-monitor 10m -r 192.168.0.1
router-monitor 10m -i 8.8.8.8
router-monitor 12h --latitude 45.484 --longitude 9.204
router-monitor 12h --wifi-interface wlan0
router-monitor 12h --tcp-target 1.1.1.1:443
router-monitor 12h --websocket-url wss://example.com/socket
```

By default, monitoring starts in the background. The command prints the process ID, the expected finish time, and the output CSV path before returning control to the terminal. Use `--foreground` to keep it attached to the terminal and show an in-place progress animation.

## Stopping a Run

`router-monitor` does not install an operating-system service by default. A background run is a normal process, and the startup message prints the process ID:

```text
Monitoring started in background
Process ID: 12345
```

If the monitor is running in the foreground, press `Ctrl+C` in the terminal where it is running.

If the monitor is running in the background, stop it with the printed process ID:

| Platform                    | Stop by process ID             | Find a running monitor if the PID was lost |
| --------------------------- | ------------------------------ | ------------------------------------------ |
| Debian, Ubuntu, Arch Linux  | `kill 12345`                   | `pgrep -af router-monitor`                 |
| macOS                       | `kill 12345`                   | `pgrep -af router-monitor`                 |
| Windows PowerShell          | `Stop-Process -Id 12345`       | `Get-Process router-monitor`               |
| Windows Command Prompt      | `taskkill /PID 12345`          | `tasklist \| findstr router-monitor`       |

On Linux systems where you have wrapped the binary in a systemd unit, stop that wrapper instead:

```bash
sudo systemctl stop router-monitor
```

Supported duration units are `s`, `m`, `h`, and `d`. Decimal values are allowed, such as `3.5m`, `2.25h`, and `0.5d`.

By default the tool pings router IP `192.168.1.1` and Internet IP `1.1.1.1`. Use `--router-ip` and `--internet-ip` to override them.

The router web interface is also measured once per sample using `http://<router-ip>`. Any HTTP response counts as success, including redirects and authentication errors, because the measurement is about responsiveness rather than login status. Requests time out after 3 seconds.

Application-level connectivity is measured with a DNS lookup for `google.com`, an HTTPS request to `https://www.google.com`, and an HTTPS request to `https://1.1.1.1`. Each application-level check uses a 3-second timeout and reports only `ok` or `timeout`.

To compare the host's default network path against a WiFi adapter, pass `--wifi-interface wlan0`. WiFi monitoring reads Linux `iw dev wlan0 link` data, `/proc/net/wireless` link quality, and performs router/Internet pings bound to that interface.

The persistent TCP monitor is enabled by default and connects to `1.1.1.1:443`; use `--tcp-target HOST:PORT` to change it. WebSocket monitoring is enabled by default and connects to the public echo server `wss://echo.websocket.org`; use `--websocket-url URL` to change it.

The client-link monitor is isolated behind a trait so future platform-specific providers can emit the same WiFi/session metric shape without changing CSV or correlation logic.

Core monitoring, weather, TCP, WebSocket, CSV, and event logging are portable across Linux, macOS, and Windows. WiFi telemetry currently uses Linux `iw` and `/proc/net/wireless`; on macOS and Windows `--wifi-interface` is ignored gracefully until native client-link providers are added.

Environmental data is collected every 5 minutes and cached between refreshes. By default the tool attempts city-level public IP geolocation and caches the resolved latitude/longitude, place metadata, source, and precision under the user cache directory. This does not require GPS hardware and is usually accurate enough for city-level weather correlation. Use `--latitude` and `--longitude` to provide an explicit location. If geolocation or weather lookup is unavailable, weather collection is disabled gracefully and the weather CSV cells remain empty.

Weather data currently comes from Open-Meteo. The weather provider is isolated behind an internal trait so future providers or local sensors can be added without changing the monitoring loop. On Linux systems, the event log also includes the local thermal-zone temperature in Celsius when `/sys/class/thermal/thermal_zone0/temp` is available.

Use `-o router.csv` to write to a specific CSV file. Use `-o logs` or `-o logs/` to create/use a directory and write a default timestamped CSV file inside it.

## Defaults

| Setting                         | Default                                                                              |
| ------------------------------- | ------------------------------------------------------------------------------------ |
| Run mode                        | Background process                                                                   |
| Sample interval                 | 1 second                                                                             |
| Duration syntax without units   | Seconds                                                                              |
| Router IP                       | `192.168.1.1`                                                                        |
| Internet ping IP                | `1.1.1.1`                                                                            |
| Router HTTP URL                 | `http://<router-ip>`                                                                 |
| DNS lookup host                 | `google.com`                                                                         |
| HTTPS Google URL                | `https://www.google.com`                                                             |
| HTTPS Cloudflare URL            | `https://1.1.1.1`                                                                    |
| HTTP/DNS/HTTPS timeout          | 3 seconds                                                                            |
| TCP monitor target              | `1.1.1.1:443`                                                                        |
| WebSocket monitor URL           | `wss://echo.websocket.org`                                                           |
| WebSocket heartbeat interval    | 15 seconds                                                                           |
| WiFi monitoring                 | Disabled unless `--wifi-interface` is supplied                                       |
| WiFi low signal threshold       | `-75 dBm`                                                                            |
| WiFi low link-quality threshold | `35`                                                                                 |
| Weather provider                | Open-Meteo                                                                           |
| Weather refresh interval        | 5 minutes                                                                            |
| Location                        | City-level public IP geolocation, unless `--latitude` and `--longitude` are supplied |
| Output CSV                      | `router-monitor-YYYYMMDD-HHMMSS.csv` in the current directory                        |
| Event log                       | Same path as CSV with `.events.log` extension                                        |
| Verbose output                  | Disabled                                                                             |
| Completion beep                 | Disabled                                                                             |
| Foreground mode                 | Disabled unless `--foreground` is supplied                                           |

## CSV Output

If no output file is supplied, the tool creates a file named `router-monitor-YYYYMMDD-HHMMSS.csv` in the current directory.

The CSV header is:

```csv
timestamp,script_version,router_ms,internet_ms,router_http_ms,dns_lookup_ms,https_google_ms,https_cloudflare_ms,outside_temperature_c,outside_relative_humidity,outside_apparent_temperature_c,wifi_signal_dbm,wifi_link_quality,wifi_tx_bitrate,wifi_rx_bitrate,wifi_router_ms,wifi_internet_ms,wifi_disconnect_count,wifi_reconnect_count,wifi_associated_bssid,wifi_channel,tcp_connected,tcp_disconnect_count,tcp_reconnect_count,tcp_session_uptime_seconds,last_tcp_disconnect,websocket_connected,websocket_disconnect_count,websocket_reconnect_count,websocket_uptime_seconds,last_websocket_disconnect,suspected_failure_layer,router_status,internet_status,router_http_status,dns_status,https_google_status,https_cloudflare_status
```

`script_version` is the current `router-monitor` package version. Successful latency values are numeric milliseconds. Failed ping, DNS, HTTP, HTTPS, or WiFi-bound ping checks leave the latency cell empty and write `timeout` in the matching status column when a status column exists. Weather values are written as Celsius temperature, relative humidity percentage, and apparent Celsius temperature; unavailable weather values are empty.

`suspected_failure_layer` is one of:

```text
none
wifi
session
internet
router
unknown
```

## Event Log

Each run also creates a sidecar event log next to the CSV, for example `router-monitor-20260613-193000.events.log`.

When refreshed weather data indicates any of these conditions, the event log records `HIGH_HEAT_CONDITIONS`:

```text
outside_temperature_c >= 30
outside_apparent_temperature_c >= 35
outside_relative_humidity >= 75
```

Connectivity timeout transitions are also written with the current environmental values so later analysis can compare network instability with heat, humidity, and local device thermal state when available.

WiFi, TCP, WebSocket, and correlation events are also written, including:

```text
WIFI_SIGNAL_LOW
WIFI_DISCONNECTED
WIFI_RECONNECTED
TCP_DISCONNECTED
TCP_RECONNECTED
WEBSOCKET_DISCONNECTED
WEBSOCKET_RECONNECTED
POSSIBLE_WIFI_LAYER_FAILURE
POSSIBLE_SESSION_LAYER_FAILURE
```

## Tests

```bash
cargo test
```
