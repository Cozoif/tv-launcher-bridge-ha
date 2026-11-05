//! SPAKE2 over edwards25519, compatible with the version Android's
//! "Wireless debugging" pairing uses (BoringSSL's spake25519).

use curve25519_dalek::constants::ED25519_BASEPOINT_POINT;
use curve25519_dalek::edwards::{CompressedEdwardsY, EdwardsPoint};
use curve25519_dalek::scalar::Scalar;
use rand::RngCore;
use sha2::{Digest, Sha512};

// Fixed points, made by hashing "edwards25519 point generation seed (M)" / "(N)"
// with SHA-256 until a valid point appears (checked against the published values).
const M_BYTES: [u8; 32] = hex32("5ada7e4bf6ddd9adb6626d32131c6b5c51a1e347a3478f53cfcf441b88eed12e");
const N_BYTES: [u8; 32] = hex32("10e3df0ae37d8e7a99b5fe74b44672103dbddcbd06af680d71329a11693bc778");

const fn hex32(s: &str) -> [u8; 32] {
    let b = s.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = (nib(b[2 * i]) << 4) | nib(b[2 * i + 1]);
        i += 1;
    }
    out
}

const fn nib(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => 0,
    }
}

fn point(bytes: &[u8; 32]) -> EdwardsPoint {
    CompressedEdwardsY(*bytes).decompress().expect("fixed SPAKE2 point")
}

pub struct Spake2 {
    alice: bool,
    my_name: Vec<u8>,
    their_name: Vec<u8>,
    /// The ephemeral secret is 8·r (the cofactor is folded in).
    r: Scalar,
    /// The password scalar is 8·t, with 8·t ≡ h(password) (mod ℓ).
    t: Scalar,
    password_hash: [u8; 64],
    my_msg: [u8; 32],
}

impl Spake2 {
    pub fn new(alice: bool, my_name: &[u8], their_name: &[u8], password: &[u8]) -> Self {
        let mut wide = [0u8; 64];
        rand::thread_rng().fill_bytes(&mut wide);
        let r = Scalar::from_bytes_mod_order_wide(&wide);

        let password_hash: [u8; 64] = Sha512::digest(password).into();
        let h = Scalar::from_bytes_mod_order_wide(&password_hash);
        let t = h * Scalar::from(8u8).invert();

        let eight = Scalar::from(8u8);
        let p = ED25519_BASEPOINT_POINT * (r * eight);
        let my_point = if alice { point(&M_BYTES) } else { point(&N_BYTES) };
        let mask = my_point.mul_by_cofactor() * t;
        let my_msg = (p + mask).compress().to_bytes();
        Self {
            alice,
            my_name: my_name.to_vec(),
            their_name: their_name.to_vec(),
            r,
            t,
            password_hash,
            my_msg,
        }
    }

    pub fn message(&self) -> [u8; 32] {
        self.my_msg
    }

    /// Combine with the other side's message; returns the 64-byte shared key.
    pub fn finish(&self, their_msg: &[u8]) -> Result<[u8; 64], String> {
        let their: [u8; 32] = their_msg
            .try_into()
            .map_err(|_| "the TV sent a pairing message of the wrong size".to_string())?;
        let q_star = CompressedEdwardsY(their)
            .decompress()
            .ok_or_else(|| "the TV sent an invalid pairing message".to_string())?;
        let their_point = if self.alice { point(&N_BYTES) } else { point(&M_BYTES) };
        let q = q_star - their_point.mul_by_cofactor() * self.t;
        let dh = (q.mul_by_cofactor() * self.r).compress().to_bytes();

        let mut sha = Sha512::new();
        let mut put = |d: &[u8]| {
            sha.update((d.len() as u64).to_le_bytes());
            sha.update(d);
        };
        if self.alice {
            put(&self.my_name);
            put(&self.their_name);
            put(&self.my_msg);
            put(&their);
        } else {
            put(&self.their_name);
            put(&self.my_name);
            put(&their);
            put(&self.my_msg);
        }
        put(&dh);
        put(&self.password_hash);
        Ok(sha.finalize().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sides_agree() {
        let a = Spake2::new(true, b"adb pair client\0", b"adb pair server\0", b"123456xyz");
        let b = Spake2::new(false, b"adb pair server\0", b"adb pair client\0", b"123456xyz");
        let ka = a.finish(&b.message()).unwrap();
        let kb = b.finish(&a.message()).unwrap();
        assert_eq!(ka, kb);
        let c = Spake2::new(false, b"adb pair server\0", b"adb pair client\0", b"654321xyz");
        assert_ne!(a.finish(&c.message()).unwrap(), c.finish(&a.message()).unwrap());
    }

    /// Runs the other side with a literal Python version of BoringSSL's algorithm.
    /// `cargo test -- --ignored` (needs python3 and the script path in SPAKE_REF).
    #[test]
    #[ignore]
    fn matches_reference() {
        use std::io::Write;
        let script = std::env::var("SPAKE_REF").expect("SPAKE_REF");
        let pw = b"482913\x01\x02\x03extra";
        let a = Spake2::new(true, b"adb pair client\0", b"adb pair server\0", pw);
        let mut child = std::process::Command::new("python3")
            .args(["-I", &script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        writeln!(child.stdin.take().unwrap(), "{} {}", hex(pw), hex(&a.message())).unwrap();
        let out = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        let mut it = out.split_whitespace();
        let bob_msg = it.next().unwrap();
        let bob_key = it.next().unwrap();
        let msg: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&bob_msg[2 * i..2 * i + 2], 16).unwrap()).collect();
        let ka = a.finish(&msg).unwrap();
        assert_eq!(hex(&ka), bob_key);
    }
}
