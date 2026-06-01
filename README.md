# router-monitor

`router-monitor` is a Rust CLI utility that samples home-router and Internet connectivity once per second and writes the measurements to CSV.

[Download prebuilt executables from GitHub Releases.](../../releases)

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
router-monitor 10m -v
router-monitor 10m -o router.csv
router-monitor 10m -r 192.168.0.1
router-monitor 10m -i 8.8.8.8
```

Supported duration units are `s`, `m`, `h`, and `d`. Decimal values are allowed, such as `3.5m`, `2.25h`, and `0.5d`.

By default the tool pings router IP `192.168.1.1` and Internet IP `1.1.1.1`. Use `--router-ip` and `--internet-ip` to override them.

Use `-o router.csv` to write to a specific CSV file. Use `-o logs` or `-o logs/` to create/use a directory and write a default timestamped CSV file inside it.

## CSV Output

If no output file is supplied, the tool creates a file named `router-monitor-YYYYMMDD-HHMMSS.csv` in the current directory.

The CSV header is:

```csv
timestamp,router_ms,internet_ms,router_status,internet_status
```

Successful latency values are numeric milliseconds. Failed pings leave the latency cell empty and write `timeout` in the matching status column.

## Tests

```bash
cargo test
```
