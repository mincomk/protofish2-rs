use anyhow::{Context, Result, anyhow};
use rcgen::generate_simple_self_signed;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::fs;
use std::path::Path;

pub const CERT_PATH: &str = "cert.pem";
pub const KEY_PATH: &str = "key.pem";

pub fn write_self_signed() -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert = generate_simple_self_signed(vec!["localhost".to_string()])
        .context("generate self-signed cert")?;
    let cert_pem = cert.cert.pem();
    let key_pem = cert.signing_key.serialize_pem();
    fs::write(CERT_PATH, &cert_pem).with_context(|| format!("write {CERT_PATH}"))?;
    fs::write(KEY_PATH, &key_pem).with_context(|| format!("write {KEY_PATH}"))?;
    tracing::info!(cert = CERT_PATH, key = KEY_PATH, "wrote self-signed cert");
    Ok((
        vec![CertificateDer::from(cert.cert.der().to_vec())],
        PrivateKeyDer::Pkcs8(cert.signing_key.serialize_der().into()),
    ))
}

pub fn load_cert_chain(path: impl AsRef<Path>) -> Result<Vec<CertificateDer<'static>>> {
    let path = path.as_ref();
    let mut reader =
        std::io::BufReader::new(fs::File::open(path).with_context(|| format!("open {path:?}"))?);
    let chain: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<_, _>>()
        .with_context(|| format!("parse certs in {path:?}"))?;
    if chain.is_empty() {
        return Err(anyhow!("no certificates found in {path:?}"));
    }
    Ok(chain)
}
