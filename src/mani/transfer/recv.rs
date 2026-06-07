use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    Chunk, ManiStreamId, SequenceNumber, datagram::packet::Packet,
    mani::transfer::assembler::Assembler,
};
use tokio::sync::{
    Mutex, Notify,
    mpsc::{Receiver, Sender},
    oneshot,
};

pub(crate) enum RecvPipelineCommand {
    EndTransfer {
        final_sequence_number: SequenceNumber,
        reply: oneshot::Sender<()>,
    },
}

#[derive(Debug)]
pub(crate) enum RecvSenderCommand {
    UpdateCredits {
        additional_credits: usize,
    },
    Nack {
        sequence_numbers: Vec<SequenceNumber>,
    },
}

/// Coordinates credit return so credits track *application drain*.
///
/// In Dual mode credits advance based on `min(reliable_drained,
/// unreliable_drained)`: a credit is only returned to the sender once
/// **both** sinks have surfaced the corresponding packet to the user. When
/// one sink closes, the coordinator stops gating credits on it and tracks
/// only the other.
///
/// In UnreliableOnly mode (`has_reliable == false`) only the unreliable
/// counter is consulted.
pub(crate) struct CreditCoordinator {
    sender_command_sender: Sender<RecvSenderCommand>,
    bulk: usize,
    state: Mutex<CoordState>,
}

struct CoordState {
    reliable_drained: u64,
    unreliable_drained: u64,
    last_emitted: u64,
    has_reliable: bool,
    reliable_closed: bool,
    unreliable_closed: bool,
}

impl CreditCoordinator {
    pub(crate) fn new(
        sender_command_sender: Sender<RecvSenderCommand>,
        bulk: usize,
        has_reliable: bool,
    ) -> Self {
        Self {
            sender_command_sender,
            bulk: bulk.max(1),
            state: Mutex::new(CoordState {
                reliable_drained: 0,
                unreliable_drained: 0,
                last_emitted: 0,
                has_reliable,
                reliable_closed: false,
                unreliable_closed: false,
            }),
        }
    }

    pub(super) async fn record_reliable(&self, n: usize) {
        if n == 0 {
            return;
        }
        let mut s = self.state.lock().await;
        s.reliable_drained = s.reliable_drained.saturating_add(n as u64);
        self.maybe_emit(&mut s, false).await;
    }

    pub(super) async fn record_unreliable(&self, n: usize) {
        if n == 0 {
            return;
        }
        let mut s = self.state.lock().await;
        s.unreliable_drained = s.unreliable_drained.saturating_add(n as u64);
        self.maybe_emit(&mut s, false).await;
    }

    pub(super) async fn mark_reliable_closed(&self) {
        let mut s = self.state.lock().await;
        if s.reliable_closed {
            return;
        }
        s.reliable_closed = true;
        self.maybe_emit(&mut s, true).await;
    }

    pub(super) async fn mark_unreliable_closed(&self) {
        let mut s = self.state.lock().await;
        if s.unreliable_closed {
            return;
        }
        s.unreliable_closed = true;
        self.maybe_emit(&mut s, true).await;
    }

    async fn maybe_emit(&self, s: &mut CoordState, force: bool) {
        let Some(effective) = effective_count(s) else {
            return;
        };
        if effective <= s.last_emitted {
            return;
        }
        let pending = effective - s.last_emitted;
        if !force && (pending as usize) < self.bulk {
            return;
        }
        s.last_emitted = effective;
        let cmd = RecvSenderCommand::UpdateCredits {
            additional_credits: pending as usize,
        };
        if let Err(err) = self.sender_command_sender.send(cmd).await {
            tracing::trace!("CreditCoordinator: failed to send UpdateCredits: {}", err);
        }
    }
}

