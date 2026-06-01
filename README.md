# router-monitor

`router-monitor` is a Rust CLI utility that samples home-router, router HTTP, and Internet connectivity once per second and writes the measurements to CSV.

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
```

By default, monitoring starts in the background. The command prints the process ID, the expected finish time, and the output CSV path before returning control to the terminal. Use `--foreground` to keep it attached to the terminal and show an in-place progress animation.

Supported duration units are `s`, `m`, `h`, and `d`. Decimal values are allowed, such as `3.5m`, `2.25h`, and `0.5d`.

By default the tool pings router IP `192.168.1.1` and Internet IP `1.1.1.1`. Use `--router-ip` and `--internet-ip` to override them.

The router web interface is also measured once per sample using `http://<router-ip>`. Any HTTP response counts as success, including redirects and authentication errors, because the measurement is about responsiveness rather than login status. Requests time out after 3 seconds.

Use `-o router.csv` to write to a specific CSV file. Use `-o logs` or `-o logs/` to create/use a directory and write a default timestamped CSV file inside it.

## CSV Output

If no output file is supplied, the tool creates a file named `router-monitor-YYYYMMDD-HHMMSS.csv` in the current directory.

The CSV header is:

```csv
timestamp,router_ms,internet_ms,router_http_ms,router_status,internet_status,router_http_status
```

Successful latency values are numeric milliseconds. Failed pings or HTTP checks leave the latency cell empty and write `timeout` in the matching status column.

## Tests

```bash
cargo test
```
