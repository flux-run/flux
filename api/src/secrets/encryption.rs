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
    let key_str = env::var("FLUXBASE_SECRET_KEY")
        .unwrap_or_else(|_| "01234567890123456789012345678901".to_string());
    
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
