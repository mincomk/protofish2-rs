use std::sync::Arc;

use dashmap::DashMap;
use quinn::ConnectionError;
use tokio::sync::mpsc::Sender;

use crate::{
    ManiStreamId,
    datagram::chunk::{Chunk, parse_chunk},
};

#[derive(Clone)]
pub struct DatagramChunkRouter {
    senders: Arc<DashMap<ManiStreamId, Sender<Chunk>>>,
    pub max_chunk_buffer_size: usize,
}

impl DatagramChunkRouter {
    pub fn new(max_chunk_buffer_size: usize) -> Self {
        Self {
            senders: Arc::new(DashMap::new()),
            max_chunk_buffer_size,
        }
    }

    pub fn register(&mut self, stream_id: ManiStreamId, sender: Sender<Chunk>) {
        self.senders.insert(stream_id, sender);
    }

    pub async fn run(&self, quic: quinn::Connection) {
        loop {
            match quic.read_datagram().await {
                Ok(datagram) => {
                    tracing::trace!("Received datagram with length {}", datagram.len());

                    if let Ok(chunk) = parse_chunk(datagram) {
                        self.route(chunk.stream_id, chunk);
                    } else {
                        tracing::warn!("Failed to parse datagram into chunk");
                    }
                }
                Err(ConnectionError::ApplicationClosed(_)) => {
                    tracing::debug!("Connection closed by peer");
                    break;
                }
                Err(err) => {
                    tracing::warn!("Failed to read datagram: {}", err);
                    break;
                }
            }
        }
    }

    fn route(&self, stream_id: ManiStreamId, chunk: Chunk) {
        let mut remove_senders = Vec::new();
        if let Some(sender) = self.senders.get(&stream_id) {
            let sequence_number = chunk.sequence_number;
            if let Err(err) = sender.try_send(chunk) {
                remove_senders.push(stream_id);

                tracing::error!(
                    "Tried to send chunk with sequence number {} to stream {}, to a closed stream: {}",
                    sequence_number,
                    stream_id,
                    err
                );
            }
        } else {
            tracing::warn!(
                "Received chunk for unregistered stream {} with sequence number {}",
                stream_id,
                chunk.sequence_number
            );
        }

        for stream_id in remove_senders {
            self.senders.remove(&stream_id);
        }
    }
}
