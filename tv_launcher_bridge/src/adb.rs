//! A small, self-contained ADB client over the network (no `adb` program needed).
//!
//! Supports Android "Wireless debugging" (pairing code + encrypted connection)
//! and the older plain network debugging (port 5555 with an "Allow" prompt).

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use base64::Engine;
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use rsa::traits::PublicKeyParts;
use rsa::{BigUint, Pkcs1v15Sign, RsaPrivateKey};
use sha1::Sha1;

use crate::spake2::Spake2;
use crate::tls;

const A_CNXN: u32 = 0x4e58_4e43;
const A_AUTH: u32 = 0x4854_5541;
const A_OPEN: u32 = 0x4e45_504f;
const A_OKAY: u32 = 0x5941_4b4f;
const A_CLSE: u32 = 0x4553_4c43;
const A_WRTE: u32 = 0x4554_5257;
const A_STLS: u32 = 0x534c_5453;

const AUTH_TOKEN: u32 = 1;
const AUTH_SIGNATURE: u32 = 2;
const AUTH_RSAPUBLICKEY: u32 = 3;

const VERSION: u32 = 0x0100_0000;
const STLS_VERSION: u32 = 0x0100_0000;
const MAX_DATA: u32 = 256 * 1024;

/// This computer's ADB identity: the RSA key and the TLS setup made from it.
pub struct Identity {
    pub key: RsaPrivateKey,
    pub tls: Arc<rustls::ClientConfig>,
}

/// Load the sign-in key from `path`, or create one (first run only).
pub fn load_or_create_identity(path: &std::path::Path) -> Result<Identity, String> {
    let key = match std::fs::read_to_string(path)
        .ok()
        .and_then(|pem| RsaPrivateKey::from_pkcs8_pem(&pem).ok())
    {
        Some(k) => k,
        None => {
            let key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048)
                .map_err(|e| format!("could not create a security key: {e}"))?;
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let pem = key
                .to_pkcs8_pem(LineEnding::LF)
                .map_err(|e| format!("could not save the key: {e}"))?;
            std::fs::write(path, pem.as_bytes()).map_err(|e| format!("could not save the key: {e}"))?;
            key
        }
    };
    let tls = tls::client_config(&key)?;
    // Keep a copy of the certificate next to the key (handy for checking).
    if let Ok(der) = tls::make_certificate(&key) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(der);
        let body: Vec<String> = b64.as_bytes().chunks(64).map(|c| String::from_utf8_lossy(c).into_owned()).collect();
        let pem = format!("-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n", body.join("\n"));
        let _ = std::fs::write(path.with_extension("cert.pem"), pem);
    }
    Ok(Identity { key, tls })
}

/// The public key in the format Android expects (`adbkey.pub`).
pub fn android_public_key(key: &RsaPrivateKey) -> Vec<u8> {
    let n = key.n();
    let words = 64usize; // 2048 bits / 32
    let mut n_bytes = n.to_bytes_le();
    n_bytes.resize(words * 4, 0);
    let n0 = u32::from_le_bytes([n_bytes[0], n_bytes[1], n_bytes[2], n_bytes[3]]);
    // Inverse of n0 modulo 2^32 (Newton iteration), then negate.
    let mut inv: u32 = n0;
    for _ in 0..5 {
        inv = inv.wrapping_mul(2u32.wrapping_sub(n0.wrapping_mul(inv)));
    }
    let n0inv = inv.wrapping_neg();
    let rr: BigUint = (BigUint::from(1u32) << (words * 32 * 2)) % n;
    let mut rr_bytes = rr.to_bytes_le();
    rr_bytes.resize(words * 4, 0);
    let e_bytes = key.e().to_bytes_le();
    let mut e = [0u8; 4];
    for (i, b) in e_bytes.iter().take(4).enumerate() {
        e[i] = *b;
    }

    let mut raw = Vec::with_capacity(12 + words * 8);
    raw.extend_from_slice(&(words as u32).to_le_bytes());
    raw.extend_from_slice(&n0inv.to_le_bytes());
    raw.extend_from_slice(&n_bytes);
    raw.extend_from_slice(&rr_bytes);
    raw.extend_from_slice(&e);
    let mut out = base64::engine::general_purpose::STANDARD
        .encode(raw)
        .into_bytes();
    out.extend_from_slice(b" tv-launcher-bridge@home-assistant\0");
    out
}

fn open_tcp(host: &str, port: u16) -> Result<TcpStream, String> {
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("can't find {host}: {e}"))?
        .next()
        .ok_or_else(|| format!("can't find {host}"))?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
        .map_err(|e| format!("can't reach the TV at {host}:{port} ({e})"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(10))).ok();
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

// ---------------------------------------------------------------------------
// Pairing ("Pair device with pairing code")
// ---------------------------------------------------------------------------

