use std::{collections::VecDeque, task::Waker};

use thiserror::Error;

use crate::{SequenceNumber, datagram::chunk::Chunk};

#[derive(Error, Debug, Clone)]
pub enum AssemblerError {
    #[error("retransmission buffer overflow")]
    RetransmissionBufferOverflow,
}

enum AssemblerChunk {
    Payload(Chunk),
    Missing,
}

pub struct Assembler {
    cursor: SequenceNumber,
    chunks: VecDeque<AssemblerChunk>,
    waker: Option<Waker>,

    max_retransmission_buffer_size: usize,
}

impl Assembler {
    pub fn new(max_retransmission_buffer_size: usize) -> Self {
        Self {
            cursor: 0.into(),
            chunks: VecDeque::with_capacity(max_retransmission_buffer_size),
            waker: None,

            max_retransmission_buffer_size,
        }
    }

    pub fn push(
        &mut self,
        sequence_number: SequenceNumber,
        payload: Chunk,
    ) -> Result<(), AssemblerError> {
        if self.chunks.len() >= self.max_retransmission_buffer_size {
            tracing::warn!(
                "Retransmission buffer overflow, dropping payload with sequence number {}",
                sequence_number
            );
            return Err(AssemblerError::RetransmissionBufferOverflow);
        }

        if let Some(index) = self.index(sequence_number) {
            if let AssemblerChunk::Missing = self.chunks[index] {
                self.chunks[index] = AssemblerChunk::Missing;
            }
        } else {
            let missing_chunks = sequence_number - self.cursor;
            for _ in 0..missing_chunks.into() {
                self.chunks.push_back(AssemblerChunk::Missing);
            }
            self.chunks.push_back(AssemblerChunk::Payload(payload));
        }

        if let Some(waker) = self.waker.take() {
            waker.wake();
        }

        Ok(())
    }

    pub fn read_ordered(&mut self) -> Vec<Chunk> {
        let mut payloads = Vec::new();
        while let Some(chunk) = self.chunks.front() {
            match chunk {
                AssemblerChunk::Payload(payload) => {
                    payloads.push(payload.clone());
                    self.chunks.pop_front();
                    self.cursor = self.cursor + 1.into();
                }
                AssemblerChunk::Missing => break,
            }
        }
        payloads
    }

    pub fn missing_sequence_numbers(&self) -> Vec<SequenceNumber> {
        self.chunks
            .iter()
            .enumerate()
            .filter_map(|(index, chunk)| {
                if let AssemblerChunk::Missing = chunk {
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
        if index >= self.chunks.len() {
            return None;
        }
        Some(index)
    }
}
