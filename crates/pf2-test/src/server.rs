use crate::audio::{MAX_QUEUED_FRAMES, Player, SAMPLE_RATE};
use crate::cert::write_self_signed;
use crate::cli::Mode;
use anyhow::{Context, Result};
use protofish2::{
    ManiTransferRecvStreams,
    compression::CompressionType,
    config::ProtofishConfig,
    connection::{ProtofishServer, ServerConfig},
    mani::transfer::{
        jitter::OpusJitterBuffer,
        opus::OpusDecoderStream,
        recv::{TransferReliableRecvStream, TransferUnreliableRecvStream},
    },
};
use std::net::SocketAddr;
use std::time::Duration;

const FRAME_MS: u64 = 20;
const PLAYOUT_DELAY_MS: u64 = 100;

pub async fn run(bind: SocketAddr, mode: Mode) -> Result<()> {
    let (cert_chain, private_key) = write_self_signed()?;

    let mut protofish_config = ProtofishConfig::default();
    protofish_config.mani_config.initial_backpressure_credits = 10;
    protofish_config.mani_config.backpressure_credit_batch_size = 1;

    let config = ServerConfig {
        bind_address: bind,
        cert_chain,
        private_key,
        supported_compression_types: vec![CompressionType::None],
        keepalive_interval: Duration::from_secs(5),
        protofish_config,
    };
    let server = ProtofishServer::bind(config).context("bind server")?;
    tracing::info!(addr = %server.local_addr()?, "server listening");

    let incoming = server.accept().await.context("no incoming connection")?;
    let conn = incoming.accept().await.context("accept connection")?;
    let mut stream = conn.accept_mani().await.context("accept mani")?;
    let transfer = stream.accept_transfer().await.context("accept transfer")?;

    let player = Player::new()?;

    match (mode, transfer) {
        (Mode::Unreliable, ManiTransferRecvStreams::UnreliableOnly { unreliable }) => {
            play_unreliable(unreliable, &player).await?;
        }
        (
            Mode::Reliable,
            ManiTransferRecvStreams::Dual {
                unreliable,
                reliable,
            },
        ) => {
            tokio::spawn(drain_unreliable(unreliable));
            play_reliable(reliable, &player).await?;
        }
        (mode, transfer) => {
            anyhow::bail!(
                "mode/transfer mismatch: cli={:?}, peer sent {}",
                mode,
                describe_transfer(&transfer),
            );
        }
    }

    player.wait_drained();
    Ok(())
}

fn describe_transfer(t: &ManiTransferRecvStreams) -> &'static str {
    match t {
        ManiTransferRecvStreams::Dual { .. } => "Dual (reliable)",
        ManiTransferRecvStreams::UnreliableOnly { .. } => "UnreliableOnly",
    }
}

async fn play_unreliable(unreliable: TransferUnreliableRecvStream, player: &Player) -> Result<()> {
    let mut jitter = OpusJitterBuffer::new(
        unreliable,
        SAMPLE_RATE,
        opus::Channels::Stereo,
        FRAME_MS,
        PLAYOUT_DELAY_MS,
    )
    .context("create jitter buffer")?;

    loop {
        player.wait_until_below(MAX_QUEUED_FRAMES).await;
        match jitter.yield_pcm().await {
            Ok(Some(pcm)) => player.push_pcm(pcm),
            Ok(None) => break,
            Err(e) => {
                tracing::warn!(error = %e, "jitter decode error");
                break;
            }
        }
    }
    Ok(())
}

async fn play_reliable(reliable: TransferReliableRecvStream, player: &Player) -> Result<()> {
    let mut decoder = OpusDecoderStream::new(reliable, SAMPLE_RATE, opus::Channels::Stereo)
        .context("create opus decoder")?;

    while let Some(chunks) = decoder.recv().await? {
        for chunk in chunks {
            player.wait_until_below(MAX_QUEUED_FRAMES).await;
            let pcm: Vec<f32> = chunk.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
            player.push_pcm(pcm);
        }
    }
    Ok(())
}

async fn drain_unreliable(mut unreliable: TransferUnreliableRecvStream) {
    while unreliable.recv().await.is_some() {}
}
