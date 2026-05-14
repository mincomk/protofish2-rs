use anyhow::{Context, Result};
use rodio::{OutputStream, Sink, buffer::SamplesBuffer};
use std::time::Duration;

pub const SAMPLE_RATE: u32 = 48000;
pub const CHANNELS: u16 = 2;
pub const MAX_QUEUED_FRAMES: usize = 25;
const POLL_INTERVAL: Duration = Duration::from_millis(2);

pub struct Player {
    _stream: OutputStream,
    sink: Sink,
}

impl Player {
    pub fn new() -> Result<Self> {
        let (stream, handle) =
            OutputStream::try_default().context("failed to open default audio output")?;
        let sink = Sink::try_new(&handle).context("failed to create rodio sink")?;
        Ok(Self {
            _stream: stream,
            sink,
        })
    }

    pub fn push_pcm(&self, pcm: Vec<f32>) {
        self.sink
            .append(SamplesBuffer::new(CHANNELS, SAMPLE_RATE, pcm));
    }

    pub async fn wait_until_below(&self, threshold: usize) {
        while self.sink.len() >= threshold {
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub fn wait_drained(&self) {
        self.sink.sleep_until_end();
    }
}
