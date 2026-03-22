use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, bail};
use deno_ast::swc::ast::{
    CallExpr, Callee, ExportAll, ImportDecl, Lit, NamedExport, TsImportEqualsDecl,
};
use deno_ast::swc::ecma_visit::{Visit, VisitWith};
use deno_ast::{MediaType, ParseParams, parse_module};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shared::project::{
    ArtifactDependency, ArtifactDependencyKind, ArtifactMediaType, ArtifactModule,
    ArtifactSourceKind, DEFAULT_ARTIFACT_PATH, DEFAULT_PROJECT_CONFIG_PATH, FLUX_PROJECT_VERSION,
    FluxBuildArtifact, FluxProjectConfig, NpmPackageSnapshot,
};
use url::Url;

const DEFAULT_ENTRY_FILE: &str = "src/index.ts";
const MODULE_FILE_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "json"];
const FLUX_PG_SPECIFIER: &str = "flux:pg";
const FLUX_REDIS_SPECIFIER: &str = "flux:redis";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityDiagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub specifier: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NpmCompatibility {
    Compatible,
    Warning,
    Incompatible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpmCompatibilityReport {
    pub specifier: String,
    pub status: NpmCompatibility,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct ProjectAnalysis {
    pub project_dir: PathBuf,
    pub entry_path: PathBuf,
    pub config: FluxProjectConfig,
    pub artifact_path: PathBuf,
    pub artifact: FluxBuildArtifact,
    pub diagnostics: Vec<CompatibilityDiagnostic>,
    pub npm_reports: Vec<NpmCompatibilityReport>,
}

#[derive(Debug, Clone)]
struct ImportEdge {
    kind: ArtifactDependencyKind,
    specifier: String,
}

#[derive(Debug, Clone)]
struct LoadedModule {
    specifier: String,
    base_specifier: String,
    source_kind: ArtifactSourceKind,
    media_type: ArtifactMediaType,
    source: String,
    npm_snapshot: Option<NpmPackageSnapshot>,
}

#[derive(Debug, Default, Deserialize)]
struct PackageManifest {
    main: Option<String>,
    module: Option<String>,
    dependencies: Option<BTreeMap<String, String>>,
    #[serde(rename = "devDependencies")]
    dev_dependencies: Option<BTreeMap<String, String>>,
    #[serde(rename = "optionalDependencies")]
    optional_dependencies: Option<BTreeMap<String, String>>,
    #[serde(rename = "peerDependencies")]
    peer_dependencies: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct DenoConfig {
    pub imports: Option<BTreeMap<String, String>>,
}

pub fn default_entry_path() -> PathBuf {
    PathBuf::from(DEFAULT_ENTRY_FILE)
}

pub fn project_config_path(project_dir: &Path) -> PathBuf {
    project_dir.join(DEFAULT_PROJECT_CONFIG_PATH)
}

pub fn artifact_path(project_dir: &Path, config: &FluxProjectConfig) -> PathBuf {
    project_dir.join(config.artifact.trim_start_matches("./"))
}

pub fn resolve_entry_path(entry_override: Option<&str>) -> Result<PathBuf> {
    if let Some(entry) = entry_override {
        return Ok(PathBuf::from(entry));
    }

    let config_path = project_config_path(Path::new("."));
    if config_path.exists() {
        let config = load_project_config(Path::new("."))?;
        return Ok(PathBuf::from(config.entry));
    }

    Ok(default_entry_path())
}

pub fn load_project_config(project_dir: &Path) -> Result<FluxProjectConfig> {
    let path = project_config_path(project_dir);
    let source =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&source).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn write_project_config(project_dir: &Path, config: &FluxProjectConfig) -> Result<()> {
    let path = project_config_path(project_dir);
    let json = serde_json::to_string_pretty(config).context("failed to serialize flux.json")?;
    fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))
}

pub fn default_project_config(entry_name: &str) -> FluxProjectConfig {
    FluxProjectConfig::new(format!("./{entry_name}"))
}

pub fn scaffold_project(project_dir: &Path, force: bool) -> Result<()> {
    let config_path = project_config_path(project_dir);
    let entry_path = project_dir.join(DEFAULT_ENTRY_FILE);
    let deno_config_path = project_dir.join("deno.json");

    if !force && (config_path.exists() || entry_path.exists() || deno_config_path.exists()) {
        bail!(
            "refusing to overwrite existing project files in {} (use --force)",
            project_dir.display()
        );
    }

    // Ensure all necessary directories exist
    for path in &[&config_path, &entry_path, &deno_config_path] {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }

    let config = default_project_config(DEFAULT_ENTRY_FILE);
    write_project_config(project_dir, &config)?;

    // 1. Write deno.json (Project Metadata & Imports)
    fs::write(
        &deno_config_path,
        concat!(
            "{\n",
            "  \"imports\": {\n",
            "    \"hono\": \"npm:hono@4.10.6\",\n",
            "    \"zod\": \"npm:zod@3.23.8\",\n",
            "    \"@hono/zod-validator\": \"npm:@hono/zod-validator@0.7.6\",\n",
            "    \"drizzle-orm\": \"npm:drizzle-orm@0.31.0\",\n",
            "    \"drizzle-zod\": \"npm:drizzle-zod@0.8.3\",\n",
            "    \"pg\": \"npm:pg@8.12.0\"\n",
            "  }\n",
            "}\n"
        ),
    )
    .with_context(|| format!("failed to write {}", deno_config_path.display()))?;

    // 2. Write src/index.ts (Combined Application Logic & Infrastructure Examples)
    fs::write(
        &entry_path,
        concat!(
            "import { Hono } from \"hono\";\n",
            "import { z } from \"zod\";\n",
            "import { zValidator } from \"@hono/zod-validator\";\n",
            "\n",
            "/**\n",
            " * Flux Hello World\n",
            " * Every request is recorded and can be replayed for instant debugging.\n",
            " */\n",
            "const app = new Hono();\n",
            "\n",
            "app.get(\"/\", (c) => {\n",
            "  return c.json({\n",
            "    message: \"Welcome to Flux!\",\n",
            "    timestamp: new Date().toISOString()\n",
            "  });\n",
            "});\n",
            "\n",
            "/**\n",
            " * Example: POST with Zod validation\n",
            " */\n",
            "const helloSchema = z.object({\n",
            "  name: z.string().min(1)\n",
            "});\n",
            "\n",
            "app.post(\"/hello\", zValidator(\"json\", helloSchema), (c) => {\n",
            "  const { name } = c.req.valid(\"json\");\n",
            "  return c.json({\n",
            "    message: `Hello, ${name}!`\n",
            "  });\n",
            "});\n",
            "\n",
            "/**\n",
            " * Example: Database and Data Model (Drizzle + Postgres)\n",
            " * \n",
            " * To use:\n",
            " * 1. Set DATABASE_URL in your environment.\n",
            " * 2. Uncomment the code below.\n",
            " */\n",
            "/*\n",
            "import { drizzle } from \"drizzle-orm/node-postgres\";\n",
            "import { pgTable, text, serial, boolean, timestamp } from \"drizzle-orm/pg-core\";\n",
            "import { createInsertSchema, createSelectSchema } from \"drizzle-zod\";\n",
            "import pg from \"flux:pg\";\n",
            "\n",
            "export const todos = pgTable(\"todos\", {\n",
            "  id: serial(\"id\").primaryKey(),\n",
            "  title: text(\"title\").notNull(),\n",
            "  completed: boolean(\"completed\").default(false),\n",
            "  createdAt: timestamp(\"created_at\").defaultNow(),\n",
            "});\n",
            "\n",
            "const pool = new pg.Pool({ connectionString: Deno.env.get(\"DATABASE_URL\") });\n",
            "export const db = drizzle(pool);\n",
            "\n",
            "export const insertTodoSchema = createInsertSchema(todos);\n",
            "export const selectTodoSchema = createSelectSchema(todos);\n",
            "export type Todo = z.infer<typeof selectTodoSchema>;\n",
            "\n",
            "// Example route using the database\n",
            "app.get(\"/todos\", async (c) => {\n",
            "  const allTodos = await db.select().from(todos);\n",
            "  return c.json(allTodos);\n",
            "});\n",
            "*/\n",
            "\n",
            "Deno.serve(app.fetch);\n"
        ),
    )
    .with_context(|| format!("failed to write {}", entry_path.display()))?;

    Ok(())
}


pub fn resolve_built_artifact(entry: &Path) -> Result<(FluxProjectConfig, PathBuf)> {
    let project_dir = entry.parent().unwrap_or(Path::new("."));
    let config = load_project_config(project_dir).with_context(|| {
        format!(
            "missing {} beside {}",
            DEFAULT_PROJECT_CONFIG_PATH,
            entry.display()
        )
    })?;
    let built_artifact = artifact_path(project_dir, &config);
    if !built_artifact.exists() {
        bail!(
            "built artifact not found: {}\nrun:\n  flux build {}",
            built_artifact.display(),
            entry.display()
        );
    }
    Ok((config, built_artifact))
}

pub fn write_artifact(artifact_path: &Path, artifact: &FluxBuildArtifact) -> Result<()> {
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(artifact).context("failed to serialize artifact")?;
    fs::write(artifact_path, json)
        .with_context(|| format!("failed to write {}", artifact_path.display()))
}

pub async fn analyze_project(entry: &Path) -> Result<ProjectAnalysis> {
    let entry_path = canonicalize_existing_path(entry)?;
    let project_dir = entry_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let entry_name = entry_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid entry path: {}", entry_path.display()))?
        .to_string();

    let mut config = if project_config_path(&project_dir).exists() {
        load_project_config(&project_dir)?
    } else {
        default_project_config(&entry_name)
    };
    config.entry = format!("./{entry_name}");
    if config.artifact.trim().is_empty() {
        config.artifact = DEFAULT_ARTIFACT_PATH.to_string();
    }

    let artifact_path = artifact_path(&project_dir, &config);
    let entry_specifier = file_url_string(&entry_path)?;
    let route_name = entry_path
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid entry file name: {}", entry_path.display()))?
        .to_string();

    let client = reqwest::Client::builder()
        .user_agent("flux-build/0.2")
        .build()
        .context("failed to build HTTP client")?;

    let mut graph = GraphBuilder::new(client);
    graph.project_id = config.project_id.clone();
    let artifact = graph.build(entry_specifier.clone(), route_name).await;

    Ok(ProjectAnalysis {
        project_dir,
        entry_path,
        config,
        artifact_path,
        artifact,
        diagnostics: graph.diagnostics(),
        npm_reports: graph.npm_reports(),
    })
}

pub fn watch_fingerprint(dir: &Path) -> Result<String> {
    let mut entries = Vec::new();
    collect_watch_entries(dir, &mut entries)?;
    entries.sort();

    let mut hasher = Sha256::new();
    for (path, modified) in entries {
        hasher.update(path.as_bytes());
        hasher.update(modified.as_bytes());
    }
    Ok(hex_hash(hasher.finalize()))
}

fn collect_watch_entries(dir: &Path, entries: &mut Vec<(String, String)>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }

    for child in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let child = child.with_context(|| format!("failed to read child in {}", dir.display()))?;
        let path = child.path();
        let file_name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default();

        if path.is_dir() {
            if matches!(file_name, ".git" | ".flux" | "node_modules" | "target")
                || file_name.starts_with('.')
            {
                continue;
            }
            collect_watch_entries(&path, entries)?;
            continue;
        }

        if !matches!(
            path.extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default(),
            "js" | "mjs" | "jsx" | "ts" | "tsx" | "json"
        ) {
            continue;
        }

        let modified = child
            .metadata()
            .and_then(|meta| meta.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let stamp = modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default();
        entries.push((
            path.to_string_lossy().into_owned(),
            format!("{}:{}", stamp.as_secs(), stamp.subsec_nanos()),
        ));
    }

    Ok(())
}

