use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as b64, Engine};
use rand::{rngs::OsRng, RngCore};
use std::env;

#[derive(Debug, Clone)]
pub struct EncryptionError(pub String);

impl std::fmt::Display for EncryptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Encryption error: {}", self.0)
    }
}

// Ensure the secret key is 32 bytes for AES-256. If missing, panic at init.
fn get_cipher() -> Result<Aes256Gcm, EncryptionError> {
    let key_str = match env::var("FLUXBASE_SECRET_KEY") {
        Ok(v) if !v.is_empty() => v,
        _ => {
            if env::var("FLUX_ENV").as_deref() == Ok("production") {
                panic!(
                    "[Flux] FLUXBASE_SECRET_KEY must be set in production. \
                     Generate a 32-byte random key with: openssl rand -hex 16"
                );
            }
            tracing::warn!(
                "[Flux] FLUXBASE_SECRET_KEY not configured — using insecure default AES key. \
                 Set FLUXBASE_SECRET_KEY (exactly 32 bytes) before deploying to production."
            );
            "01234567890123456789012345678901".to_string()
        }
    };
    
    let key_bytes = key_str.as_bytes();
    
    if key_bytes.len() != 32 {
        return Err(EncryptionError("FLUXBASE_SECRET_KEY must be exactly 32 bytes".into()));
    }
    
    let key = aes_gcm::Key::<Aes256Gcm>::from_slice(key_bytes);
    Ok(Aes256Gcm::new(key))
}

pub fn encrypt_secret(value: &str) -> Result<String, EncryptionError> {
    let cipher = get_cipher()?;
    
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let ciphertext = cipher
        .encrypt(nonce, value.as_bytes())
        .map_err(|e| EncryptionError(format!("Encrypt failed: {}", e)))?;
    
    // The `aes-gcm` crate appends the Auth Tag (16 bytes) continuously to the ciphertext.
    // So `ciphertext` here is actually (ciphertext || tag).
    // We just store: base64(nonce):base64(ciphertext_with_tag)
    
    let b64_nonce = b64.encode(nonce_bytes);
    let b64_ciphertext = b64.encode(&ciphertext);
    
    Ok(format!("{}:{}", b64_nonce, b64_ciphertext))
}

/// # Errors
/// Returns `EncryptionError` when the encrypted string has wrong format,
/// bad base64, wrong nonce length, or the AEAD tag verification fails.
pub fn decrypt_secret(encrypted_str: &str) -> Result<String, EncryptionError> {
    let cipher = get_cipher()?;
    
    let parts: Vec<&str> = encrypted_str.split(':').collect();
    if parts.len() != 2 {
        return Err(EncryptionError("Invalid encrypted format".into()));
    }
    
    let nonce_bytes = b64
        .decode(parts[0])
        .map_err(|_| EncryptionError("Invalid nonce encoding".into()))?;
    
    if nonce_bytes.len() != 12 {
        return Err(EncryptionError("Invalid nonce length".into()));
    }
    
    let ciphertext_with_tag = b64
        .decode(parts[1])
        .map_err(|_| EncryptionError("Invalid ciphertext encoding".into()))?;
    
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    let plaintext_bytes = cipher
        .decrypt(nonce, ciphertext_with_tag.as_ref())
        .map_err(|e| EncryptionError(format!("Decrypt failed: {}", e)))?;
    
    String::from_utf8(plaintext_bytes)
        .map_err(|_| EncryptionError("Invalid UTF-8 in decrypted secret".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::Mutex;

    // Serialize all encryption tests — they all set FLUXBASE_SECRET_KEY.
    static ENCRYPT_ENV_LOCK: Mutex<()> = Mutex::new(());

    fn set_key(key: &str) {
        unsafe { env::set_var("FLUXBASE_SECRET_KEY", key) };
    }

    fn valid_32_byte_key() -> &'static str {
        "01234567890123456789012345678901"
    }

    // ── encrypt/decrypt roundtrip ─────────────────────────────────────────

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        set_key(valid_32_byte_key());
        let plain = "super-secret-value";
        let encrypted = encrypt_secret(plain).expect("encrypt failed");
        let decrypted = decrypt_secret(&encrypted).expect("decrypt failed");
        assert_eq!(plain, decrypted);
    }

    #[test]
    fn encrypt_produces_unique_ciphertexts() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        set_key(valid_32_byte_key());
        let a = encrypt_secret("hello").unwrap();
        let b = encrypt_secret("hello").unwrap();
        // Each call uses a fresh random nonce, so ciphertexts must differ.
        assert_ne!(a, b, "two encryptions of the same plaintext should differ");
    }

    #[test]
    fn roundtrip_empty_string() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        set_key(valid_32_byte_key());
        let enc = encrypt_secret("").unwrap();
        let dec = decrypt_secret(&enc).unwrap();
        assert_eq!(dec, "");
    }

    #[test]
    fn roundtrip_unicode() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        set_key(valid_32_byte_key());
        let plain = "🔑 café αβγ";
        let dec = decrypt_secret(&encrypt_secret(plain).unwrap()).unwrap();
        assert_eq!(dec, plain);
    }

    #[test]
    fn roundtrip_long_value() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        set_key(valid_32_byte_key());
        let plain = "x".repeat(4096);
        let dec = decrypt_secret(&encrypt_secret(&plain).unwrap()).unwrap();
        assert_eq!(dec, plain);
    }

    #[test]
    fn encrypted_format_has_two_parts() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        set_key(valid_32_byte_key());
        let enc = encrypt_secret("test").unwrap();
        let parts: Vec<&str> = enc.split(':').collect();
        assert_eq!(parts.len(), 2, "format must be base64_nonce:base64_ciphertext");
    }

    // ── decrypt error paths ────────────────────────────────────────────────

    #[test]
    fn decrypt_rejects_missing_colon() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        set_key(valid_32_byte_key());
        assert!(decrypt_secret("nocolonhere").is_err());
    }

    #[test]
    fn decrypt_rejects_bad_nonce_base64() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        set_key(valid_32_byte_key());
        assert!(decrypt_secret("!!!not_b64:validciphertext").is_err());
    }

    #[test]
    fn decrypt_rejects_wrong_nonce_length() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        set_key(valid_32_byte_key());
        // base64 of 6 bytes (need 12)
        use base64::{Engine, engine::general_purpose::STANDARD as b64};
        let short_nonce = b64.encode(&[0u8; 6]);
        let fake_ct = b64.encode(&[1u8; 32]);
        assert!(decrypt_secret(&format!("{}:{}", short_nonce, fake_ct)).is_err());
    }

    #[test]
    fn decrypt_rejects_tampered_ciphertext() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        set_key(valid_32_byte_key());
        let enc = encrypt_secret("sensitive").unwrap();
        let parts: Vec<&str> = enc.splitn(2, ':').collect();
        // Flip last byte of ciphertext → AEAD tag verification fails.
        use base64::{Engine, engine::general_purpose::STANDARD as b64};
        let mut ct_bytes = b64.decode(parts[1]).unwrap();
        let last = ct_bytes.len() - 1;
        ct_bytes[last] ^= 0xFF;
        let bad_enc = format!("{}:{}", parts[0], b64.encode(&ct_bytes));
        assert!(decrypt_secret(&bad_enc).is_err());
    }

    #[test]
    fn wrong_key_length_returns_error() {
        let _lock = ENCRYPT_ENV_LOCK.lock().unwrap();
        unsafe { env::set_var("FLUXBASE_SECRET_KEY", "tooshort") };
        assert!(encrypt_secret("value").is_err());
        // Restore valid key for other tests.
        set_key(valid_32_byte_key());
    }
}
