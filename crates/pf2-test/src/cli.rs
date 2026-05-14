use clap::{Parser, Subcommand, ValueEnum};
use protofish2::TransferMode;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "pf2-test", about = "Protofish2 audio streaming demo")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Server {
        #[arg(long, default_value = "127.0.0.1:5000")]
        bind: SocketAddr,
        #[arg(long, value_enum, default_value_t = Mode::Unreliable)]
        mode: Mode,
    },
    Client {
        #[arg(long)]
        addr: SocketAddr,
        #[arg(long)]
        file: PathBuf,
        #[arg(long, value_enum, default_value_t = Mode::Unreliable)]
        mode: Mode,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Mode {
    Reliable,
    Unreliable,
}

impl From<Mode> for TransferMode {
    fn from(value: Mode) -> Self {
        match value {
            Mode::Reliable => TransferMode::Dual,
            Mode::Unreliable => TransferMode::UnreliableOnly,
        }
    }
}
