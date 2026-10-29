//! Encrypts saved secrets (passwords, private keys) at rest with XChaCha20-Poly1305.
use anyhow::{Result, anyhow};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use rand::RngExt;

pub struct Vault {
    cipher: XChaCha20Poly1305,
}

impl Vault {
    pub fn new(key: &[u8; 32]) -> Self {
        Self { cipher: XChaCha20Poly1305::new(&Key::from(*key)) }
    }

    pub fn generate_key() -> [u8; 32] {
        let mut k = [0u8; 32];
        rand::rng().fill(&mut k);
        k
    }

    /// Returns base64(nonce || ciphertext).
    pub fn seal(&self, plaintext: &str) -> Result<String> {
        let mut nonce = [0u8; 24];
        rand::rng().fill(&mut nonce);
        let ct = self
            .cipher
            .encrypt(&XNonce::from(nonce), plaintext.as_bytes())
            .map_err(|_| anyhow!("encryption failed"))?;
        let mut out = nonce.to_vec();
        out.extend(ct);
        Ok(B64.encode(out))
    }

    pub fn open(&self, sealed: &str) -> Result<String> {
        let raw = B64.decode(sealed)?;
        if raw.len() < 25 {
            return Err(anyhow!("sealed value too short"));
        }
        let (nonce, ct) = raw.split_at(24);
        let pt = self
            .cipher
            .decrypt(&XNonce::try_from(nonce).map_err(|_| anyhow!("bad nonce"))?, ct)
            .map_err(|_| anyhow!("decryption failed (wrong master key?)"))?;
        Ok(String::from_utf8(pt)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_tamper() {
        let v = Vault::new(&Vault::generate_key());
        let s = v.seal("s3cret").unwrap();
        assert_eq!(v.open(&s).unwrap(), "s3cret");
        assert_ne!(v.seal("s3cret").unwrap(), s, "nonce must differ");
        let other = Vault::new(&Vault::generate_key());
        assert!(other.open(&s).is_err());
    }
}
