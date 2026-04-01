use derive_more::{Add, Display, From, Into, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, PartialOrd, Ord, From, Into)]
pub struct ManiStreamId(pub u64);

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Display, PartialOrd, Ord, Add, Sub, From, Into,
)]
pub struct SequenceNumber(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timestamp(pub u64);
