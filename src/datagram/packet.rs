use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::io::Cursor;

use crate::{ManiStreamId, SequenceNumber, Timestamp};

/// Size of the fixed wire header prepended to every datagram:
/// `stream_id` (u64) + `sequence_number` (u32) + `timestamp` (u64) = 20 bytes.
pub const PACKET_WIRE_HEADER_SIZE: usize = 20;

/// Overhead added to the content field of a non-fragmented packet: 1 flag byte.
pub const SINGLE_CONTENT_OVERHEAD: usize = 1;

/// Overhead added to the content field of each fragment:
/// 1 flag byte + u16 frag_index + u16 total_frags = 5 bytes.
///
/// All fragments of the same logical chunk share the same `sequence_number` in
/// the packet wire header, so no additional chunk-key is needed here.
pub const FRAG_CONTENT_OVERHEAD: usize = 5;

#[derive(Debug, Clone)]
pub struct Packet {
    pub stream_id: ManiStreamId,
    pub sequence_number: SequenceNumber,
    pub timestamp: Timestamp,
    pub content: Bytes,
}

pub fn serialize_packet(packet: &Packet) -> Bytes {
    let mut buf = BytesMut::new();
    buf.put_u64(packet.stream_id.0);
    buf.put_u32(packet.sequence_number.0);
    buf.put_u64(packet.timestamp.0);
    buf.put_slice(&packet.content);
    buf.freeze()
}

pub fn parse_packet(bytes: Bytes) -> Result<Packet, PacketParseError> {
    let mut cur = Cursor::new(bytes);

    let stream_id = ManiStreamId(cur.get_u64());
    let sequence_number = SequenceNumber(cur.get_u32());
    let timestamp = Timestamp(cur.get_u64());

    let remaining = cur.remaining();
    let content = cur.copy_to_bytes(remaining);

    Ok(Packet {
        stream_id,
        sequence_number,
        timestamp,
        content,
    })
}

/// The decoded content of a packet after stripping the fragmentation header.
///
/// All fragments that belong to the same logical chunk carry the **same**
/// `sequence_number` in the outer packet header.  The `frag_index` /
/// `total_frags` fields within the fragment content are used to reassemble
/// them.
#[derive(Debug, Clone)]
pub enum DecodedContent {
    /// The entire compressed payload is contained in this single packet.
    Single(Bytes),
    /// This packet carries one fragment of a larger compressed payload.
    Fragment {
        /// 0-based index of this fragment within the chunk.
        frag_index: u16,
        /// Total number of fragments for this chunk.
        total_frags: u16,
        /// The compressed bytes carried by this fragment.
        payload: Bytes,
    },
}

/// Encodes a complete (non-fragmented) compressed payload into the on-wire content format.
///
/// Prepends a `0x00` flag byte indicating a single-packet payload.
pub fn encode_single_content(compressed: Bytes) -> Bytes {
    let mut buf = BytesMut::with_capacity(1 + compressed.len());
    buf.put_u8(0x00);
    buf.put_slice(&compressed);
    buf.freeze()
}

/// Encodes one fragment of a fragmented compressed payload into the on-wire content format.
///
/// Prepends a `0x01` flag byte followed by `frag_index` (u16) and `total_frags` (u16).
/// All fragments of the same logical chunk must be sent with the **same** `sequence_number`
/// in the outer packet header so that the receiver can group them without an additional key.
pub fn encode_fragment_content(frag_index: u16, total_frags: u16, payload: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(FRAG_CONTENT_OVERHEAD + payload.len());
    buf.put_u8(0x01);
    buf.put_u16(frag_index);
    buf.put_u16(total_frags);
    buf.put_slice(payload);
    buf.freeze()
}

/// Decodes the fragmentation header from the `content` field of a received packet.
pub fn decode_content(mut bytes: Bytes) -> Result<DecodedContent, PacketParseError> {
    if bytes.is_empty() {
        return Err(PacketParseError::UnexpectedEndOfData);
    }
    match bytes.get_u8() {
        0x00 => Ok(DecodedContent::Single(bytes)),
        0x01 => {
            if bytes.remaining() < 4 {
                return Err(PacketParseError::UnexpectedEndOfData);
            }
            let frag_index = bytes.get_u16();
            let total_frags = bytes.get_u16();
            Ok(DecodedContent::Fragment {
                frag_index,
                total_frags,
                payload: bytes,
            })
        }
        _ => Err(PacketParseError::UnexpectedEndOfData),
    }
}

#[derive(thiserror::Error, Debug, Clone)]
pub enum PacketParseError {
    #[error("Unexpected end of data")]
    #[allow(dead_code)]
    UnexpectedEndOfData,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_single() {
        let original = Bytes::from_static(b"hello compressed");
        let encoded = encode_single_content(original.clone());
        match decode_content(encoded).unwrap() {
            DecodedContent::Single(payload) => assert_eq!(payload, original),
            _ => panic!("expected Single"),
        }
    }

    #[test]
    fn test_encode_decode_fragment() {
        let payload = b"fragment bytes";
        let encoded = encode_fragment_content(2, 5, payload);
        match decode_content(encoded).unwrap() {
            DecodedContent::Fragment {
                frag_index,
                total_frags,
                payload: p,
            } => {
                assert_eq!(frag_index, 2);
                assert_eq!(total_frags, 5);
                assert_eq!(p.as_ref(), payload);
            }
            _ => panic!("expected Fragment"),
        }
    }

    #[test]
    fn test_empty_content_errors() {
        assert!(decode_content(Bytes::new()).is_err());
    }
}