fn canonicalize_existing_path(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        bail!("entry file not found: {}", path.display());
    }
    path.canonicalize()
        .with_context(|| format!("failed to resolve {}", path.display()))
}

fn resolve_existing_module_path(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return canonicalize_existing_path(path);
    }

    if path.extension().is_none() {
        for extension in MODULE_FILE_EXTENSIONS {
            let candidate = path.with_extension(extension);
            if candidate.is_file() {
                return canonicalize_existing_path(&candidate);
            }
        }
    }

    if path.is_dir() {
        if let Some(package_entry) = resolve_package_entry(path)? {
            return Ok(package_entry);
        }
    }

    if path.extension().is_none() {
        for extension in MODULE_FILE_EXTENSIONS {
            let candidate = path.join(format!("index.{extension}"));
            if candidate.is_file() {
                return canonicalize_existing_path(&candidate);
            }
        }
    }

    bail!("entry file not found: {}", path.display())
}

fn read_package_manifest(path: &Path) -> Result<PackageManifest> {
    let source =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&source).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn read_deno_config(path: &Path) -> Result<DenoConfig> {
    let source =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&source).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn write_deno_config(path: &Path, config: &DenoConfig) -> Result<()> {
    let source = serde_json::to_string_pretty(config).context("failed to serialize deno.json")?;
    fs::write(path, source).with_context(|| format!("failed to write {}", path.display()))
}