fn effective_count(s: &CoordState) -> Option<u64> {
    let r_active = s.has_reliable && !s.reliable_closed;
    let u_active = !s.unreliable_closed;
    match (r_active, u_active) {
        (true, true) => Some(s.reliable_drained.min(s.unreliable_drained)),
        (true, false) => Some(s.reliable_drained),
        (false, true) => Some(s.unreliable_drained),
        (false, false) => None,
    }
}

pub struct TransferReliableRecvStream {
    pub id: ManiStreamId,

    receiver: Option<Receiver<Packet>>,
    assembler: Assembler,

    end_receiver: Arc<Notify>,
    sender_command_sender: Sender<RecvSenderCommand>,
    command_receiver: Receiver<RecvPipelineCommand>,
    pending_end: Option<(SequenceNumber, oneshot::Sender<()>)>,
    credit_coord: Arc<CreditCoordinator>,
    closed_signaled: bool,
}

impl Drop for TransferReliableRecvStream {
    fn drop(&mut self) {
        let coord = self.credit_coord.clone();
        let already = self.closed_signaled;
        if let Some(mut receiver) = self.receiver.take() {
            tracing::debug!(
                stream_id = self.id.0,
                "TransferReliableRecvStream dropped without consuming all data; draining channel"
            );
            tokio::spawn(async move {
                while receiver.recv().await.is_some() {}
                if !already {
                    coord.mark_reliable_closed().await;
                }
            });
        } else if !already {
            tokio::spawn(async move { coord.mark_reliable_closed().await });
        }
    }
}

impl TransferReliableRecvStream {
    pub(crate) fn new(
        id: ManiStreamId,
        receiver: Receiver<Packet>,
        max_retransmission_buffer_size: usize,
        end_receiver: Arc<Notify>,
        command_receiver: Receiver<RecvPipelineCommand>,
        sender_command_sender: Sender<RecvSenderCommand>,
        credit_coord: Arc<CreditCoordinator>,
    ) -> Self {
        Self {
            id,
            receiver: Some(receiver),
            assembler: Assembler::new(max_retransmission_buffer_size),
            end_receiver,
            sender_command_sender,
            command_receiver,
            pending_end: None,
            credit_coord,
            closed_signaled: false,
        }
    }

    async fn signal_eof(&mut self) {
        if !self.closed_signaled {
            self.closed_signaled = true;
            self.credit_coord.mark_reliable_closed().await;
        }
    }

    pub async fn recv(&mut self) -> Option<Vec<Chunk>> {
        loop {
            #[allow(clippy::collapsible_if)]
            if let Some((final_seq, _)) = &self.pending_end {
                if self.assembler.cursor() > *final_seq {
                    let (_, reply) = self.pending_end.take().unwrap();
                    let _ = reply.send(());
                    self.signal_eof().await;
                    return None; // Signal EOF
                }
            }

            tokio::select! {
                _ = self.end_receiver.notified() => {
                    self.signal_eof().await;
                    return None; // Signal EOF
                }

                Some(cmd) = self.command_receiver.recv() => {
                    match cmd {
                        RecvPipelineCommand::EndTransfer { final_sequence_number, reply } => {
                            self.pending_end = Some((final_sequence_number, reply));
                        }
                    }
                }
                packet_opt = self.receiver.as_mut().expect("receiver already taken").recv() => {
                    let packet = match packet_opt {
                        Some(c) => c,
                        None => {
                            self.signal_eof().await;
                            return None;
                        }
                    };
                    let sequence_number = packet.sequence_number;
                    if let Err(err) = self.assembler.push(packet.sequence_number, packet) {
                        tracing::error!(
                            "Failed to push packet with sequence number {} to assembler: {}",
                            sequence_number,
                            err
                        );
                    }

                    let missings = self.assembler.missing_sequence_numbers();
                    #[allow(clippy::collapsible_if)]
                    if !missings.is_empty() {
                        let command = RecvSenderCommand::Nack { sequence_numbers: missings };
                        if let Err(err) = self.sender_command_sender.send(command).await {
                            tracing::trace!("Failed to send NACK for missing sequence numbers: {}", err);
                        }
                    }

                    let packets = self.assembler.read_ordered();
                    if !packets.is_empty() {
                        self.credit_coord.record_reliable(packets.len()).await;
                        return Some(packets.into_iter().map(Into::into).collect());
                    }
                }
            }
        }
    }
}