const PAIR_SPAKE2_MSG: u8 = 0;
const PAIR_PEER_INFO: u8 = 1;
const PEER_INFO_SIZE: usize = 8192;

fn pair_send(s: &mut tls::TlsStream, kind: u8, payload: &[u8]) -> Result<(), String> {
    let mut msg = vec![1u8, kind];
    msg.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    msg.extend_from_slice(payload);
    s.write_all(&msg)
        .and_then(|_| s.flush())
        .map_err(|e| format!("pairing: lost the connection ({e})"))
}

fn pair_recv(s: &mut tls::TlsStream, want: u8) -> Result<Vec<u8>, String> {
    let mut h = [0u8; 6];
    s.read_exact(&mut h)
        .map_err(|e| format!("pairing: the TV closed the connection ({e}). Is the pairing code still on screen?"))?;
    let len = u32::from_be_bytes([h[2], h[3], h[4], h[5]]) as usize;
    if h[0] != 1 || h[1] != want || len > PEER_INFO_SIZE * 2 {
        return Err("pairing: the TV sent something unexpected".into());
    }
    let mut data = vec![0u8; len];
    s.read_exact(&mut data)
        .map_err(|e| format!("pairing: the TV closed the connection ({e})"))?;
    Ok(data)
}

fn gcm_nonce(seq: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(&seq.to_le_bytes());
    n
}

/// Pair with the TV using the 6-digit code from "Pair device with pairing code".
/// Returns the TV's name for this pairing.
pub fn pair(host: &str, port: u16, code: &str, id: &Identity) -> Result<String, String> {
    let code: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if code.is_empty() {
        return Err("type the pairing code shown on the TV".into());
    }
    let tcp = open_tcp(host, port).map_err(|e| {
        format!("{e}. Use the pairing port shown under the code (it is different from the connect port).")
    })?;
    let mut s = tls::handshake(id.tls.clone(), host, tcp)?;
    let mut ekm = [0u8; 64];
    s.conn
        .export_keying_material(&mut ekm, b"adb-label\0", None)
        .map_err(|e| format!("pairing: {e}"))?;
    let mut password = code.into_bytes();
    password.extend_from_slice(&ekm);

    let spake = Spake2::new(true, b"adb pair client\0", b"adb pair server\0", &password);
    pair_send(&mut s, PAIR_SPAKE2_MSG, &spake.message())?;
    let theirs = pair_recv(&mut s, PAIR_SPAKE2_MSG)?;
    let shared = spake.finish(&theirs)?;

    let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, &shared);
    let mut aes_key = [0u8; 16];
    hk.expand(b"adb pairing_auth aes-128-gcm key", &mut aes_key)
        .map_err(|_| "pairing: key setup failed".to_string())?;
    let cipher = Aes128Gcm::new_from_slice(&aes_key).map_err(|_| "pairing: key setup failed".to_string())?;

    let mut info = vec![0u8; PEER_INFO_SIZE];
    info[0] = 0; // RSA public key
    let pk = android_public_key(&id.key);
    let n = pk.len().min(PEER_INFO_SIZE - 1);
    info[1..1 + n].copy_from_slice(&pk[..n]);
    let sealed = cipher
        .encrypt(Nonce::from_slice(&gcm_nonce(0)), info.as_slice())
        .map_err(|_| "pairing: encryption failed".to_string())?;
    pair_send(&mut s, PAIR_PEER_INFO, &sealed)?;

    let reply = pair_recv(&mut s, PAIR_PEER_INFO)?;
    let opened = cipher
        .decrypt(Nonce::from_slice(&gcm_nonce(0)), reply.as_slice())
        .map_err(|_| "wrong pairing code. Check the code on the TV and try again.".to_string())?;
    let guid: String = opened
        .get(1..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as char)
        .collect();
    Ok(if guid.is_empty() { "the TV".into() } else { guid })
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

enum Stream {
    Plain(TcpStream),
    Tls(Box<tls::TlsStream>),
    /// Only while swapping a plain connection for an encrypted one.
    Swapping,
}

fn swapping() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::NotConnected, "connection is being secured")
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
            Stream::Swapping => Err(swapping()),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
            Stream::Swapping => Err(swapping()),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
            Stream::Swapping => Err(swapping()),
        }
    }
}

impl Stream {
    fn set_timeout(&self, secs: u64) {
        let t = Some(Duration::from_secs(secs));
        match self {
            Stream::Plain(s) => drop(s.set_read_timeout(t)),
            Stream::Tls(s) => drop(s.sock.set_read_timeout(t)),
            Stream::Swapping => {}
        }
    }
}

struct Packet {
    cmd: u32,
    arg0: u32,
    arg1: u32,
    data: Vec<u8>,
}

pub struct Conn {
    stream: Stream,
    next_id: u32,
    pub encrypted: bool,
    pub device: String,
}

