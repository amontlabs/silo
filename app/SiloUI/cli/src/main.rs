use clap::Parser;
use silo_cli::cli::{run, Cli};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let directory = std::env::current_dir().unwrap_or_default();
    match run(cli, &directory).await {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("silo: {error}");
            std::process::exit(1);
        }
    }
}