fn resolve_package_entry(package_dir: &Path) -> Result<Option<PathBuf>> {
    let manifest_path = package_dir.join("package.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let manifest = read_package_manifest(&manifest_path)?;
    for candidate in [manifest.module.as_deref(), manifest.main.as_deref()]
        .into_iter()
        .flatten()
    {
        let resolved = resolve_existing_module_path(&package_dir.join(candidate))?;
        return Ok(Some(resolved));
    }

    for extension in MODULE_FILE_EXTENSIONS {
        let candidate = package_dir.join(format!("index.{extension}"));
        if candidate.is_file() {
            return Ok(Some(canonicalize_existing_path(&candidate)?));
        }
    }

    Ok(None)
}

fn bare_package_parts(specifier: &str) -> Option<(String, Option<String>)> {
    if !is_bare_package_import(specifier) {
        return None;
    }

    if let Some(stripped) = specifier.strip_prefix('@') {
        let mut parts = stripped.split('/');
        let scope = parts.next()?;
        let name = parts.next()?;
        let package_name = format!("@{scope}/{name}");
        let remainder = parts.collect::<Vec<_>>().join("/");
        return if remainder.is_empty() {
            Some((package_name, None))
        } else {
            Some((package_name, Some(remainder)))
        };
    }

    let mut parts = specifier.split('/');
    let package_name = parts.next()?.to_string();
    let remainder = parts.collect::<Vec<_>>().join("/");
    if remainder.is_empty() {
        Some((package_name, None))
    } else {
        Some((package_name, Some(remainder)))
    }
}

fn local_file_dependency<'a>(manifest: &'a PackageManifest, package_name: &str) -> Option<&'a str> {
    manifest
        .dependencies
        .as_ref()
        .and_then(|deps| deps.get(package_name))
        .or_else(|| {
            manifest
                .dev_dependencies
                .as_ref()
                .and_then(|deps| deps.get(package_name))
        })
        .or_else(|| {
            manifest
                .optional_dependencies
                .as_ref()
                .and_then(|deps| deps.get(package_name))
        })
        .or_else(|| {
            manifest
                .peer_dependencies
                .as_ref()
                .and_then(|deps| deps.get(package_name))
        })
        .map(String::as_str)
        .filter(|value| value.starts_with("file:"))
}

fn resolve_node_modules_bare_import(
    package_name: &str,
    subpath: Option<&str>,
    base_path: &Path,
) -> Result<Option<String>> {
    for ancestor in base_path.ancestors().skip(1) {
        let package_dir = ancestor.join("node_modules").join(package_name);
        if !package_dir.exists() {
            continue;
        }

        let target = if let Some(subpath) = subpath {
            resolve_existing_module_path(&package_dir.join(subpath))?
        } else if let Some(package_entry) = resolve_package_entry(&package_dir)? {
            package_entry
        } else {
            resolve_existing_module_path(&package_dir)?
        };

        return Ok(Some(file_url_string(&target)?));
    }

    Ok(None)
}