pub fn connect(host: &str, port: u16, id: &Identity, on_prompt: &dyn Fn()) -> Result<Conn, String> {
    let tcp = open_tcp(host, port).map_err(|e| {
        format!("{e}. Check the address and port on the TV's Wireless debugging screen.")
    })?;
    let mut c = Conn {
        stream: Stream::Plain(tcp),
        next_id: 1,
        encrypted: false,
        device: String::new(),
    };
    c.send(A_CNXN, VERSION, MAX_DATA, b"host::tv-launcher-bridge\0")?;
    let mut signed = false;
    for _ in 0..12 {
        let p = c.recv().map_err(|e| {
            if c.encrypted {
                format!("the TV didn't accept this computer ({e}). Pair it with the code from Wireless debugging first.")
            } else if signed {
                format!("the TV didn't answer the sign-in ({e}). If a prompt is showing on the TV, choose Allow and try again.")
            } else {
                e
            }
        })?;
        match p.cmd {
            A_CNXN => {
                c.device = String::from_utf8_lossy(&p.data)
                    .split(';')
                    .find_map(|kv| kv.strip_prefix("ro.product.model=").map(|s| s.to_string()))
                    .unwrap_or_default();
                return Ok(c);
            }
            A_STLS => {
                // Wireless debugging: switch this connection to TLS.
                c.send(A_STLS, STLS_VERSION, 0, &[])?;
                let Stream::Plain(tcp) = std::mem::replace(&mut c.stream, Stream::Swapping) else {
                    return Err("unexpected second encryption request".into());
                };
                let tls = tls::handshake(id.tls.clone(), host, tcp)?;
                c.stream = Stream::Tls(Box::new(tls));
                c.encrypted = true;
            }
            A_AUTH if p.arg0 == AUTH_TOKEN => {
                if !signed {
                    let sig = id
                        .key
                        .sign(Pkcs1v15Sign::new::<Sha1>(), &p.data)
                        .map_err(|e| format!("signing failed: {e}"))?;
                    c.send(A_AUTH, AUTH_SIGNATURE, 0, &sig)?;
                    signed = true;
                } else {
                    // The TV doesn't know this key yet: offer it, and wait for "Allow".
                    c.send(A_AUTH, AUTH_RSAPUBLICKEY, 0, &android_public_key(&id.key))?;
                    on_prompt();
                    c.stream.set_timeout(60);
                }
            }
            _ => {}
        }
    }
    Err("ADB connection handshake exceeded 12 packets; retry or pair again".into())
}

impl Conn {
    fn send(&mut self, cmd: u32, arg0: u32, arg1: u32, data: &[u8]) -> Result<(), String> {
        let sum: u32 = data.iter().map(|b| *b as u32).fold(0u32, |a, b| a.wrapping_add(b));
        let mut h = Vec::with_capacity(24 + data.len());
        for v in [cmd, arg0, arg1, data.len() as u32, sum, cmd ^ 0xffff_ffff] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        h.extend_from_slice(data);
        self.stream
            .write_all(&h)
            .and_then(|_| self.stream.flush())
            .map_err(|e| format!("lost the connection to the TV ({e})"))
    }

    fn recv(&mut self) -> Result<Packet, String> {
        let mut h = [0u8; 24];
        self.stream
            .read_exact(&mut h)
            .map_err(|e| format!("the TV closed the connection ({e})"))?;
        let w = |i: usize| u32::from_le_bytes([h[i], h[i + 1], h[i + 2], h[i + 3]]);
        let len = w(12) as usize;
        if len > 16 * 1024 * 1024 {
            return Err("the TV sent something unexpected".into());
        }
        let mut data = vec![0u8; len];
        self.stream
            .read_exact(&mut data)
            .map_err(|e| format!("the TV closed the connection ({e})"))?;
        Ok(Packet {
            cmd: w(0),
            arg0: w(4),
            arg1: w(8),
            data,
        })
    }

    /// Run one shell command on the TV and return what it printed.
    pub fn shell(&mut self, command: &str) -> Result<String, String> {
        let local_id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let mut dest = format!("shell:{command}").into_bytes();
        dest.push(0);
        self.send(A_OPEN, local_id, 0, &dest)?;
        self.stream.set_timeout(25);
        let mut out = Vec::new();
        let mut remote = 0u32;
        loop {
            let p = self.recv()?;
            if p.arg1 != local_id {
                continue; // leftovers from an earlier command
            }
            match p.cmd {
                A_OKAY => remote = p.arg0,
                A_WRTE => {
                    remote = p.arg0;
                    out.extend_from_slice(&p.data);
                    self.send(A_OKAY, local_id, remote, &[])?;
                }
                A_CLSE => {
                    let _ = self.send(A_CLSE, local_id, remote, &[]);
                    break;
                }
                _ => {}
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

/// Quote a value for the TV's shell.
pub fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn is_package_name(s: &str) -> bool {
    !s.is_empty()
        && s.contains('.')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}
