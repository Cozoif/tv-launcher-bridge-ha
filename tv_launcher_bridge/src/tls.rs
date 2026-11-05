//! TLS 1.3 for Android "Wireless debugging": a self-signed certificate made from
//! this computer's ADB key, and a client that (like adb itself) doesn't check the
//! TV's certificate — the TV is trusted through pairing instead.

use std::net::TcpStream;
use std::sync::Arc;

use rsa::pkcs8::{EncodePrivateKey, EncodePublicKey};
use rsa::{Pkcs1v15Sign, RsaPrivateKey};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
use sha2::{Digest, Sha256};

fn der_len(n: usize) -> Vec<u8> {
    if n < 0x80 {
        vec![n as u8]
    } else {
        let bytes: Vec<u8> = n.to_be_bytes().iter().copied().skip_while(|b| *b == 0).collect();
        let mut v = vec![0x80 | bytes.len() as u8];
        v.extend(bytes);
        v
    }
}

fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend(der_len(content.len()));
    v.extend_from_slice(content);
    v
}

fn seq(parts: &[&[u8]]) -> Vec<u8> {
    tlv(0x30, &parts.concat())
}

/// A self-signed X.509 certificate for the key (DER).
pub fn make_certificate(key: &RsaPrivateKey) -> Result<Vec<u8>, String> {
    let sha256_rsa = seq(&[
        &tlv(0x06, &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b]),
        &[0x05, 0x00],
    ]);
    let cn = |s: &str| {
        seq(&[&tlv(
            0x31,
            &seq(&[&tlv(0x06, &[0x55, 0x04, 0x03]), &tlv(0x0c, s.as_bytes())]),
        )])
    };
    let name = cn("tv-launcher-bridge");
    let validity = seq(&[&tlv(0x17, b"250101000000Z"), &tlv(0x18, b"20500101000000Z")]);
    let spki = key
        .to_public_key()
        .to_public_key_der()
        .map_err(|e| format!("certificate: {e}"))?;
    let tbs = seq(&[
        &[0xa0, 0x03, 0x02, 0x01, 0x02], // version 3
        &[0x02, 0x01, 0x01],             // serial 1
        &sha256_rsa,
        &name,
        &validity,
        &name,
        spki.as_bytes(),
    ]);
    let digest = Sha256::digest(&tbs);
    let sig = key
        .sign(Pkcs1v15Sign::new::<Sha256>(), &digest)
        .map_err(|e| format!("certificate: {e}"))?;
    let mut bits = vec![0u8];
    bits.extend(sig);
    Ok(seq(&[&tbs, &sha256_rsa, &tlv(0x03, &bits)]))
}

#[derive(Debug)]
struct TrustByPairing(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for TrustByPairing {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

pub fn client_config(key: &RsaPrivateKey) -> Result<Arc<ClientConfig>, String> {
    let provider = Arc::new(rustls_rustcrypto::provider());
    let cert = make_certificate(key)?;
    let key_der = key
        .to_pkcs8_der()
        .map_err(|e| format!("key: {e}"))?
        .as_bytes()
        .to_vec();
    let cfg = ClientConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| format!("TLS setup: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(TrustByPairing(provider)))
        .with_client_auth_cert(
            vec![CertificateDer::from(cert)],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der)),
        )
        .map_err(|e| format!("TLS setup: {e}"))?;
    Ok(Arc::new(cfg))
}

pub type TlsStream = StreamOwned<ClientConnection, TcpStream>;

/// Run the TLS handshake on an open connection.
pub fn handshake(cfg: Arc<ClientConfig>, host: &str, mut tcp: TcpStream) -> Result<TlsStream, String> {
    let name = ServerName::try_from(host.to_string())
        .map_err(|_| format!("\"{host}\" isn't a valid address"))?;
    let mut conn = ClientConnection::new(cfg, name).map_err(|e| format!("TLS: {e}"))?;
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp).map_err(|e| {
            format!("secure connection failed ({e}). If you haven't paired yet, pair with the code first.")
        })?;
    }
    Ok(StreamOwned::new(conn, tcp))
}
