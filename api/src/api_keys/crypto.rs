use rand::RngCore;
use sha2::{Sha256, Digest};
use base64::{engine::general_purpose, Engine as _};

pub fn generate_new_key() -> (String, String) {
    let mut random_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut random_bytes);
    
    let base64_encoded = general_purpose::URL_SAFE_NO_PAD.encode(&random_bytes);
    let plaintext_key = format!("flux_{}", base64_encoded);
    
    let hash = generate_hash(&plaintext_key);
    
    (plaintext_key, hash)
}

pub fn generate_hash(plaintext: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(plaintext.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}
