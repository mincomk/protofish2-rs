# Project Structure

- `examples/jitter_ogg/`: Example application demonstrating Ogg Opus reading, simulating local transmission over a Protofish2 connection, decoding via `OpusJitterBuffer`, and writing the output to PCM (WAV) format using `hound`.

# Common Mistakes

- The `protofish2::TransferMode` enum is exposed directly from the crate root, rather than from a private `mani::message` module.
- `ogg::PacketReader` yields `Option<Packet>` via `read_packet()`. `read_packet_expected()` yields `Packet` or an Error, which makes `while let Some()` invalid when expecting Option.
