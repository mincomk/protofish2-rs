#[cfg(feature = "opus")]
use crate::mani::transfer::recv::TransferReliableRecvStream;

#[cfg(feature = "opus")]
pub struct OpusDecoderStream {
    receiver: TransferReliableRecvStream,
    decoder: opus::Decoder,
    channels: opus::Channels,
}

#[cfg(feature = "opus")]
impl OpusDecoderStream {
    pub fn new(
        receiver: TransferReliableRecvStream,
        sample_rate: u32,
        channels: opus::Channels,
    ) -> Result<Self, opus::Error> {
        let decoder = opus::Decoder::new(sample_rate, channels)?;
        Ok(Self {
            receiver,
            decoder,
            channels,
        })
    }

    pub async fn recv(&mut self) -> Result<Option<Vec<Vec<i16>>>, opus::Error> {
        match self.receiver.recv().await {
            Some(chunks) => {
                let mut decoded_chunks = Vec::with_capacity(chunks.len());
                for chunk in chunks {
                    // Maximum frame size for opus is 120ms.
                    // 120ms at 48kHz is 5760 samples per channel.
                    let max_samples = 5760 * self.channels as usize;
                    let mut pcm = vec![0i16; max_samples];
                    let decoded_len = tokio::task::block_in_place(|| {
                        self.decoder.decode(&chunk.content, &mut pcm, false)
                    })?;
                    pcm.truncate(decoded_len * self.channels as usize);
                    decoded_chunks.push(pcm);
                }
                Ok(Some(decoded_chunks))
            }
            None => Ok(None),
        }
    }
}
