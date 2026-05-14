use crate::cert::{CERT_PATH, load_cert_chain};
use crate::cli::Mode;
use anyhow::{Context, Result};
use bytes::Bytes;
use ogg::PacketReader;
use protofish2::{
    SequenceNumber, Timestamp,
    compression::CompressionType,
    config::ProtofishConfig,
    connection::{ClientConfig, ProtofishClient},
};
use std::collections::HashMap;
use std::fs::File;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

const FRAME_MS: u64 = 20;

pub async fn run(addr: SocketAddr, mode: Mode, file: &Path) -> Result<()> {
    let root_certificates = load_cert_chain(CERT_PATH)?;

    let mut protofish_config = ProtofishConfig::default();
    protofish_config.mani_config.initial_backpressure_credits = 10;
    protofish_config.mani_config.backpressure_credit_batch_size = 1;

    let config = ClientConfig {
        bind_address: "0.0.0.0:0".parse().unwrap(),
        root_certificates,
        supported_compression_types: vec![CompressionType::None],
        keepalive_range: Duration::from_secs(1)..Duration::from_secs(10),
        protofish_config,
    };
    let client = ProtofishClient::bind(config).context("bind client")?;

    tracing::info!(%addr, "connecting");
    let conn = client
        .connect(addr, "localhost", HashMap::new())
        .await
        .context("connect")?;

    let mut stream = conn.open_mani().await.context("open mani")?;
    let mut transfer = stream
        .start_transfer(mode.into(), CompressionType::None, SequenceNumber(0), None)
        .await
        .context("start transfer")?;

    tracing::info!(path = %file.display(), "streaming ogg");
    let mut reader = PacketReader::new(File::open(file).context("open input file")?);
    let mut timestamp_ms: u64 = 0;
    let mut sent: u64 = 0;

    while let Some(packet) = reader.read_packet().context("read ogg packet")? {
        if packet.data.starts_with(b"OpusHead") || packet.data.starts_with(b"OpusTags") {
            continue;
        }
        transfer
            .send(Timestamp(timestamp_ms), Bytes::from(packet.data))
            .await
            .context("send packet")?;
        timestamp_ms += FRAME_MS;
        sent += 1;
        tokio::time::sleep(Duration::from_millis(1)).await;
    }

    tracing::info!(sent, "sent all frames; ending transfer");
    transfer.end().await.context("end transfer")?;
    Ok(())
}
