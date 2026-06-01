mod cli;
mod csv_writer;
mod duration;
mod errors;
mod monitor;
mod ping;
mod validation;

use anyhow::Result;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let config = cli::parse_args()?;

    monitor::run(config)
}