fn resolve_local_bare_import(specifier: &str, base_specifier: &str) -> Result<Option<String>> {
    let (package_name, subpath) = match bare_package_parts(specifier) {
        Some(parts) => parts,
        None => return Ok(None),
    };

    let base = Url::parse(base_specifier)
        .with_context(|| format!("invalid base specifier: {}", base_specifier))?;
    if base.scheme() != "file" {
        return Ok(None);
    }

    let base_path = base
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("invalid base path for bare import resolution"))?;

    for ancestor in base_path.ancestors().skip(1) {
        let deno_config_path = ancestor.join("deno.json");
        if deno_config_path.is_file() {
            let config = read_deno_config(&deno_config_path)?;
            if let Some(imports) = config.imports {
                if let Some(target) = imports.get(specifier) {
                    return resolve_dependency_specifier(target, &file_url_string(&deno_config_path)?).map(Some);
                }

                // Support prefix matching (e.g. "drizzle-orm/pg-core" matched by "drizzle-orm")
                for (key, target) in &imports {
                    if specifier.starts_with(key) && specifier.as_bytes().get(key.len()) == Some(&b'/') {
                        let subpath = &specifier[key.len()..];
                        let joined_target = format!("{}{}", target, subpath);
                        return resolve_dependency_specifier(&joined_target, &file_url_string(&deno_config_path)?).map(Some);
                    }
                }
            }
        }

        let manifest_path = ancestor.join("package.json");
        if !manifest_path.is_file() {
            continue;
        }

        let manifest = read_package_manifest(&manifest_path)?;
        let Some(file_target) = local_file_dependency(&manifest, &package_name) else {
            continue;
        };

        let package_dir = ancestor.join(file_target.trim_start_matches("file:"));
        let target = if let Some(subpath) = &subpath {
            resolve_existing_module_path(&package_dir.join(subpath))?
        } else if let Some(package_entry) = resolve_package_entry(&package_dir)? {
            package_entry
        } else {
            resolve_existing_module_path(&package_dir)?
        };

        return Ok(Some(file_url_string(&target)?));
    }

    resolve_node_modules_bare_import(&package_name, subpath.as_deref(), &base_path)
}

struct GraphBuilder {
    client: reqwest::Client,
    modules: BTreeMap<String, ArtifactModule>,
    diagnostics: Vec<CompatibilityDiagnostic>,
    npm_snapshots: BTreeMap<String, NpmPackageSnapshot>,
    npm_status: BTreeMap<String, NpmCompatibility>,
    project_id: Option<String>,
}

