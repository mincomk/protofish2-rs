mod audio;
mod cert;
mod cli;
mod client;
mod server;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    match Cli::parse().command {
        Command::Server { bind, mode } => server::run(bind, mode).await,
        Command::Client { addr, mode, file } => client::run(addr, mode, &file).await,
    }
}
