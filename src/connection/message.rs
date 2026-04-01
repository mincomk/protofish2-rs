use crate::compression::CompressionType;
use bytes::Bytes;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMessageType {
    ClientHello = 0x01,
    ServerHello = 0x02,
    Keepalive = 0x03,
    KeepaliveAck = 0x04,
    Error = 0x05,
    Close = 0x06,
}

impl ConnectionMessageType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::ClientHello),
            0x02 => Some(Self::ServerHello),
            0x03 => Some(Self::Keepalive),
            0x04 => Some(Self::KeepaliveAck),
            0x05 => Some(Self::Error),
            0x06 => Some(Self::Close),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ConnectionMessage {
    ClientHello(ClientHello),
    ServerHello(ServerHello),
    Keepalive,
    KeepaliveAck,
    Error(ConnectionErrorMsg),
    Close,
}

#[derive(Debug, Clone)]
pub struct ClientHello {
    pub version: u16,
    pub host: String,
    pub headers: HashMap<String, Bytes>,
    pub available_compression_types: Vec<CompressionType>,
    pub keepalive_min_ms: u32,
    pub keepalive_max_ms: u32,
}

#[derive(Debug, Clone)]
pub struct ServerHello {
    pub version: u16,
    pub headers: HashMap<String, Bytes>,
    pub compression_type: CompressionType,
    pub keepalive_interval_ms: u32,
}

#[derive(Debug, Clone)]
pub struct ConnectionErrorMsg {
    pub code: u16,
    pub message: String,
    pub close: bool,
}
