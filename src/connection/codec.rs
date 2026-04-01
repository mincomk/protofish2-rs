use std::collections::HashMap;
use std::io::Cursor;

use bytes::{Buf, BufMut, Bytes, BytesMut, TryGetError};
use thiserror::Error;
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

use crate::compression::CompressionType;
use crate::connection::message::{
    ClientHello, ConnectionErrorMsg, ConnectionMessage, ConnectionMessageType, ServerHello,
};

#[derive(Error, Debug)]
pub enum ConnectionMessageParseError {
    #[error("Invalid message type: {0}")]
    InvalidMessageType(u8),

    #[error("Invalid message format")]
    InvalidMessageFormat,

    #[error("String decode error")]
    StringDecodeError,

    #[error("Unexpected end of message")]
    UnexpectedEndOfMessage(#[from] TryGetError),
}

fn try_get_string(cur: &mut Cursor<Bytes>) -> Result<String, ConnectionMessageParseError> {
    let len = cur.try_get_u16()? as usize;
    if cur.remaining() < len {
        return Err(TryGetError {
            requested: len,
            available: cur.remaining(),
        }
        .into());
    }
    let string_bytes = cur.copy_to_bytes(len);
    String::from_utf8(string_bytes.to_vec())
        .map_err(|_| ConnectionMessageParseError::StringDecodeError)
}

fn put_string(buf: &mut BytesMut, s: &str) {
    let bytes = s.as_bytes();
    buf.put_u16(bytes.len() as u16);
    buf.put_slice(bytes);
}

fn try_get_headers(
    cur: &mut Cursor<Bytes>,
) -> Result<HashMap<String, Bytes>, ConnectionMessageParseError> {
    let count = cur.try_get_u16()?;
    let mut map = HashMap::with_capacity(count as usize);
    for _ in 0..count {
        let key = try_get_string(cur)?;
        let val_len = cur.try_get_u16()? as usize;
        if cur.remaining() < val_len {
            return Err(TryGetError {
                requested: val_len,
                available: cur.remaining(),
            }
            .into());
        }
        let val = cur.copy_to_bytes(val_len);
        map.insert(key, val);
    }
    Ok(map)
}

fn put_headers(buf: &mut BytesMut, headers: &HashMap<String, Bytes>) {
    buf.put_u16(headers.len() as u16);
    for (k, v) in headers {
        put_string(buf, k);
        buf.put_u16(v.len() as u16);
        buf.put_slice(v);
    }
}

pub fn parse_connection_message(
    bytes: Bytes,
) -> Result<ConnectionMessage, ConnectionMessageParseError> {
    let mut cur = Cursor::new(bytes);

    let message_type = cur.try_get_u8()?;

    match ConnectionMessageType::from_u8(message_type).ok_or(
        ConnectionMessageParseError::InvalidMessageType(message_type),
    )? {
        ConnectionMessageType::ClientHello => {
            let version = cur.try_get_u16()?;
            let host = try_get_string(&mut cur)?;
            let headers = try_get_headers(&mut cur)?;
            let compressions_count = cur.try_get_u8()?;
            let mut available_compression_types = Vec::with_capacity(compressions_count as usize);
            for _ in 0..compressions_count {
                let ct = cur.try_get_u8()?;
                available_compression_types.push(
                    CompressionType::from_u8(ct)
                        .ok_or(ConnectionMessageParseError::InvalidMessageFormat)?,
                );
            }
            let keepalive_min_ms = cur.try_get_u32()?;
            let keepalive_max_ms = cur.try_get_u32()?;
            Ok(ConnectionMessage::ClientHello(ClientHello {
                version,
                host,
                headers,
                available_compression_types,
                keepalive_min_ms,
                keepalive_max_ms,
            }))
        }
        ConnectionMessageType::ServerHello => {
            let version = cur.try_get_u16()?;
            let headers = try_get_headers(&mut cur)?;
            let ct = cur.try_get_u8()?;
            let compression_type = CompressionType::from_u8(ct)
                .ok_or(ConnectionMessageParseError::InvalidMessageFormat)?;
            let keepalive_interval_ms = cur.try_get_u32()?;
            Ok(ConnectionMessage::ServerHello(ServerHello {
                version,
                headers,
                compression_type,
                keepalive_interval_ms,
            }))
        }
        ConnectionMessageType::Keepalive => Ok(ConnectionMessage::Keepalive),
        ConnectionMessageType::KeepaliveAck => Ok(ConnectionMessage::KeepaliveAck),
        ConnectionMessageType::Error => {
            let code = cur.try_get_u16()?;
            let message = try_get_string(&mut cur)?;
            let close = cur.try_get_u8()? != 0;
            Ok(ConnectionMessage::Error(ConnectionErrorMsg {
                code,
                message,
                close,
            }))
        }
        ConnectionMessageType::Close => Ok(ConnectionMessage::Close),
    }
}

pub fn serialize_connection_message(message: &ConnectionMessage) -> Bytes {
    let mut buf = BytesMut::new();
    match message {
        ConnectionMessage::ClientHello(hello) => {
            buf.put_u8(ConnectionMessageType::ClientHello as u8);
            buf.put_u16(hello.version);
            put_string(&mut buf, &hello.host);
            put_headers(&mut buf, &hello.headers);
            buf.put_u8(hello.available_compression_types.len() as u8);
            for ct in &hello.available_compression_types {
                buf.put_u8(*ct as u8);
            }
            buf.put_u32(hello.keepalive_min_ms);
            buf.put_u32(hello.keepalive_max_ms);
        }
        ConnectionMessage::ServerHello(hello) => {
            buf.put_u8(ConnectionMessageType::ServerHello as u8);
            buf.put_u16(hello.version);
            put_headers(&mut buf, &hello.headers);
            buf.put_u8(hello.compression_type as u8);
            buf.put_u32(hello.keepalive_interval_ms);
        }
        ConnectionMessage::Keepalive => buf.put_u8(ConnectionMessageType::Keepalive as u8),
        ConnectionMessage::KeepaliveAck => buf.put_u8(ConnectionMessageType::KeepaliveAck as u8),
        ConnectionMessage::Error(err) => {
            buf.put_u8(ConnectionMessageType::Error as u8);
            buf.put_u16(err.code);
            put_string(&mut buf, &err.message);
            buf.put_u8(if err.close { 1 } else { 0 });
        }
        ConnectionMessage::Close => buf.put_u8(ConnectionMessageType::Close as u8),
    }
    buf.freeze()
}

pub struct ControlStreamCodec {
    inner: LengthDelimitedCodec,
}

impl ControlStreamCodec {
    pub fn new() -> Self {
        Self {
            inner: LengthDelimitedCodec::builder()
                .length_field_length(4)
                .new_codec(),
        }
    }
}

impl Decoder for ControlStreamCodec {
    type Item = ConnectionMessage;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.inner.decode(src)? {
            Some(bytes) => {
                let msg = parse_connection_message(bytes.freeze())
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                Ok(Some(msg))
            }
            None => Ok(None),
        }
    }
}

impl Encoder<ConnectionMessage> for ControlStreamCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: ConnectionMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = serialize_connection_message(&item);
        self.inner.encode(bytes, dst)
    }
}

impl Default for ControlStreamCodec {
    fn default() -> Self {
        Self::new()
    }
}