pub struct TransferUnreliableRecvStream {
    pub id: ManiStreamId,

    end_receiver: Arc<Notify>,
    is_end: Arc<AtomicBool>,
    receiver: Option<Receiver<Packet>>,
    credit_coord: Arc<CreditCoordinator>,
    closed_signaled: bool,
}

impl Drop for TransferUnreliableRecvStream {
    fn drop(&mut self) {
        let coord = self.credit_coord.clone();
        let already = self.closed_signaled;
        if let Some(mut receiver) = self.receiver.take() {
            tracing::debug!(
                stream_id = self.id.0,
                "TransferUnreliableRecvStream dropped without consuming all data; draining channel"
            );
            tokio::spawn(async move {
                while receiver.recv().await.is_some() {}
                if !already {
                    coord.mark_unreliable_closed().await;
                }
            });
        } else if !already {
            tokio::spawn(async move { coord.mark_unreliable_closed().await });
        }
    }
}

impl TransferUnreliableRecvStream {
    pub(crate) async fn new(
        id: ManiStreamId,
        is_end: Arc<AtomicBool>,
        receiver: Receiver<Packet>,
        end_receiver: Arc<Notify>,
        credit_coord: Arc<CreditCoordinator>,
    ) -> Self {
        Self {
            id,
            receiver: Some(receiver),
            is_end,
            end_receiver,
            credit_coord,
            closed_signaled: false,
        }
    }

    async fn signal_eof(&mut self) {
        if !self.closed_signaled {
            self.closed_signaled = true;
            self.credit_coord.mark_unreliable_closed().await;
        }
    }

    pub async fn recv(&mut self) -> Option<Chunk> {
        loop {
            if self.is_end.load(std::sync::atomic::Ordering::SeqCst)
                && self.receiver.as_ref().map_or(true, |r| r.is_empty())
            {
                self.signal_eof().await;
                return None; // Signal EOF
            }

            tokio::select! {
                _ = self.end_receiver.notified() => {
                    if self.is_end.load(Ordering::SeqCst)
                        && self.receiver.as_ref().map_or(true, |r| r.is_empty())
                    {
                        self.signal_eof().await;
                        return None; // Signal EOF
                    }
                }
                packet_opt = self.receiver.as_mut().expect("receiver already taken").recv() => {
                    let packet = match packet_opt {
                        Some(c) => c,
                        None => {
                            self.signal_eof().await;
                            return None;
                        }
                    };
                    self.credit_coord.record_unreliable(1).await;
                    return Some(packet.into());
                }
            }
        }
    }
}

pub(crate) async fn create_stream_pair(
    id: ManiStreamId,
    receiver1: Receiver<Packet>,
    receiver2: Receiver<Packet>,
    end_receiver: Arc<Notify>,
    is_end: Arc<AtomicBool>,
    sender_command_sender: Sender<RecvSenderCommand>,
    max_retransmission_buffer_size: usize,
    command_receiver: Receiver<RecvPipelineCommand>,
    credit_coord: Arc<CreditCoordinator>,
) -> (TransferReliableRecvStream, TransferUnreliableRecvStream) {
    let reliable_stream = TransferReliableRecvStream::new(
        id,
        receiver1,
        max_retransmission_buffer_size,
        end_receiver.clone(),
        command_receiver,
        sender_command_sender.clone(),
        credit_coord.clone(),
    );
    let unreliable_stream =
        TransferUnreliableRecvStream::new(id, is_end, receiver2, end_receiver, credit_coord).await;

    (reliable_stream, unreliable_stream)
}
