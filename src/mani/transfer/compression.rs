use bytes::Bytes;
use tokio::sync::mpsc;

use crate::{compression::Compression, datagram::chunk::Chunk};

pub struct CompressedChunkReceiver {
    receiver: mpsc::Receiver<Chunk>,
    senders: Vec<mpsc::Sender<Chunk>>,
    compression: Box<dyn Compression>,
}

impl CompressedChunkReceiver {
    pub fn new(
        receiver: mpsc::Receiver<Chunk>,
        senders: Vec<mpsc::Sender<Chunk>>,
        compression: Box<dyn Compression>,
    ) -> Self {
        Self {
            receiver,
            senders,
            compression,
        }
    }

    pub async fn run(&mut self) {
        while let Some(chunk) = self.receiver.recv().await {
            let compressed_chunk = self.compression.decompress(&chunk.content);
            for sender in &self.senders {
                if let Err(err) = sender
                    .send(Chunk {
                        stream_id: chunk.stream_id,
                        sequence_number: chunk.sequence_number,
                        timestamp: chunk.timestamp,
                        content: Bytes::from(compressed_chunk.clone()),
                    })
                    .await
                {
                    tracing::debug!("Failed to send compressed chunk: {}", err);
                }
            }
        }
    }
}
