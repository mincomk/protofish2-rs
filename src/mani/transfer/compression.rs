use bytes::Bytes;
use tokio::sync::mpsc;

use crate::{compression::Compression, datagram::packet::Packet};

pub struct CompressedPacketReceiver {
    receiver: mpsc::Receiver<Packet>,
    senders: Vec<mpsc::Sender<Packet>>,
    compression: Box<dyn Compression>,
}

impl CompressedPacketReceiver {
    pub fn new(
        receiver: mpsc::Receiver<Packet>,
        senders: Vec<mpsc::Sender<Packet>>,
        compression: Box<dyn Compression>,
    ) -> Self {
        Self {
            receiver,
            senders,
            compression,
        }
    }

    pub async fn run(&mut self) {
        while let Some(packet) = self.receiver.recv().await {
            let compressed_packet = self.compression.decompress(&packet.content);
            for sender in &self.senders {
                if let Err(err) = sender
                    .send(Packet {
                        stream_id: packet.stream_id,
                        sequence_number: packet.sequence_number,
                        timestamp: packet.timestamp,
                        content: Bytes::from(compressed_packet.clone()),
                    })
                    .await
                {
                    tracing::debug!("Failed to send compressed packet: {}", err);
                }
            }
        }
    }
}