impl GraphBuilder {
    fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            modules: BTreeMap::new(),
            diagnostics: Vec::new(),
            npm_snapshots: BTreeMap::new(),
            npm_status: BTreeMap::new(),
            project_id: None,
        }
    }

    async fn build(&mut self, entry_specifier: String, route_name: String) -> FluxBuildArtifact {
        let mut queue = VecDeque::from([(entry_specifier.clone(), None::<String>)]);

        while let Some((specifier, npm_owner)) = queue.pop_front() {
            if self.modules.contains_key(&specifier) {
                continue;
            }

            let loaded = match self.load_module(&specifier).await {
                Ok(module) => module,
                Err(err) => {
                    self.push_diagnostic(
                        DiagnosticSeverity::Error,
                        "load_failed",
                        &specifier,
                        err.to_string(),
                    );
                    if let Some(owner) = npm_owner {
                        self.mark_npm(&owner, NpmCompatibility::Incompatible);
                    }
                    continue;
                }
            };

            if let Some(snapshot) = &loaded.npm_snapshot {
                self.npm_snapshots
                    .insert(snapshot.specifier.clone(), snapshot.clone());
                self.npm_status
                    .entry(snapshot.specifier.clone())
                    .or_insert(NpmCompatibility::Compatible);
            }

            self.collect_warning_diagnostics(
                &loaded.source,
                &loaded.specifier,
                npm_owner.as_deref(),
            );

            let mut dependencies = Vec::new();
            match analyze_imports(&loaded.specifier, &loaded.source, loaded.media_type.clone()) {
                Ok(imports) => {
                    for import in imports {
                        match resolve_dependency_specifier(
                            &import.specifier,
                            &loaded.base_specifier,
                        ) {
                            Ok(resolved) => {
                                if resolved.starts_with("node:") {
                                    self.push_diagnostic(
                                        DiagnosticSeverity::Error,
                                        "node_import",
                                        &loaded.specifier,
                                        format!(
                                            "node: imports are not supported: {}",
                                            import.specifier
                                        ),
                                    );
                                    if let Some(owner) = npm_owner.as_deref() {
                                        self.mark_npm(owner, NpmCompatibility::Incompatible);
                                    }
                                    continue;
                                }

                                dependencies.push(ArtifactDependency {
                                    kind: import.kind,
                                    specifier: import.specifier.clone(),
                                    resolved_specifier: resolved.clone(),
                                });

                                let next_owner = npm_owner.clone().or_else(|| {
                                    if resolved.starts_with("npm:") {
                                        Some(resolved.clone())
                                    } else {
                                        None
                                    }
                                });
                                queue.push_back((resolved, next_owner));
                            }
                            Err(err) => {
                                self.push_diagnostic(
                                    DiagnosticSeverity::Error,
                                    "unsupported_import",
                                    &loaded.specifier,
                                    format!("{} ({})", err, import.specifier),
                                );
                                if let Some(owner) = npm_owner.as_deref() {
                                    self.mark_npm(owner, NpmCompatibility::Incompatible);
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    self.push_diagnostic(
                        DiagnosticSeverity::Error,
                        "parse_failed",
                        &loaded.specifier,
                        err.to_string(),
                    );
                    if let Some(owner) = npm_owner.as_deref() {
                        self.mark_npm(owner, NpmCompatibility::Incompatible);
                    }
                }
            }

            dependencies.sort_by(|left, right| {
                left.resolved_specifier
                    .cmp(&right.resolved_specifier)
                    .then(left.specifier.cmp(&right.specifier))
            });

            self.modules.insert(
                loaded.specifier.clone(),
                ArtifactModule {
                    specifier: loaded.specifier,
                    base_specifier: loaded.base_specifier,
                    source_kind: loaded.source_kind,
                    media_type: loaded.media_type,
                    sha256: sha256_hex(loaded.source.as_bytes()),
                    size_bytes: loaded.source.len(),
                    source: loaded.source,
                    dependencies,
                },
            );
        }

        let npm_packages = self.npm_snapshots.values().cloned().collect::<Vec<_>>();
        let modules = self.modules.values().cloned().collect::<Vec<_>>();

        let graph_sha256 = {
            let canonical = serde_json::to_vec(&serde_json::json!({
                "entry_specifier": entry_specifier,
                "route_name": route_name,
                "modules": modules,
                "npm_packages": npm_packages,
            }))
            .unwrap_or_default();
            sha256_hex(&canonical)
        };

        FluxBuildArtifact {
            flux_version: FLUX_PROJECT_VERSION.to_string(),
            entry_specifier,
            route_name,
            project_id: self.project_id.clone(),
            graph_sha256,
            modules,
            npm_packages,
        }
    }

    async fn load_module(&self, specifier: &str) -> Result<LoadedModule> {
        if specifier == FLUX_PG_SPECIFIER {
            return Ok(LoadedModule {
                specifier: FLUX_PG_SPECIFIER.to_string(),
                base_specifier: FLUX_PG_SPECIFIER.to_string(),
                source_kind: ArtifactSourceKind::Local,
                media_type: ArtifactMediaType::JavaScript,
                npm_snapshot: None,
                source: flux_pg_module_source().to_string(),
            });
        }

        if specifier == FLUX_REDIS_SPECIFIER {
            return Ok(LoadedModule {
                specifier: FLUX_REDIS_SPECIFIER.to_string(),
                base_specifier: FLUX_REDIS_SPECIFIER.to_string(),
                source_kind: ArtifactSourceKind::Local,
                media_type: ArtifactMediaType::JavaScript,
                npm_snapshot: None,
                source: flux_redis_module_source().to_string(),
            });
        }

        if specifier.starts_with("npm:") {
            let fetch_url = format!("https://esm.sh/{}", specifier.trim_start_matches("npm:"));
            let response = self
                .client
                .get(&fetch_url)
                .send()
                .await
                .with_context(|| format!("failed to fetch {}", specifier))?;
            let final_url = response.url().to_string();
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string());
            let source = response
                .error_for_status()
                .with_context(|| format!("failed to fetch {}", specifier))?
                .text()
                .await
                .with_context(|| format!("failed to read body for {}", specifier))?;

            return Ok(LoadedModule {
                specifier: specifier.to_string(),
                base_specifier: final_url.clone(),
                source_kind: ArtifactSourceKind::Npm,
                media_type: infer_media_type(&final_url, content_type.as_deref()),
                npm_snapshot: Some(NpmPackageSnapshot {
                    specifier: specifier.to_string(),
                    fetched_url: final_url,
                    root_sha256: sha256_hex(source.as_bytes()),
                }),
                source,
            });
        }

        if specifier.starts_with("https://") {
            let response = self
                .client
                .get(specifier)
                .send()
                .await
                .with_context(|| format!("failed to fetch {}", specifier))?;
            let final_url = response.url().to_string();
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(|value| value.to_string());
            let source = response
                .error_for_status()
                .with_context(|| format!("failed to fetch {}", specifier))?
                .text()
                .await
                .with_context(|| format!("failed to read body for {}", specifier))?;

            return Ok(LoadedModule {
                specifier: specifier.to_string(),
                base_specifier: final_url.clone(),
                source_kind: ArtifactSourceKind::Remote,
                media_type: infer_media_type(&final_url, content_type.as_deref()),
                npm_snapshot: None,
                source,
            });
        }

        let file_url = Url::parse(specifier)
            .with_context(|| format!("invalid local file specifier: {}", specifier))?;
        let path = file_url
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("only file URLs are supported for local modules"))?;
        let canonical = canonicalize_existing_path(&path)?;
        let canonical_specifier = file_url_string(&canonical)?;
        let source = fs::read_to_string(&canonical)
            .with_context(|| format!("failed to read {}", canonical.display()))?;

        Ok(LoadedModule {
            specifier: canonical_specifier.clone(),
            base_specifier: canonical_specifier.clone(),
            source_kind: ArtifactSourceKind::Local,
            media_type: infer_media_type(&canonical_specifier, None),
            npm_snapshot: None,
            source,
        })
    }

    fn collect_warning_diagnostics(
        &mut self,
        source: &str,
        specifier: &str,
        npm_owner: Option<&str>,
    ) {
        for global in ["Buffer", "process", "__dirname", "__filename", "global"] {
            if contains_identifier(source, global) {
                self.push_diagnostic(
                    DiagnosticSeverity::Warning,
                    "unsupported_global",
                    specifier,
                    format!("{} may not be available in Flux runtime", global),
                );
                if let Some(owner) = npm_owner {
                    self.mark_npm(owner, NpmCompatibility::Warning);
                }
            }
        }

        for web_api in [
            "window",
            "document",
            "navigator",
            "localStorage",
            "sessionStorage",
            "Worker",
        ] {
            if contains_identifier(source, web_api) {
                self.push_diagnostic(
                    DiagnosticSeverity::Warning,
                    "unsupported_web_api",
                    specifier,
                    format!(
                        "{} is not part of Flux's supported web API surface",
                        web_api
                    ),
                );
                if let Some(owner) = npm_owner {
                    self.mark_npm(owner, NpmCompatibility::Warning);
                }
            }
        }
    }

    fn mark_npm(&mut self, specifier: &str, status: NpmCompatibility) {
        let entry = self
            .npm_status
            .entry(specifier.to_string())
            .or_insert(NpmCompatibility::Compatible);
        *entry = match (*entry, status) {
            (NpmCompatibility::Incompatible, _) | (_, NpmCompatibility::Incompatible) => {
                NpmCompatibility::Incompatible
            }
            (NpmCompatibility::Warning, _) | (_, NpmCompatibility::Warning) => {
                NpmCompatibility::Warning
            }
            _ => NpmCompatibility::Compatible,
        };
    }

    fn push_diagnostic(
        &mut self,
        severity: DiagnosticSeverity,
        code: &str,
        specifier: &str,
        message: String,
    ) {
        self.diagnostics.push(CompatibilityDiagnostic {
            severity,
            code: code.to_string(),
            specifier: specifier.to_string(),
            message,
        });
    }

    fn diagnostics(&self) -> Vec<CompatibilityDiagnostic> {
        let mut diagnostics = self.diagnostics.clone();
        diagnostics.sort_by(|left, right| {
            left.specifier
                .cmp(&right.specifier)
                .then(left.code.cmp(&right.code))
                .then(left.message.cmp(&right.message))
        });
        diagnostics
    }

    fn npm_reports(&self) -> Vec<NpmCompatibilityReport> {
        self.npm_snapshots
            .keys()
            .map(|specifier| {
                let status = *self
                    .npm_status
                    .get(specifier)
                    .unwrap_or(&NpmCompatibility::Compatible);
                let reason = match status {
                    NpmCompatibility::Compatible => {
                        "no unsupported node or web runtime dependencies detected"
                    }
                    NpmCompatibility::Warning => {
                        "package graph uses globals or web APIs that may need review"
                    }
                    NpmCompatibility::Incompatible => {
                        "package graph requires unsupported imports or CommonJS behavior"
                    }
                };
                NpmCompatibilityReport {
                    specifier: specifier.clone(),
                    status,
                    reason: reason.to_string(),
                }
            })
            .collect()
    }
}

fn analyze_imports(
    specifier: &str,
    source: &str,
    media_type: ArtifactMediaType,
) -> Result<Vec<ImportEdge>> {
    if media_type == ArtifactMediaType::Json {
        return Ok(Vec::new());
    }

    let mut parser_media_type = match media_type {
        ArtifactMediaType::JavaScript => MediaType::JavaScript,
        ArtifactMediaType::Mjs => MediaType::Mjs,
        ArtifactMediaType::Jsx => MediaType::Jsx,
        ArtifactMediaType::TypeScript => MediaType::TypeScript,
        ArtifactMediaType::Tsx => MediaType::Tsx,
        ArtifactMediaType::Json => MediaType::Json,
    };
    if specifier.ends_with(".mts") {
        parser_media_type = MediaType::Mts;
    }

    let parsed = parse_module(ParseParams {
        specifier: Url::parse(specifier)
            .with_context(|| format!("invalid module specifier: {}", specifier))?,
        text: source.into(),
        media_type: parser_media_type,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })
    .with_context(|| format!("failed to parse {}", specifier))?;

    let mut visitor = ImportCollector::default();
    parsed.program_ref().visit_with(&mut visitor);

    if visitor.uses_require {
        bail!("require() is not supported");
    }
    if visitor.uses_ts_import_equals {
        bail!("TypeScript import-equals syntax is not supported");
    }
    if !visitor.invalid_dynamic_imports.is_empty() {
        bail!("dynamic import() must use a string literal specifier");
    }

    let mut imports = visitor.imports;
    imports.sort_by(|left, right| left.specifier.cmp(&right.specifier));
    imports.dedup_by(|left, right| left.kind == right.kind && left.specifier == right.specifier);
    Ok(imports)
}

#[derive(Default)]
struct ImportCollector {
    imports: Vec<ImportEdge>,
    invalid_dynamic_imports: Vec<String>,
    uses_require: bool,
    uses_ts_import_equals: bool,
}

impl Visit for ImportCollector {
    fn visit_import_decl(&mut self, node: &ImportDecl) {
        if !node.type_only {
            self.imports.push(ImportEdge {
                kind: ArtifactDependencyKind::StaticImport,
                specifier: atom_to_string(&node.src.value),
            });
        }
        node.visit_children_with(self);
    }

    fn visit_named_export(&mut self, node: &NamedExport) {
        if !node.type_only {
            if let Some(src) = &node.src {
                self.imports.push(ImportEdge {
                    kind: ArtifactDependencyKind::ReExport,
                    specifier: atom_to_string(&src.value),
                });
            }
        }
        node.visit_children_with(self);
    }

    fn visit_export_all(&mut self, node: &ExportAll) {
        self.imports.push(ImportEdge {
            kind: ArtifactDependencyKind::ReExport,
            specifier: atom_to_string(&node.src.value),
        });
        node.visit_children_with(self);
    }

    fn visit_ts_import_equals_decl(&mut self, _node: &TsImportEqualsDecl) {
        self.uses_ts_import_equals = true;
    }

    fn visit_call_expr(&mut self, node: &CallExpr) {
        if let Callee::Expr(expr) = &node.callee {
            if let deno_ast::swc::ast::Expr::Ident(ident) = expr.as_ref() {
                if ident.sym == *"require" {
                    self.uses_require = true;
                }
            }
        }

        if matches!(node.callee, Callee::Import(_)) {
            if let Some(first_arg) = node.args.first() {
                match first_arg.expr.as_ref() {
                    deno_ast::swc::ast::Expr::Lit(Lit::Str(value)) => {
                        self.imports.push(ImportEdge {
                            kind: ArtifactDependencyKind::DynamicImport,
                            specifier: atom_to_string(&value.value),
                        })
                    }
                    deno_ast::swc::ast::Expr::Tpl(template) if template.exprs.is_empty() => {
                        if let Some(quasi) = template.quasis.first() {
                            self.imports.push(ImportEdge {
                                kind: ArtifactDependencyKind::DynamicImport,
                                specifier: quasi.raw.to_string(),
                            });
                        }
                    }
                    _ => self
                        .invalid_dynamic_imports
                        .push("dynamic import".to_string()),
                }
            }
        }

        node.visit_children_with(self);
    }
}

fn resolve_dependency_specifier(specifier: &str, base_specifier: &str) -> Result<String> {
    if specifier == "pg" || specifier == FLUX_PG_SPECIFIER {
        return Ok(FLUX_PG_SPECIFIER.to_string());
    }
    if specifier == "redis" || specifier == FLUX_REDIS_SPECIFIER {
        return Ok(FLUX_REDIS_SPECIFIER.to_string());
    }

    if specifier.starts_with("node:") {
        return Ok(specifier.to_string());
    }
    if specifier.starts_with("http://") {
        bail!("http imports are not supported; use https URLs instead");
    }
    if specifier.starts_with("https://") {
        return Ok(specifier.to_string());
    }
    if specifier.starts_with("npm:") {
        return Ok(format!(
            "https://esm.sh/{}",
            specifier.trim_start_matches("npm:")
        ));
    }
    if specifier.starts_with("file://") {
        let url = Url::parse(specifier).context("invalid file URL import")?;
        let path = url
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("invalid file URL import"))?;
        let canonical = resolve_existing_module_path(&path)?;
        return file_url_string(&canonical);
    }
    if is_bare_package_import(specifier) {
        if let Some(local) = resolve_local_bare_import(specifier, base_specifier)? {
            return Ok(local);
        }
        return Ok(format!("https://esm.sh/{specifier}"));
    }

    let base = Url::parse(base_specifier)
        .with_context(|| format!("invalid base specifier: {}", base_specifier))?;
    let joined = base
        .join(specifier)
        .with_context(|| format!("failed to resolve {} from {}", specifier, base_specifier))?;

    if joined.scheme() == "file" {
        let path = joined
            .to_file_path()
            .map_err(|_| anyhow::anyhow!("invalid resolved local path"))?;
        let canonical = resolve_existing_module_path(&path)?;
        return file_url_string(&canonical);
    }

    Ok(joined.to_string())
}

