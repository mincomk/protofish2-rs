use bytes::{BufMut, Bytes, BytesMut};
use std::collections::HashMap;
use tokio::sync::mpsc;

use crate::{
    Timestamp,
    compression::Compression,
    datagram::packet::{DecodedContent, Packet, decode_content},
    mani::transfer::recv::RecvSenderCommand,
};

/// Buffers in-flight fragments for a single logical chunk.
struct FragmentBuffer {
    total_frags: u16,
    fragments: HashMap<u16, Bytes>,
    timestamp: Timestamp,
}

pub struct CompressedPacketReceiver {
    receiver: mpsc::Receiver<Packet>,
    senders: Vec<mpsc::Sender<Packet>>,
    compression: Box<dyn Compression>,
    sender_command_sender: mpsc::Sender<RecvSenderCommand>,
    credits_bulk_update_count: usize,
    updated_credits: usize,
    /// Incomplete fragment groups keyed by `chunk_seq` (sequence number of first fragment).
    fragment_buffers: HashMap<u32, FragmentBuffer>,
    /// Maximum number of concurrent incomplete fragment groups before evicting the oldest.
    max_fragment_groups: usize,
}

impl CompressedPacketReceiver {
    pub fn new(
        receiver: mpsc::Receiver<Packet>,
        senders: Vec<mpsc::Sender<Packet>>,
        compression: Box<dyn Compression>,
        sender_command_sender: mpsc::Sender<RecvSenderCommand>,
        credits_bulk_update_count: usize,
        max_fragment_groups: usize,
    ) -> Self {
        Self {
            receiver,
            senders,
            compression,
            sender_command_sender,
            credits_bulk_update_count,
            updated_credits: 0,
            fragment_buffers: HashMap::new(),
            max_fragment_groups,
        }
    }

    pub async fn run(&mut self) {
        while let Some(packet) = self.receiver.recv().await {
            // Always update credits per received packet (fragment or single).
            self.updated_credits += 1;
            if self.updated_credits >= self.credits_bulk_update_count {
                let command = RecvSenderCommand::UpdateCredits {
                    additional_credits: self.updated_credits,
                };
                if let Err(err) = self.sender_command_sender.send(command).await {
                    tracing::trace!("Failed to send credits update: {}", err);
                }
                self.updated_credits = 0;
            }

            match decode_content(packet.content.clone()) {
                Err(e) => {
                    tracing::warn!(
                        "Failed to decode packet content on stream {}: {}",
                        packet.stream_id.0,
                        e
                    );
                }
                Ok(DecodedContent::Single(compressed)) => {
                    let decompressed = self.compression.decompress(&compressed);
                    let stop = self
                        .route(packet.stream_id, packet.sequence_number, packet.timestamp, decompressed)
                        .await;
                    if stop {
                        break;
                    }
                }
                Ok(DecodedContent::Fragment {
                    frag_index,
                    total_frags,
                    payload,
                }) => {
                    // All fragments of a logical chunk share the same sequence_number.
                    let key = packet.sequence_number.0;

                    // Evict oldest incomplete group if we're at capacity.
                    if !self.fragment_buffers.contains_key(&key)
                        && self.fragment_buffers.len() >= self.max_fragment_groups
                    {
                        if let Some(oldest) = self.fragment_buffers.keys().copied().min() {
                            self.fragment_buffers.remove(&oldest);
                            tracing::debug!(
                                "Evicted incomplete fragment group seq={}",
                                oldest
                            );
                        }
                    }

                    // Insert this fragment (scope ends so borrow is released before remove).
                    {
                        let buf = self
                            .fragment_buffers
                            .entry(key)
                            .or_insert_with(|| FragmentBuffer {
                                total_frags,
                                fragments: HashMap::new(),
                                timestamp: packet.timestamp,
                            });
                        buf.fragments.insert(frag_index, payload);
                    }

                    // Check if all fragments have arrived.
                    let is_complete = self
                        .fragment_buffers
                        .get(&key)
                        .map(|b| b.fragments.len() == b.total_frags as usize)
                        .unwrap_or(false);

                    if is_complete {
                        // Remove and assemble.
                        if let Some(buf) = self.fragment_buffers.remove(&key) {
                            let mut assembled = BytesMut::new();
                            let mut ok = true;
                            for i in 0..buf.total_frags {
                                if let Some(frag) = buf.fragments.get(&i) {
                                    assembled.put_slice(frag);
                                } else {
                                    tracing::warn!(
                                        "Fragment {} missing for seq={}; dropping group",
                                        i,
                                        key
                                    );
                                    ok = false;
                                    break;
                                }
                            }
                            if ok {
                                let decompressed =
                                    self.compression.decompress(&assembled.freeze());
                                let stop = self
                                    .route(
                                        packet.stream_id,
                                        packet.sequence_number,
                                        buf.timestamp,
                                        decompressed,
                                    )
                                    .await;
                                if stop {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Routes a decompressed packet to all registered senders.
    /// Returns `true` if the receiver loop should stop (all senders dropped).
    async fn route(
        &self,
        stream_id: crate::ManiStreamId,
        sequence_number: crate::SequenceNumber,
        timestamp: Timestamp,
        decompressed: Vec<u8>,
    ) -> bool {
        let mut any_success = false;

        for sender in &self.senders {
            if sender
                .send(Packet {
                    stream_id,
                    sequence_number,
                    timestamp,
                    content: Bytes::from(decompressed.clone()),
                })
                .await
                .is_ok()
            {
                any_success = true;
            } else {
                tracing::debug!("Failed to send decompressed packet, receiver likely dropped");
            }
        }

        if !any_success && !self.senders.is_empty() {
            tracing::debug!(
                "All receivers dropped for stream {}, stopping CompressedPacketReceiver",
                stream_id.0
            );
            return true;
        }

        false
    }
}
