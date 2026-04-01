use std::collections::BTreeMap;

use bytes::Bytes;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::{
    ManiStreamId, SequenceNumber, Timestamp,
    compression::Compression,
    datagram::chunk::{Chunk, serialize_chunk},
};

#[derive(Error, Debug, Clone)]
pub enum TransferSendError {
    #[error("retransmission buffer is full")]
    RetransmissionBufferFull,

    #[error("failed to send datagram: {0}")]
    DatagramSendFailed(String),

    #[error("compression failed")]
    CompressionFailed,
}

pub(crate) enum TransferSendCommand {
    EndTransfer {
        final_sequence_number: SequenceNumber,
        response: oneshot::Sender<Result<(), TransferSendError>>,
    },
    SendTransferEndAck,
}

pub struct TransferSendStream {
    id: ManiStreamId,
    compression: Box<dyn Compression>,
    quic_connection: quinn::Connection,
    sequence_counter: SequenceNumber,
    retransmission_buffer: BTreeMap<SequenceNumber, Chunk>,
    max_retransmission_buffer_size: usize,
    command_sender: Option<mpsc::Sender<TransferSendCommand>>,
}

impl TransferSendStream {
    pub(crate) fn new(
        id: ManiStreamId,
        compression: Box<dyn Compression>,
        quic_connection: quinn::Connection,
        initial_sequence_number: SequenceNumber,
        max_retransmission_buffer_size: usize,
        command_sender: mpsc::Sender<TransferSendCommand>,
    ) -> Self {
        Self {
            id,
            compression,
            quic_connection,
            sequence_counter: initial_sequence_number,
            retransmission_buffer: BTreeMap::new(),
            max_retransmission_buffer_size,
            command_sender: Some(command_sender),
        }
    }

    pub async fn send(
        &mut self,
        timestamp: Timestamp,
        content: Bytes,
    ) -> Result<(), TransferSendError> {
        if self.retransmission_buffer.len() >= self.max_retransmission_buffer_size {
            return Err(TransferSendError::RetransmissionBufferFull);
        }

        let compressed_content = self.compression.compress(&content);

        let chunk = Chunk {
            stream_id: self.id,
            sequence_number: self.sequence_counter,
            timestamp,
            content: Bytes::from(compressed_content),
        };

        let serialized = serialize_chunk(&chunk);

        self.quic_connection
            .send_datagram(serialized)
            .map_err(|e| TransferSendError::DatagramSendFailed(e.to_string()))?;

        self.retransmission_buffer
            .insert(self.sequence_counter, chunk);

        self.sequence_counter = SequenceNumber(self.sequence_counter.0.wrapping_add(1));

        Ok(())
    }

    pub fn current_sequence_number(&self) -> SequenceNumber {
        self.sequence_counter
    }

    pub async fn end(&mut self) -> Result<(), TransferSendError> {
        let final_sequence_number = SequenceNumber(self.sequence_counter.0.wrapping_sub(1));

        if let Some(command_sender) = &self.command_sender {
            let (response_tx, response_rx) = oneshot::channel();

            command_sender
                .send(TransferSendCommand::EndTransfer {
                    final_sequence_number,
                    response: response_tx,
                })
                .await
                .map_err(|_| {
                    TransferSendError::DatagramSendFailed(
                        "Failed to send end transfer command".to_string(),
                    )
                })?;

            response_rx.await.map_err(|_| {
                TransferSendError::DatagramSendFailed(
                    "Failed to receive end transfer response".to_string(),
                )
            })?
        } else {
            Err(TransferSendError::DatagramSendFailed(
                "Command sender not available".to_string(),
            ))
        }
    }

    #[allow(dead_code)]
    pub(crate) fn get_chunk_for_retransmission(
        &self,
        sequence_number: SequenceNumber,
    ) -> Option<&Chunk> {
        self.retransmission_buffer.get(&sequence_number)
    }
}