fn file_url_string(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map_err(|_| anyhow::anyhow!("failed to convert {} to file URL", path.display()))
        .map(|url| url.to_string())
}

fn infer_media_type(specifier: &str, content_type: Option<&str>) -> ArtifactMediaType {
    let lower = specifier.to_ascii_lowercase();
    if lower.ends_with(".tsx") {
        ArtifactMediaType::Tsx
    } else if lower.ends_with(".ts") || lower.ends_with(".mts") {
        ArtifactMediaType::TypeScript
    } else if lower.ends_with(".jsx") {
        ArtifactMediaType::Jsx
    } else if lower.ends_with(".mjs") {
        ArtifactMediaType::Mjs
    } else if lower.ends_with(".json") {
        ArtifactMediaType::Json
    } else if let Some(content_type) = content_type {
        if content_type.contains("application/json") {
            ArtifactMediaType::Json
        } else if content_type.contains("typescript") {
            ArtifactMediaType::TypeScript
        } else {
            ArtifactMediaType::JavaScript
        }
    } else {
        ArtifactMediaType::JavaScript
    }
}

fn is_bare_package_import(specifier: &str) -> bool {
    !specifier.is_empty()
        && !specifier.starts_with("./")
        && !specifier.starts_with("../")
        && !specifier.starts_with('/')
        && !specifier.starts_with("file://")
        && !specifier.starts_with("https://")
        && !specifier.starts_with("http://")
        && !specifier.starts_with("npm:")
        && !specifier.starts_with("node:")
        && !specifier.starts_with("flux:")
}

