use std::{collections::VecDeque, task::Waker};

use thiserror::Error;

use crate::{datagram::packet::Packet, SequenceNumber};

#[derive(Error, Debug, Clone)]
pub enum AssemblerError {
    #[error("retransmission buffer overflow")]
    RetransmissionBufferOverflow,
}

enum AssemblerPacket {
    Payload(Packet),
    Missing,
}

pub struct Assembler {
    cursor: SequenceNumber,
    packets: VecDeque<AssemblerPacket>,
    waker: Option<Waker>,

    max_retransmission_buffer_size: usize,
}

impl Assembler {
    pub fn new(max_retransmission_buffer_size: usize) -> Self {
        Self {
            cursor: 0.into(),
            packets: VecDeque::with_capacity(max_retransmission_buffer_size),
            waker: None,

            max_retransmission_buffer_size,
        }
    }

    pub fn push(
        &mut self,
        sequence_number: SequenceNumber,
        payload: Packet,
    ) -> Result<(), AssemblerError> {
        if self.packets.len() >= self.max_retransmission_buffer_size {
            tracing::warn!(
                "Retransmission buffer overflow, dropping payload with sequence number {}",
                sequence_number
            );
            return Err(AssemblerError::RetransmissionBufferOverflow);
        }

        if let Some(index) = self.index(sequence_number) {
            if let AssemblerPacket::Missing = self.packets[index] {
                self.packets[index] = AssemblerPacket::Payload(payload);
            }
        } else {
            let missing_packets = sequence_number - self.cursor;
            for _ in 0..missing_packets.into() {
                self.packets.push_back(AssemblerPacket::Missing);
            }
            self.packets.push_back(AssemblerPacket::Payload(payload));
        }

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }

        Ok(())
    }

    pub fn read_ordered(&mut self) -> Vec<Packet> {
        let mut payloads = Vec::new();
        while let Some(packet) = self.packets.front() {
            match packet {
                AssemblerPacket::Payload(payload) => {
                    payloads.push(payload.clone());
                    self.packets.pop_front();
                    self.cursor = self.cursor + 1.into();
                }
                AssemblerPacket::Missing => break,
            }
        }
        payloads
    }

    pub fn missing_sequence_numbers(&self) -> Vec<SequenceNumber> {
        self.packets
            .iter()
            .enumerate()
            .filter_map(|(index, packet)| {
                if let AssemblerPacket::Missing = packet {
                    Some(self.cursor + SequenceNumber(index as u32))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn cursor(&self) -> SequenceNumber {
        self.cursor
    }

    #[inline]
    fn index(&self, sequence_number: SequenceNumber) -> Option<usize> {
        if sequence_number < self.cursor {
            tracing::trace!(
                "Received sequence number {} is less than cursor {}",
                sequence_number,
                self.cursor
            );
            return None;
        }
        let index = u32::from(sequence_number - self.cursor) as usize;
        if index >= self.packets.len() {
            return None;
        }
        Some(index)
    }
}
