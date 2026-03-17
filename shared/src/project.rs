use serde::{Deserialize, Serialize};

pub const FLUX_PROJECT_VERSION: &str = "0.2";
pub const DEFAULT_PROJECT_CONFIG_PATH: &str = "flux.json";
pub const DEFAULT_ARTIFACT_PATH: &str = "./.flux/artifact.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FluxProjectConfig {
    pub flux_version: String,
    pub entry: String,
    pub artifact: String,
}

impl FluxProjectConfig {
    pub fn new(entry: impl Into<String>) -> Self {
        Self {
            flux_version: FLUX_PROJECT_VERSION.to_string(),
            entry: entry.into(),
            artifact: DEFAULT_ARTIFACT_PATH.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FluxBuildArtifact {
    pub flux_version: String,
    pub entry_specifier: String,
    pub route_name: String,
    pub graph_sha256: String,
    pub modules: Vec<ArtifactModule>,
    pub npm_packages: Vec<NpmPackageSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactModule {
    pub specifier: String,
    pub base_specifier: String,
    pub source_kind: ArtifactSourceKind,
    pub media_type: ArtifactMediaType,
    pub sha256: String,
    pub size_bytes: usize,
    pub source: String,
    pub dependencies: Vec<ArtifactDependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactSourceKind {
    Local,
    Remote,
    Npm,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactMediaType {
    JavaScript,
    Mjs,
    Jsx,
    TypeScript,
    Tsx,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArtifactDependency {
    pub kind: ArtifactDependencyKind,
    pub specifier: String,
    pub resolved_specifier: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactDependencyKind {
    StaticImport,
    DynamicImport,
    ReExport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NpmPackageSnapshot {
    pub specifier: String,
    pub fetched_url: String,
    pub root_sha256: String,
}