use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shared::project::FluxBuildArtifact;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InlineRuntimeArtifact {
    pub name: String,
    pub sha256: String,
    pub size_bytes: usize,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RuntimeArtifact {
    Inline(InlineRuntimeArtifact),
    Built(FluxBuildArtifact),
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

    RuntimeArtifact::Inline(InlineRuntimeArtifact {
        name,
        sha256,
        size_bytes: code.len(),
        code,
    })
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

pub fn load_built_artifact_from_file(path: impl AsRef<Path>) -> Result<RuntimeArtifact, String> {
    let path = path.as_ref();
    let raw = fs::read_to_string(path)
        .map_err(|err| format!("failed to read '{}': {}", path.display(), err))?;
    let artifact: FluxBuildArtifact = serde_json::from_str(&raw)
        .map_err(|err| format!("failed to parse '{}': {}", path.display(), err))?;
    Ok(RuntimeArtifact::Built(artifact))
}

impl RuntimeArtifact {
    pub fn code_version(&self) -> &str {
        match self {
            RuntimeArtifact::Inline(artifact) => &artifact.sha256,
            RuntimeArtifact::Built(artifact) => &artifact.graph_sha256,
        }
    }

    pub fn size_bytes(&self) -> usize {
        match self {
            RuntimeArtifact::Inline(artifact) => artifact.size_bytes,
            RuntimeArtifact::Built(artifact) => artifact.modules.iter().map(|module| module.size_bytes).sum(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_deterministic() {
        let left = build_artifact("worker.js", "export default 1;");
        let right = build_artifact("worker.js", "export default 1;");

        assert_eq!(left.code_version(), right.code_version());
        assert_eq!(left.size_bytes(), right.size_bytes());
    }

    #[test]
    fn artifact_keeps_original_code() {
        let artifact = build_artifact("worker.js", "console.log('x');");
        match artifact {
            RuntimeArtifact::Inline(artifact) => {
                assert_eq!(artifact.name, "worker.js");
                assert_eq!(artifact.code, "console.log('x');");
            }
            RuntimeArtifact::Built(_) => panic!("expected inline artifact"),
        }
    }
}