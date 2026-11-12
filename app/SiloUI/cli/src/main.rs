use clap::Parser;
use silo_cli::cli::{run, Cli};

#[tokio::main]
async fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1 && arguments[0] == silo_cli::engine::CHANNEL_FLAG {
        println!("{}", silo_cli::engine::build_channel().identifier());
        return;
    }
    let cli = Cli::parse();
    let directory = std::env::current_dir().unwrap_or_default();
    match run(cli, &directory).await {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("silo: {error}");
            std::process::exit(error.exit);
        }
    }
}
