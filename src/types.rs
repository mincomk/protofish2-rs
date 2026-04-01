use derive_more::{Add, Display, From, Into, Sub};

/// Unique identifier for a MANI stream within a connection.
///
/// This is a lightweight wrapper around a `u64` that uniquely identifies
/// a stream in the protocol. It corresponds to the QUIC stream index.
///
/// # Examples
///
/// ```
/// use protofish2::ManiStreamId;
///
/// let stream_id = ManiStreamId(42);
/// println!("Stream ID: {}", stream_id); // Prints: Stream ID: 42
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, PartialOrd, Ord, From, Into)]
pub struct ManiStreamId(pub u64);

/// Sequence number used for ordering chunks within a transfer.
///
/// Sequence numbers wrap around at `u32::MAX` to allow for efficient
/// tracking of sent/received chunks. They support arithmetic operations
/// for calculating ranges of missing chunks.
///
/// # Examples
///
/// ```
/// use protofish2::SequenceNumber;
///
/// let seq1 = SequenceNumber(100);
/// let seq2 = SequenceNumber(101);
/// assert_eq!(seq2 - seq1, SequenceNumber(1));
/// ```
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Display, PartialOrd, Ord, Add, Sub, From, Into,
)]
pub struct SequenceNumber(pub u32);

/// Timestamp for a chunk, measured in milliseconds.
///
/// This timestamp is included with each chunk to track timing information
/// and can be useful for performance analysis and debugging.
///
/// # Examples
///
/// ```
/// use protofish2::Timestamp;
///
/// let ts = Timestamp(1234567890);
/// println!("Timestamp: {} ms", ts.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timestamp(pub u64);
