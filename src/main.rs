mod background;
mod cli;
mod csv_writer;
mod duration;
mod errors;
mod http_monitor;
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

    if config.run_in_background {
        return background::spawn(config);
    }

    monitor::run(config)
}
