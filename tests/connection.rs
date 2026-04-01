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

#[tokio::test]
async fn test_successful_connection() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let (cert_chain, private_key) = generate_cert();

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

    let client_config = ClientConfig {
        bind_address: "127.0.0.1:0".parse().unwrap(),
        root_certificates: cert_chain,
        supported_compression_types: vec![CompressionType::Lz4, CompressionType::None],
        keepalive_range: Duration::from_secs(1)..Duration::from_secs(10),
        protofish_config: protofish2::config::ProtofishConfig::default(),
    };

    let client = ProtofishClient::bind(client_config).expect("Failed to bind client");

    let server_task = tokio::spawn(async move {
        let incoming = server.accept().await.expect("No incoming connection");
        let server_conn = incoming
            .accept()
            .await
            .expect("Server failed to accept handshake");
        server_conn
    });

    let client_conn = client
        .connect(server_addr, "localhost", HashMap::new())
        .await
        .expect("Client failed to connect and handshake");

    let server_conn = server_task.await.unwrap();

    assert_eq!(
        client_conn.state.read().await.compression_type,
        CompressionType::Lz4
    );
    assert_eq!(
        server_conn.state.read().await.compression_type,
        CompressionType::Lz4
    );
}
