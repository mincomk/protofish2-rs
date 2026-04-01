use std::collections::HashMap;
use std::time::Duration;

use protofish2::compression::CompressionType;
use protofish2::connection::{ClientConfig, ProtofishClient, ProtofishServer, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

fn generate_cert() -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
    let subject_alt_names = vec!["localhost".to_string()];
    let cert = rcgen::generate_simple_self_signed(subject_alt_names).unwrap();
    let der_cert = cert.cert.der().to_vec();
    let der_key = cert.signing_key.serialize_der();
    (
        vec![CertificateDer::from(der_cert)],
        PrivateKeyDer::Pkcs8(der_key.into()),
    )
}

#[tokio::main]
async fn main() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    // Initialize tracing for logs
    tracing_subscriber::fmt::init();
    tracing::info!("Starting basic_connection example...");

    // Generate self-signed certificate for the example
    let (cert_chain, private_key) = generate_cert();

    // 1. Setup the Server
    let server_config = ServerConfig {
        bind_address: "127.0.0.1:0".parse().unwrap(),
        cert_chain: cert_chain.clone(),
        private_key,
        supported_compression_types: vec![CompressionType::Lz4, CompressionType::None],
        keepalive_interval: Duration::from_secs(5),
        protofish_config: protofish2::config::ProtofishConfig::default(),
    };

    let server = ProtofishServer::bind(server_config).expect("Failed to bind server");
    let server_addr = server.local_addr().unwrap();
    tracing::info!("Server bound to {}", server_addr);

    // 2. Setup the Client
    let client_config = ClientConfig {
        bind_address: "127.0.0.1:0".parse().unwrap(),
        root_certificates: cert_chain,
        supported_compression_types: vec![CompressionType::Lz4, CompressionType::None],
        keepalive_range: Duration::from_secs(1)..Duration::from_secs(10),
        protofish_config: protofish2::config::ProtofishConfig::default(),
    };

    let client = ProtofishClient::bind(client_config).expect("Failed to bind client");
    tracing::info!("Client bound locally");

    // 3. Spawn server task to accept incoming connections
    let server_task = tokio::spawn(async move {
        tracing::info!("Server waiting for incoming connection...");
        if let Some(incoming) = server.accept().await {
            tracing::info!("Server received an incoming connection request...");
            match incoming.accept().await {
                Ok(conn) => {
                    let comp = conn.state.read().await.compression_type;
                    tracing::info!(
                        "Server successfully handshaked! Negotiated compression: {:?}",
                        comp
                    );
                    conn
                }
                Err(e) => {
                    tracing::error!("Server handshake failed: {}", e);
                    panic!("Handshake failed");
                }
            }
        } else {
            panic!("No incoming connection");
        }
    });

    // 4. Client connects to server
    tracing::info!("Client attempting to connect to {}...", server_addr);
    let mut headers = HashMap::new();
    headers.insert(
        "example-header".to_string(),
        bytes::Bytes::from("hello server"),
    );

    let client_conn = client
        .connect(server_addr, "localhost", headers)
        .await
        .expect("Client failed to connect and handshake");

    let comp = client_conn.state.read().await.compression_type;
    tracing::info!(
        "Client successfully handshaked! Negotiated compression: {:?}",
        comp
    );

    // Wait for the server task to complete
    let _server_conn = server_task.await.unwrap();

    tracing::info!("Example completed successfully!");
}
