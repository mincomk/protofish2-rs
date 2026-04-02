use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use quinn::ConnectionError;
use tokio::sync::mpsc::Sender;
use tokio::time::Instant;

use crate::{
    ManiStreamId,
    datagram::chunk::{Chunk, parse_chunk},
};

#[derive(Clone)]
pub struct DatagramChunkRouter {
    senders: Arc<DashMap<ManiStreamId, Sender<Chunk>>>,
    pending_chunks: Arc<DashMap<ManiStreamId, Vec<(Instant, Chunk)>>>,
    pub max_chunk_buffer_size: usize,
    pub pending_chunk_timeout: Duration,
    pub pending_chunk_cleanup_interval: Duration,
}

impl DatagramChunkRouter {
    pub fn new(
        max_chunk_buffer_size: usize,
        pending_chunk_timeout: Duration,
        pending_chunk_cleanup_interval: Duration,
    ) -> Self {
        Self {
            senders: Arc::new(DashMap::new()),
            pending_chunks: Arc::new(DashMap::new()),
            max_chunk_buffer_size,
            pending_chunk_timeout,
            pending_chunk_cleanup_interval,
        }
    }

    pub fn register(&mut self, stream_id: ManiStreamId, sender: Sender<Chunk>) {
        self.senders.insert(stream_id, sender.clone());

        // Flush any pending chunks for this stream
        if let Some((_, chunks)) = self.pending_chunks.remove(&stream_id) {
            tokio::spawn(async move {
                for (_, chunk) in chunks {
                    if sender.send(chunk).await.is_err() {
                        break;
                    }
                }
            });
        }
    }

    pub async fn run(&self, quic: quinn::Connection) {
        let mut cleanup_interval = tokio::time::interval(self.pending_chunk_cleanup_interval);

        loop {
            tokio::select! {
                _ = cleanup_interval.tick() => {
                    self.cleanup_pending_chunks();
                }
                datagram_res = quic.read_datagram() => {
                    match datagram_res {
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
            tracing::trace!(
                "Received chunk for unregistered stream {} with sequence number {}, buffering it",
                stream_id,
                chunk.sequence_number
            );
            let mut entry = self
                .pending_chunks
                .entry(stream_id)
                .or_insert_with(Vec::new);
            if entry.len() < self.max_chunk_buffer_size {
                entry.push((Instant::now(), chunk));
            } else {
                tracing::warn!(
                    "Pending chunk buffer full for stream {}, dropping sequence number {}",
                    stream_id,
                    chunk.sequence_number
                );
            }
        }

        for stream_id in remove_senders {
            self.senders.remove(&stream_id);
        }
    }

    fn cleanup_pending_chunks(&self) {
        let now = Instant::now();
        self.pending_chunks.retain(|_, chunks| {
            chunks.retain(|(timestamp, _)| {
                now.duration_since(*timestamp) < self.pending_chunk_timeout
            });
            !chunks.is_empty()
        });
    }
}