fn contains_identifier(source: &str, needle: &str) -> bool {
    let bytes = source.as_bytes();
    let needle_bytes = needle.as_bytes();
    if needle_bytes.is_empty() || needle_bytes.len() > bytes.len() {
        return false;
    }

    for index in 0..=bytes.len() - needle_bytes.len() {
        if &bytes[index..index + needle_bytes.len()] != needle_bytes {
            continue;
        }
        let left_ok = index == 0 || !is_identifier_byte(bytes[index - 1]);
        let right_index = index + needle_bytes.len();
        let right_ok = right_index == bytes.len() || !is_identifier_byte(bytes[right_index]);
        if left_ok && right_ok {
            return true;
        }
    }
    false
}

fn flux_pg_module_source() -> &'static str {
    r#"
const __fluxPg = globalThis.Flux?.postgres;

if (!__fluxPg || !__fluxPg.NodePgPool || !__fluxPg.nodePgTypes) {
    throw new Error("Flux pg shim is unavailable");
}

function __fluxPgNormalizeConfig(config = {}) {
    const ssl = config?.ssl;
    return {
        connectionString: String(config?.connectionString ?? ""),
        tls: !!ssl,
        caCertPem: ssl && typeof ssl === "object" && ssl.ca != null ? String(ssl.ca) : null,
    };
}

class DatabaseError extends Error {
    constructor(message, details = {}) {
        super(message);
        this.name = "DatabaseError";
        Object.assign(this, details);
    }
}

function __fluxPgWrapDatabaseError(error) {
    if (error instanceof DatabaseError) {
        return error;
    }
    if (error && typeof error === "object" && (error.name === "DatabaseError" || error.code != null)) {
        return new DatabaseError(String(error.message ?? "postgres query failed"), error);
    }
    return error;
}

async function __fluxPgWrapQueryError(runQuery) {
    try {
        return await runQuery();
    } catch (error) {
        throw __fluxPgWrapDatabaseError(error);
    }
}

class Client {
    constructor(config = {}) {
        this._config = __fluxPgNormalizeConfig(config);
        this._inner = null;
        this._released = false;
    }

