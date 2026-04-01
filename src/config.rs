#[derive(Debug, Clone)]
pub struct ManiConfig {
    pub max_retransmission_buffer_size: usize,
    pub max_nack_channel_size: usize,
    pub max_datagram_channel_size: usize,
    pub max_chunk_buffer_size: usize,
}

impl Default for ManiConfig {
    fn default() -> Self {
        Self {
            max_retransmission_buffer_size: 1024,
            max_nack_channel_size: 100,
            max_datagram_channel_size: 1000,
            max_chunk_buffer_size: 1000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProtofishConfig {
    pub retransmission_buffer_size: usize,
    pub mani_config: ManiConfig,
}

impl Default for ProtofishConfig {
    fn default() -> Self {
        Self {
            retransmission_buffer_size: 1024,
            mani_config: ManiConfig::default(),
        }
    }
}
