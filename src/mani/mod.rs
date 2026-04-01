//! Reliable data transfer layer for Protofish2.
//!
//! The Mani module provides reliable, in-order delivery of data with optional compression.
//! It sits on top of QUIC and provides semantics similar to TCP but with configurable
//! reliability guarantees and compression.

mod codec;
pub(crate) mod frame;
pub(crate) mod message;
pub mod transfer;

pub mod stream;
