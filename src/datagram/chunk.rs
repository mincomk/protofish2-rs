use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::io::Cursor;

use crate::{ManiStreamId, SequenceNumber, Timestamp};

#[derive(Debug, Clone)]
pub struct Chunk {
    pub stream_id: ManiStreamId,
    pub sequence_number: SequenceNumber,
    pub timestamp: Timestamp,
    pub content: Bytes,
}

pub fn serialize_chunk(chunk: &Chunk) -> Bytes {
    let mut buf = BytesMut::new();
    buf.put_u64(chunk.stream_id.0);
    buf.put_u32(chunk.sequence_number.0);
    buf.put_u64(chunk.timestamp.0);
    buf.put_slice(&chunk.content);
    buf.freeze()
}

pub fn parse_chunk(bytes: Bytes) -> Result<Chunk, ChunkParseError> {
    let mut cur = Cursor::new(bytes);

    let stream_id = ManiStreamId(cur.get_u64());
    let sequence_number = SequenceNumber(cur.get_u32());
    let timestamp = Timestamp(cur.get_u64());

    let remaining = cur.remaining();
    let content = cur.copy_to_bytes(remaining);

    Ok(Chunk {
        stream_id,
        sequence_number,
        timestamp,
        content,
    })
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum ChunkParseError {
    #[error("Unexpected end of data")]
    #[allow(dead_code)]
    UnexpectedEndOfData,
}
