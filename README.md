![banner](/assets/protofish-banner.png)

# protofish2-rs

Rust implementation of the Protofish2 protocol - a QUIC-based reliable data transfer protocol designed for high-performance, low-latency communication. Combines QUIC's efficiency with custom reliability mechanisms and pluggable compression to handle both ordered/reliable and unordered/unreliable data streams.

## Features

- QUIC-based transport via Quinn
- Ordered and unordered data streams
- Configurable compression (Gzip, Zstd, Lz4, or none)
- Automatic retransmission via NACK (negative acknowledgment)
- Type-safe protocol implementation with newtypes
- Lock-free concurrent datagram routing
- Full async/await support with Tokio

## Project Structure

```
src/
├── lib.rs               # Public API exports
├── types.rs             # Core newtype definitions (ManiStreamId, SequenceNumber)
├── error.rs             # Error types
├── config.rs            # Configuration structures
├── compression.rs       # Compression trait and implementations
├── connection/          # QUIC connection management (server/client)
├── datagram/            # Unreliable datagram routing
└── mani/                # Main transfer protocol
    └── transfer/        # Reliable/unreliable bulk data transfer
```

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
protofish2 = { path = "./path/to/protofish2-rs" }
tokio = { version = "1", features = ["full"] }
```

## Quick Start

### Server Example

```rust
use protofish2::connection::{ProtofishServer, ServerConfig};
use protofish2::compression::CompressionType;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup TLS certificates and private key
    let cert = load_cert("cert.pem")?;
    let key = load_key("key.pem")?;

    let config = ServerConfig {
        bind_address: "127.0.0.1:5000".parse()?,
        cert_chain: vec![cert],
        private_key: key,
        supported_compression_types: vec![CompressionType::Lz4, CompressionType::None],
        keepalive_interval: Duration::from_secs(5),
        protofish_config: Default::default(),
    };

    let server = ProtofishServer::bind(config)?;

    loop {
        if let Some(incoming) = server.accept().await {
            tokio::spawn(async move {
                if let Ok(mut conn) = incoming.accept().await {
                    while let Ok((rel_stream, unrel_stream)) = conn.accept_mani().await {
                        tokio::spawn(async move {
                            // Handle transfer
                            if let Ok(mut recv) = rel_stream.recv().await {
                                while let Some(chunks) = recv.recv().await {
                                    for chunk in chunks {
                                        println!("Received: {:?}", chunk.content);
                                    }
                                }
                            }
                        });
                    }
                }
            });
        }
    }
}
```

### Client Example

```rust
use protofish2::connection::{ProtofishClient, ClientConfig};
use protofish2::compression::CompressionType;
use protofish2::types::SequenceNumber;
use std::time::Duration;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load root certificate
    let root_cert = load_root_cert("ca.pem")?;

    let config = ClientConfig {
        bind_address: "127.0.0.1:0".parse()?,
        root_certificates: vec![root_cert],
        supported_compression_types: vec![CompressionType::Lz4],
        keepalive_range: Duration::from_secs(1)..Duration::from_secs(10),
        protofish_config: Default::default(),
    };

    let client = ProtofishClient::bind(config)?;
    
    // Connect to server
    let mut conn = client.connect(
        "127.0.0.1:5000".parse()?,
        "server.example.com",
        HashMap::new(),
    ).await?;

    // Open MANI stream
    let mut stream = conn.open_mani().await?;

    // Start transfer with compression
    let mut send_stream = stream.start_transfer(
        CompressionType::Lz4,
        SequenceNumber(0),
        Some(1024), // optional total size
    ).await?;

    // Send data
    let data = b"Hello, Protofish2!";
    send_stream.send(0, data.to_vec()).await?;

    // Signal transfer complete
    stream.end_transfer(SequenceNumber(1)).await?;

    Ok(())
}
```

## Core Concepts

### ManiStreamId

Type-safe wrapper for stream identifiers. Prevents accidentally using wrong ID types.

```rust
use protofish2::types::ManiStreamId;

let stream_id = ManiStreamId(42);
```

### SequenceNumber

Tracks ordering of chunks. Supports arithmetic operations (Add, Sub).

```rust
use protofish2::types::SequenceNumber;

let seq = SequenceNumber(0);
let next = seq + SequenceNumber(1);
```

### Compression Types

Negotiated during connection handshake. Supported types:

- `CompressionType::None` - No compression
- `CompressionType::Gzip` - Gzip compression
- `CompressionType::Zstd` - Zstandard compression
- `CompressionType::Lz4` - LZ4 compression

```rust
use protofish2::compression::CompressionType;

let comp = CompressionType::Lz4;
```

### Transfer Streams

Two-stream model for each transfer:

- **Reliable Stream**: Ordered chunks with automatic gap-filling via NACK
- **Unreliable Stream**: Chunks as received, no reordering guarantee

```rust
// Accept transfer from peer
let (reliable_stream, unreliable_stream) = stream.accept_transfer().await?;

// Receive ordered chunks
while let Some(chunks) = reliable_stream.recv().await {
    for chunk in chunks {
        process(&chunk.content);
    }
}
```

### Retransmission

Uses NACK (negative acknowledgment) protocol:
1. Receiver detects missing sequence numbers
2. Sends NACK with list of missing sequences
3. Sender retransmits from internal buffer
4. No timeout-based retransmission needed

## Configuration

Customize behavior via `ProtofishConfig`:

```rust
use protofish2::config::ProtofishConfig;

let config = ProtofishConfig {
    retransmission_buffer_size: 1024 * 1024, // 1 MB
    mani_config: Default::default(),
};
```

## Building and Testing

```bash
# Build project
cargo build

# Run tests
cargo test

# Run with optimizations
cargo build --release

# Check code formatting
cargo fmt --check

# Run linter
cargo clippy -- -D warnings
```

## Error Handling

The protocol provides detailed error types:

```rust
use protofish2::error::{ProtofishConnectionError, TransferSendError};

match stream.send_payload(data).await {
    Ok(_) => println!("Success"),
    Err(e) => eprintln!("Error: {}", e),
}
```

## Dependencies

Core dependencies:
- `tokio` - Async runtime
- `quinn` - QUIC protocol
- `rustls` - TLS 1.3
- `bytes` - Efficient byte buffers
- `flate2`, `zstd`, `lz4_flex` - Compression algorithms
- `thiserror` - Error handling
- `derive_more` - Type utilities

## Example

See `examples/basic_connection.rs` for a complete client-server handshake example.

## Development Guidelines

See `AGENTS.md` for detailed development guidelines including:
- Code style and formatting
- Testing patterns
- Error handling conventions
- Module organization
- Type safety practices

## License

See LICENSE file for details.