    static __fromInner(inner, config = {}) {
        const client = new Client(config);
        client._inner = inner;
        return client;
    }

    async connect() {
        if (this._released) {
            throw new Error("pg Client has already been closed");
        }
        if (this._inner) {
            return this;
        }
        const pool = new __fluxPg.NodePgPool(this._config);
        this._pool = pool;
        this._inner = await pool.connect();
        return this;
    }

    async query(queryOrConfig, values = undefined) {
        if (!this._inner) {
            await this.connect();
        }
        return __fluxPgWrapQueryError(() => this._inner.query(queryOrConfig, values));
    }

    async release() {
        if (this._released) {
            return undefined;
        }
        this._released = true;
        if (this._inner) {
            await this._inner.release();
            this._inner = null;
        }
        if (this._pool) {
            await this._pool.end();
            this._pool = null;
        }
        return undefined;
    }

    async end() {
        return this.release();
    }
}

class Pool {
    constructor(config = {}) {
        this.options = { ...config };
        this._config = __fluxPgNormalizeConfig(config);
        this._inner = new __fluxPg.NodePgPool(this._config);
    }

    async query(queryOrConfig, values = undefined) {
        return __fluxPgWrapQueryError(() => this._inner.query(queryOrConfig, values));
    }

    async connect() {
        const inner = await this._inner.connect();
        return Client.__fromInner(inner, this.options);
    }

    async end() {
        return this._inner.end();
    }
}

const types = __fluxPg.nodePgTypes;
const defaults = {};
const native = null;

export { Client, DatabaseError, Pool, defaults, native, types };
export default { Client, DatabaseError, Pool, defaults, native, types };
"#
}

fn flux_redis_module_source() -> &'static str {
    r#"
const __fluxRedis = globalThis.Flux?.redis;
if (!__fluxRedis || !__fluxRedis.createClient) {
    throw new Error("Flux redis shim is unavailable");
}
export const createClient = __fluxRedis.createClient;
export default __fluxRedis;
"#
}

fn is_identifier_byte(value: u8) -> bool {
    value.is_ascii_alphanumeric() || value == b'_' || value == b'$'
}

fn atom_to_string(value: &deno_ast::swc::atoms::Wtf8Atom) -> String {
    value.to_string_lossy().into_owned()
}

pub fn has_errors(diagnostics: &[CompatibilityDiagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_hash(hasher.finalize())
}

fn hex_hash(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|value| format!("{:02x}", value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_fingerprint_changes_for_nested_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).expect("create src");
        fs::write(src_dir.join("index.ts"), "export default 1;\n").expect("write index");

        let before = watch_fingerprint(temp.path()).expect("fingerprint before");
        std::thread::sleep(std::time::Duration::from_millis(5));
        fs::write(src_dir.join("index.ts"), "export default 2;\n").expect("rewrite index");
        let after = watch_fingerprint(temp.path()).expect("fingerprint after");

        assert_ne!(before, after);
    }

    #[tokio::test]
    async fn analysis_is_stable_for_same_local_graph() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("index.ts"),
            "import { value } from './dep.ts';\nexport default async function () { return value; }\n",
        )
        .expect("write entry");
        fs::write(temp.path().join("dep.ts"), "export const value = 42;\n").expect("write dep");

        let first = analyze_project(&temp.path().join("index.ts"))
            .await
            .expect("first analysis");
        let second = analyze_project(&temp.path().join("index.ts"))
            .await
            .expect("second analysis");

        assert_eq!(first.artifact.graph_sha256, second.artifact.graph_sha256);
        assert_eq!(first.artifact.modules, second.artifact.modules);
    }

    #[tokio::test]
    async fn analysis_flags_node_and_require_usage() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("index.ts"),
            "import fs from 'node:fs';\nconst value = require('x');\nexport default value;\n",
        )
        .expect("write entry");

        let analysis = analyze_project(&temp.path().join("index.ts"))
            .await
            .expect("analysis");

        assert!(has_errors(&analysis.diagnostics));
        assert!(
            analysis
                .diagnostics
                .iter()
                .any(|diag| diag.code == "parse_failed" || diag.code == "node_import")
        );
    }

    #[tokio::test]
    async fn analysis_resolves_bare_imports_from_local_node_modules() {
        let temp = tempfile::tempdir().expect("tempdir");
        let node_modules = temp.path().join("node_modules").join("local-pkg");
        fs::create_dir_all(&node_modules).expect("create node_modules package");
        fs::write(
            node_modules.join("package.json"),
            r#"{
  "name": "local-pkg",
  "module": "./index.js"
}
"#,
        )
        .expect("write local package manifest");
        fs::write(node_modules.join("index.js"), "export const value = 42;\n")
            .expect("write local package entry");
        fs::write(
            temp.path().join("index.ts"),
            "import { value } from 'local-pkg';\nexport default value;\n",
        )
        .expect("write entry");

        let analysis = analyze_project(&temp.path().join("index.ts"))
            .await
            .expect("analysis");

        assert!(!has_errors(&analysis.diagnostics));
        assert!(analysis.artifact.modules.iter().any(|module| {
            module
                .specifier
                .contains("/node_modules/local-pkg/index.js")
        }));
        assert!(analysis.npm_reports.is_empty());
    }

    #[test]
    fn resolve_built_artifact_requires_existing_artifact() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(
            temp.path().join("flux.json"),
            serde_json::to_string_pretty(&default_project_config("index.ts")).unwrap(),
        )
        .expect("write config");
        fs::write(temp.path().join("index.ts"), "export default 1;\n").expect("write entry");

        let error = resolve_built_artifact(&temp.path().join("index.ts"))
            .expect_err("artifact should be required");
        assert!(error.to_string().contains("built artifact not found"));
    }
}
