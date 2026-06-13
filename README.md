# router-monitor

`router-monitor` is a Rust CLI utility that samples home-router, router HTTP, DNS, HTTPS, and Internet connectivity once per second and writes the measurements to CSV.

[Download prebuilt executables from GitHub Releases.](../../releases)

For Raspberry Pi OS, use the `aarch64-unknown-linux-musl` release on 64-bit installs and the `armv7-unknown-linux-musleabihf` release on 32-bit installs. Run `uname -m` on the Pi if you are unsure.

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
```

By default, monitoring starts in the background. The command prints the process ID, the expected finish time, and the output CSV path before returning control to the terminal. Use `--foreground` to keep it attached to the terminal and show an in-place progress animation.

Supported duration units are `s`, `m`, `h`, and `d`. Decimal values are allowed, such as `3.5m`, `2.25h`, and `0.5d`.

By default the tool pings router IP `192.168.1.1` and Internet IP `1.1.1.1`. Use `--router-ip` and `--internet-ip` to override them.

The router web interface is also measured once per sample using `http://<router-ip>`. Any HTTP response counts as success, including redirects and authentication errors, because the measurement is about responsiveness rather than login status. Requests time out after 3 seconds.

Application-level connectivity is measured with a DNS lookup for `google.com`, an HTTPS request to `https://www.google.com`, and an HTTPS request to `https://1.1.1.1`. Each application-level check uses a 3-second timeout and reports only `ok` or `timeout`.

Environmental data is collected every 5 minutes and cached between refreshes. By default the tool attempts city-level public IP geolocation and caches the resolved latitude/longitude, place metadata, source, and precision under the user cache directory. This does not require GPS hardware and is usually accurate enough for city-level weather correlation. Use `--latitude` and `--longitude` to provide an explicit location. If geolocation or weather lookup is unavailable, weather collection is disabled gracefully and the weather CSV cells remain empty.

Weather data currently comes from Open-Meteo. The weather provider is isolated behind an internal trait so future providers or local sensors can be added without changing the monitoring loop. On Raspberry Pi systems, the event log also includes the CPU temperature in Celsius when `/sys/class/thermal/thermal_zone0/temp` is available.

Use `-o router.csv` to write to a specific CSV file. Use `-o logs` or `-o logs/` to create/use a directory and write a default timestamped CSV file inside it.

## CSV Output

If no output file is supplied, the tool creates a file named `router-monitor-YYYYMMDD-HHMMSS.csv` in the current directory.

The CSV header is:

```csv
timestamp,script_version,router_ms,internet_ms,router_http_ms,dns_lookup_ms,https_google_ms,https_cloudflare_ms,outside_temperature_c,outside_relative_humidity,outside_apparent_temperature_c,router_status,internet_status,router_http_status,dns_status,https_google_status,https_cloudflare_status
```

`script_version` is the current `router-monitor` package version. Successful latency values are numeric milliseconds. Failed ping, DNS, HTTP, or HTTPS checks leave the latency cell empty and write `timeout` in the matching status column. Weather values are written as Celsius temperature, relative humidity percentage, and apparent Celsius temperature; unavailable weather values are empty.

## Event Log

Each run also creates a sidecar event log next to the CSV, for example `router-monitor-20260613-193000.events.log`.

When refreshed weather data indicates any of these conditions, the event log records `HIGH_HEAT_CONDITIONS`:

```text
outside_temperature_c >= 30
outside_apparent_temperature_c >= 35
outside_relative_humidity >= 75
```

Connectivity timeout transitions are also written with the current environmental values so later analysis can compare network instability with heat, humidity, and Raspberry Pi thermal state.

## Tests

```bash
cargo test
```
