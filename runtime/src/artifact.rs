use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeArtifact {
    pub name: String,
    pub sha256: String,
    pub size_bytes: usize,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeSubmitRequest {
    pub artifact: RuntimeArtifact,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn build_artifact(name: impl Into<String>, code: impl Into<String>) -> RuntimeArtifact {
    let name = name.into();
    let code = code.into();
    let sha256 = sha256_hex(code.as_bytes());

    RuntimeArtifact {
        name,
        sha256,
        size_bytes: code.len(),
        code,
    }
}

pub fn build_artifact_from_file(path: impl AsRef<Path>) -> Result<RuntimeArtifact, String> {
    let path = path.as_ref();
    let code = fs::read_to_string(path)
        .map_err(|err| format!("failed to read '{}': {}", path.display(), err))?;

    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("invalid file name: '{}'", path.display()))?
        .to_string();

    Ok(build_artifact(name, code))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_deterministic() {
        let left = build_artifact("worker.js", "export default 1;");
        let right = build_artifact("worker.js", "export default 1;");

        assert_eq!(left.sha256, right.sha256);
        assert_eq!(left.size_bytes, right.size_bytes);
    }

    #[test]
    fn artifact_keeps_original_code() {
        let artifact = build_artifact("worker.js", "console.log('x');");
        assert_eq!(artifact.name, "worker.js");
        assert_eq!(artifact.code, "console.log('x');");
    }
}