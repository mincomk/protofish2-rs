use crate::{
    ManiStreamId, SequenceNumber, datagram::chunk::Chunk, mani::transfer::assembler::Assembler,
};
use tokio::sync::{
    mpsc::{Receiver, Sender},
    oneshot,
};

pub(crate) enum RecvPipelineCommand {
    EndTransfer {
        final_sequence_number: SequenceNumber,
        reply: oneshot::Sender<()>,
    },
}

pub struct TransferReliableRecvStream {
    pub id: ManiStreamId,

    receiver: Receiver<Chunk>,
    nack_sender: Sender<Vec<SequenceNumber>>,
    assembler: Assembler,

    command_receiver: Receiver<RecvPipelineCommand>,
    pending_end: Option<(SequenceNumber, oneshot::Sender<()>)>,
}

pub struct TransferUnreliableRecvStream {
    pub id: ManiStreamId,

    receiver: Receiver<Chunk>,
}

impl TransferReliableRecvStream {
    pub(crate) fn new(
        id: ManiStreamId,
        receiver: Receiver<Chunk>,
        nack_sender: Sender<Vec<SequenceNumber>>,
        max_retransmission_buffer_size: usize,
        command_receiver: Receiver<RecvPipelineCommand>,
    ) -> Self {
        Self {
            id,
            receiver,
            nack_sender,
            assembler: Assembler::new(max_retransmission_buffer_size),
            command_receiver,
            pending_end: None,
        }
    }

    pub async fn recv(&mut self) -> Option<Vec<Chunk>> {
        loop {
            #[allow(clippy::collapsible_if)]
            if let Some((final_seq, _)) = &self.pending_end {
                if self.assembler.cursor() > *final_seq {
                    let (_, reply) = self.pending_end.take().unwrap();
                    let _ = reply.send(());
                    return None; // Signal EOF
                }
            }

            tokio::select! {
                Some(cmd) = self.command_receiver.recv() => {
                    match cmd {
                        RecvPipelineCommand::EndTransfer { final_sequence_number, reply } => {
                            self.pending_end = Some((final_sequence_number, reply));
                        }
                    }
                }
                chunk_opt = self.receiver.recv() => {
                    let chunk = match chunk_opt {
                        Some(c) => c,
                        None => return None,
                    };
                    let sequence_number = chunk.sequence_number;
                    if let Err(err) = self.assembler.push(chunk.sequence_number, chunk) {
                        tracing::error!(
                            "Failed to push chunk with sequence number {} to assembler: {}",
                            sequence_number,
                            err
                        );
                    }

                    let missings = self.assembler.missing_sequence_numbers();
                    #[allow(clippy::collapsible_if)]
                    if !missings.is_empty() {
                        if let Err(err) = self.nack_sender.send(missings).await {
                            tracing::trace!("Failed to send NACK for missing sequence numbers: {}", err);
                        }
                    }

                    let chunks = self.assembler.read_ordered();
                    if !chunks.is_empty() {
                        return Some(chunks);
                    }
                }
            }
        }
    }
}

impl TransferUnreliableRecvStream {
    pub(crate) fn new(id: ManiStreamId, receiver: Receiver<Chunk>) -> Self {
        Self { id, receiver }
    }

    pub async fn recv(&mut self) -> Option<Chunk> {
        self.receiver.recv().await
    }
}

pub(crate) fn create_stream_pair(
    id: ManiStreamId,
    receiver1: Receiver<Chunk>,
    receiver2: Receiver<Chunk>,
    nack_sender: Sender<Vec<SequenceNumber>>,
    max_retransmission_buffer_size: usize,
    command_receiver: Receiver<RecvPipelineCommand>,
) -> (TransferReliableRecvStream, TransferUnreliableRecvStream) {
    let reliable_stream = TransferReliableRecvStream::new(
        id,
        receiver1,
        nack_sender,
        max_retransmission_buffer_size,
        command_receiver,
    );
    let unreliable_stream = TransferUnreliableRecvStream::new(id, receiver2);

    (reliable_stream, unreliable_stream)
}
