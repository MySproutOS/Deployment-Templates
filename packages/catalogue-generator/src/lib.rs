use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA_VERSION: u32 = 1;
const OCI_PREFIX: &str = "ghcr.io/mysproutos/";

#[derive(Debug, Clone)]
pub struct GenerateOptions {
    pub apps_dir: PathBuf,
    pub plugin_lock: PathBuf,
    pub output: PathBuf,
    pub provenance_output: PathBuf,
    pub manifest_schema: PathBuf,
    pub catalogue_schema: PathBuf,
    pub provenance_schema: PathBuf,
    pub protocol_schema_dir: PathBuf,
    pub protocol_source_dir: PathBuf,
    pub plugin_source_dir: PathBuf,
    pub e2e_proof_dir: PathBuf,
    pub source_repository: String,
    pub source_workflow: String,
    pub source_ref: String,
    pub source_commit: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginLock {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    plugins: BTreeMap<String, LockedPlugin>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LockedPlugin {
    artifact: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceManifest {
    schema_version: u32,
    id: String,
    name: String,
    pitch: String,
    description_md: String,
    homepage: Option<String>,
    repository: Repository,
    license: String,
    platform: Platform,
    readiness: SourceReadiness,
    plugin: SourcePlugin,
    deployment: Deployment,
    services: Vec<ManagedService>,
    user_inputs: Vec<UserInput>,
    generated_inputs: Vec<GeneratedInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourcePlugin {
    protocol_version: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceReadiness {
    status: ReadinessStatus,
    blocked_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Repository {
    url: String,
    commit: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Platform {
    Web,
    Android,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Readiness {
    status: ReadinessStatus,
    blocked_reasons: Vec<String>,
    e2e_evidence: Option<E2eEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct E2eEvidence {
    workflow_run_url: String,
    tested_at: String,
    upstream_commit: String,
    plugin_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReadinessStatus {
    Blocked,
    Live,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Deployment {
    preset: String,
    runtime: String,
    architecture: Architecture,
    migration: Option<Migration>,
    required_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Architecture {
    Arm64,
    X86_64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Migration {
    kind: MigrationKind,
    path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MigrationKind {
    Artifact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedService {
    key: String,
    kind: ServiceKind,
    bindings: Vec<EnvironmentBinding>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ServiceKind {
    Postgres,
    Valkey,
    Elasticsearch,
    ObjectStorage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentBinding {
    environment: String,
    output: ServiceOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ServiceOutput {
    ConnectionUrl,
    Endpoint,
    Username,
    Password,
    Region,
    Bucket,
    AccessKeyId,
    SecretAccessKey,
    ForcePathStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UserInput {
    key: String,
    #[serde(rename = "type")]
    input_type: UserInputType,
    environment: String,
    required: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UserInputType {
    String,
    Url,
    Integer,
    Boolean,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratedInput {
    key: String,
    generator: Generator,
    bytes: u16,
    environment: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Generator {
    RandomBase64url,
}

#[derive(Debug, Clone, Serialize)]
struct AppManifest {
    schema_version: u32,
    id: String,
    name: String,
    pitch: String,
    description_md: String,
    homepage: Option<String>,
    repository: Repository,
    license: String,
    platform: Platform,
    readiness: Readiness,
    plugin: Plugin,
    deployment: Deployment,
    services: Vec<ManagedService>,
    user_inputs: Vec<UserInput>,
    generated_inputs: Vec<GeneratedInput>,
}

#[derive(Debug, Clone, Serialize)]
struct Plugin {
    repository: String,
    digest: String,
    protocol_version: u32,
}

#[derive(Debug, Serialize)]
struct Catalogue {
    schema_version: u32,
    generated_from_commit: String,
    apps: Vec<AppManifest>,
}

#[derive(Debug, Serialize)]
struct Provenance {
    schema_version: u32,
    repository: String,
    workflow: String,
    #[serde(rename = "ref")]
    source_ref: String,
    source_commit: String,
    subject: Subject,
    materials: Vec<Material>,
}

#[derive(Debug, Serialize)]
struct Subject {
    kind: &'static str,
    name: String,
    digest: String,
}

#[derive(Debug, Clone, Serialize)]
struct Material {
    uri: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct E2eProof {
    schema_version: u32,
    app_id: String,
    upstream_commit: String,
    plugin_digest: String,
    preset: String,
    required_capabilities: Vec<String>,
    passed: bool,
    workflow_run_url: String,
    tested_at: String,
    evidence_path: String,
    evidence_digest: String,
}

pub fn generate(options: &GenerateOptions) -> Result<()> {
    validate_provenance_identity(options)?;

    let lock_bytes = read_regular_file(&options.plugin_lock)?;
    let lock: PluginLock = parse_json(&options.plugin_lock, &lock_bytes)?;
    ensure!(
        lock.schema_version == SCHEMA_VERSION,
        "{}: unsupported plugin lock schemaVersion {}",
        options.plugin_lock.display(),
        lock.schema_version
    );
    ensure!(!lock.plugins.is_empty(), "plugin lock contains no plugins");

    let sources = discover_sources(&options.apps_dir)?;
    ensure!(
        !sources.is_empty(),
        "no apps/*/manifest-source.json files found"
    );

    let manifest_schema = load_schema(&options.manifest_schema)?;
    let catalogue_schema = load_schema(&options.catalogue_schema)?;
    let provenance_schema = load_schema(&options.provenance_schema)?;

    let mut seen_ids = BTreeMap::<String, PathBuf>::new();
    let mut apps = Vec::with_capacity(sources.len());
    let mut materials = Vec::new();

    materials.push(Material {
        uri: "catalogue/plugin-lock.json".into(),
        digest: sha256(&lock_bytes),
    });

    for source_path in &sources {
        let source_bytes = read_regular_file(source_path)?;
        let mut source: SourceManifest = parse_json(source_path, &source_bytes)?;
        validate_source(source_path, &source)?;

        if let Some(first_path) = seen_ids.insert(source.id.clone(), source_path.clone()) {
            bail!(
                "duplicate app id '{}': {} and {}",
                source.id,
                first_path.display(),
                source_path.display()
            );
        }

        let directory_id = source_path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("{}: invalid app directory", source_path.display()))?;
        ensure!(
            directory_id == source.id,
            "{}: manifest id '{}' must match app directory '{}'",
            source_path.display(),
            source.id,
            directory_id
        );

        sort_source(&mut source);
        let locked = lock.plugins.get(&source.id).ok_or_else(|| {
            anyhow!(
                "{}: missing plugin lock for '{}'",
                source_path.display(),
                source.id
            )
        })?;
        let (artifact, digest) = validate_locked_artifact(&source.id, &locked.artifact)?;

        let e2e_evidence = if source.readiness.status == ReadinessStatus::Live {
            let (evidence, evidence_materials) = validate_e2e_proof(options, &source, &digest)?;
            materials.extend(evidence_materials);
            Some(evidence)
        } else {
            None
        };

        materials.push(Material {
            uri: format!("apps/{}/manifest-source.json", source.id),
            digest: sha256(&source_bytes),
        });
        materials.push(Material {
            uri: artifact.clone(),
            digest: digest.clone(),
        });

        let manifest = AppManifest {
            schema_version: source.schema_version,
            id: source.id,
            name: source.name,
            pitch: source.pitch,
            description_md: source.description_md,
            homepage: source.homepage,
            repository: source.repository,
            license: source.license,
            platform: source.platform,
            readiness: Readiness {
                status: source.readiness.status,
                blocked_reasons: source.readiness.blocked_reasons,
                e2e_evidence,
            },
            plugin: Plugin {
                repository: artifact
                    .split_once('@')
                    .map(|(repository, _)| repository)
                    .expect("validated locked artifact")
                    .to_owned(),
                digest,
                protocol_version: source.plugin.protocol_version,
            },
            deployment: source.deployment,
            services: source.services,
            user_inputs: source.user_inputs,
            generated_inputs: source.generated_inputs,
        };
        validate_against_schema(
            &options.manifest_schema,
            &manifest_schema,
            &serde_json::to_value(&manifest)?,
        )?;
        apps.push(manifest);
    }

    let locked_ids = lock.plugins.keys().cloned().collect::<BTreeSet<_>>();
    let manifest_ids = seen_ids.keys().cloned().collect::<BTreeSet<_>>();
    ensure!(
        locked_ids == manifest_ids,
        "plugin lock IDs do not exactly match source manifest IDs (lock: {:?}; manifests: {:?})",
        locked_ids,
        manifest_ids
    );

    apps.sort_by(|left, right| left.id.cmp(&right.id));
    let catalogue = Catalogue {
        schema_version: SCHEMA_VERSION,
        generated_from_commit: options.source_commit.clone(),
        apps,
    };
    let catalogue_value = serde_json::to_value(&catalogue)?;
    validate_against_schema(
        &options.catalogue_schema,
        &catalogue_schema_without_external_manifest_ref(catalogue_schema)?,
        &catalogue_value,
    )?;
    let catalogue_bytes = canonical_json(&catalogue_value)?;

    add_schema_materials(options, &mut materials)?;
    add_source_tree_materials(options, &manifest_ids, &mut materials)?;
    materials.sort_by(|left, right| {
        left.uri
            .cmp(&right.uri)
            .then_with(|| left.digest.cmp(&right.digest))
    });
    ensure!(
        materials.windows(2).all(|pair| pair[0].uri != pair[1].uri),
        "provenance material URIs must be unique"
    );

    let provenance = Provenance {
        schema_version: SCHEMA_VERSION,
        repository: options.source_repository.clone(),
        workflow: options.source_workflow.clone(),
        source_ref: options.source_ref.clone(),
        source_commit: options.source_commit.clone(),
        subject: Subject {
            kind: "catalogue",
            name: "catalogue/catalogue.json".into(),
            digest: sha256(&catalogue_bytes),
        },
        materials,
    };
    let provenance_value = serde_json::to_value(&provenance)?;
    validate_against_schema(
        &options.provenance_schema,
        &provenance_schema,
        &provenance_value,
    )?;
    let provenance_bytes = canonical_json(&provenance_value)?;

    atomic_write_pair(
        &options.output,
        &catalogue_bytes,
        &options.provenance_output,
        &provenance_bytes,
    )
}

fn validate_provenance_identity(options: &GenerateOptions) -> Result<()> {
    ensure!(
        options.source_repository == "MySproutOS/Deployment-Templates",
        "--source-repository must be MySproutOS/Deployment-Templates"
    );
    ensure!(
        options.source_workflow.starts_with(".github/workflows/")
            && (options.source_workflow.ends_with(".yml")
                || options.source_workflow.ends_with(".yaml"))
            && !options.source_workflow.contains(".."),
        "--source-workflow must be a normalized .github/workflows/*.yml path"
    );
    ensure!(
        options.source_ref.starts_with("refs/heads/")
            || options.source_ref.starts_with("refs/tags/"),
        "--source-ref must be a full refs/heads/* or refs/tags/* ref"
    );
    ensure!(
        is_lower_hex(&options.source_commit, 40),
        "--source-commit must be an exact 40-character lowercase hexadecimal commit"
    );
    Ok(())
}

fn discover_sources(apps_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(apps_dir)
        .with_context(|| format!("read app directory {}", apps_dir.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let path = entry.path().join("manifest-source.json");
        if path.exists() {
            let metadata = fs::symlink_metadata(&path)?;
            ensure!(
                metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
                "{}: source manifest must be a regular non-symlink file",
                path.display()
            );
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

fn validate_source(path: &Path, source: &SourceManifest) -> Result<()> {
    ensure!(
        source.schema_version == SCHEMA_VERSION,
        "{}: unsupported schema_version",
        path.display()
    );
    validate_token("id", &source.id)?;
    validate_nonempty("name", &source.name)?;
    validate_nonempty("pitch", &source.pitch)?;
    validate_nonempty("description_md", &source.description_md)?;
    validate_https_url("repository.url", &source.repository.url)?;
    ensure!(
        is_lower_hex(&source.repository.commit, 40),
        "{}: repository.commit must be an exact 40-character lowercase hexadecimal commit",
        path.display()
    );
    if let Some(homepage) = &source.homepage {
        validate_https_url("homepage", homepage)?;
    }
    validate_nonempty("license", &source.license)?;
    spdx::Expression::parse(&source.license).with_context(|| {
        format!(
            "{}: license must be a valid SPDX expression",
            path.display()
        )
    })?;
    ensure!(
        source.plugin.protocol_version == SCHEMA_VERSION,
        "{}: unsupported plugin protocol_version",
        path.display()
    );
    validate_token("deployment.preset", &source.deployment.preset)?;
    validate_token("deployment.runtime", &source.deployment.runtime)?;
    if let Some(migration) = &source.deployment.migration {
        validate_relative_path("deployment.migration.path", &migration.path)?;
    }
    validate_unique_tokens(
        "deployment.required_capabilities",
        &source.deployment.required_capabilities,
    )?;

    match source.readiness.status {
        ReadinessStatus::Blocked => ensure!(
            !source.readiness.blocked_reasons.is_empty(),
            "{}: blocked readiness requires at least one blocked reason",
            path.display()
        ),
        ReadinessStatus::Live => ensure!(
            source.readiness.blocked_reasons.is_empty(),
            "{}: live readiness cannot contain blocked reasons",
            path.display()
        ),
        ReadinessStatus::Retired => ensure!(
            source.readiness.blocked_reasons.is_empty(),
            "{}: retired readiness cannot contain blocked reasons",
            path.display()
        ),
    }

    let mut structural_keys = BTreeSet::new();
    let mut environments = BTreeSet::new();
    for service in &source.services {
        validate_token("services[].key", &service.key)?;
        ensure!(
            structural_keys.insert(&service.key),
            "{}: duplicate structural key '{}'",
            path.display(),
            service.key
        );
        ensure!(
            !service.bindings.is_empty(),
            "{}: service '{}' must have explicit bindings",
            path.display(),
            service.key
        );
        for binding in &service.bindings {
            validate_environment(&binding.environment)?;
            ensure!(
                environments.insert(&binding.environment),
                "{}: duplicate environment binding '{}'",
                path.display(),
                binding.environment
            );
            ensure!(
                service_supports(service.kind, binding.output),
                "{}: service '{}' does not provide requested output",
                path.display(),
                service.key
            );
        }
    }
    for input in &source.user_inputs {
        validate_token("user_inputs[].key", &input.key)?;
        ensure!(
            structural_keys.insert(&input.key),
            "{}: duplicate structural key '{}'",
            path.display(),
            input.key
        );
        validate_environment(&input.environment)?;
        ensure!(
            environments.insert(&input.environment),
            "{}: duplicate environment binding '{}'",
            path.display(),
            input.environment
        );
    }
    for input in &source.generated_inputs {
        validate_token("generated_inputs[].key", &input.key)?;
        ensure!(
            structural_keys.insert(&input.key),
            "{}: duplicate structural key '{}'",
            path.display(),
            input.key
        );
        ensure!(
            (32..=128).contains(&input.bytes),
            "{}: generated input '{}' bytes must be between 32 and 128",
            path.display(),
            input.key
        );
        validate_environment(&input.environment)?;
        ensure!(
            environments.insert(&input.environment),
            "{}: duplicate environment binding '{}'",
            path.display(),
            input.environment
        );
    }
    Ok(())
}

fn sort_source(source: &mut SourceManifest) {
    source.readiness.blocked_reasons.sort();
    source.readiness.blocked_reasons.dedup();
    source.deployment.required_capabilities.sort();
    source
        .services
        .sort_by(|left, right| left.key.cmp(&right.key));
    for service in &mut source.services {
        service
            .bindings
            .sort_by(|left, right| left.environment.cmp(&right.environment));
    }
    source
        .user_inputs
        .sort_by(|left, right| left.key.cmp(&right.key));
    source
        .generated_inputs
        .sort_by(|left, right| left.key.cmp(&right.key));
}

fn validate_locked_artifact(id: &str, value: &str) -> Result<(String, String)> {
    ensure!(
        value.trim() == value,
        "plugin lock for '{id}' contains whitespace"
    );
    let expected_prefix = format!("{OCI_PREFIX}{id}-plugin@sha256:");
    ensure!(
        value.starts_with(&expected_prefix),
        "plugin lock for '{id}' must be an immutable public {expected_prefix}<64 lowercase hex> artifact"
    );
    ensure!(
        value.matches('@').count() == 1,
        "plugin lock for '{id}' has an ambiguous OCI reference"
    );
    ensure!(
        !value[..value.find('@').expect("prefix contains @")].contains(':'),
        "plugin lock for '{id}' must not contain a tag"
    );
    let digest = value
        .rsplit_once('@')
        .map(|(_, digest)| digest)
        .expect("validated prefix")
        .to_owned();
    ensure!(
        digest
            .strip_prefix("sha256:")
            .is_some_and(|hex| is_lower_hex(hex, 64)),
        "plugin lock for '{id}' must contain exactly 64 lowercase sha256 hex characters"
    );
    Ok((value.to_owned(), digest))
}

fn validate_e2e_proof(
    options: &GenerateOptions,
    source: &SourceManifest,
    plugin_digest: &str,
) -> Result<(E2eEvidence, Vec<Material>)> {
    let path = options.e2e_proof_dir.join(format!("{}.json", source.id));
    let bytes = read_regular_file(&path).with_context(|| {
        format!(
            "app '{}' cannot be live without detached end-to-end proof",
            source.id
        )
    })?;
    let proof: E2eProof = parse_json(&path, &bytes)?;
    ensure!(
        proof.schema_version == SCHEMA_VERSION,
        "{}: unsupported E2E proof schema",
        path.display()
    );
    ensure!(proof.passed, "{}: E2E proof did not pass", path.display());
    ensure!(
        proof.app_id == source.id,
        "{}: E2E proof app mismatch",
        path.display()
    );
    ensure!(
        proof.upstream_commit == source.repository.commit,
        "{}: E2E proof upstream mismatch",
        path.display()
    );
    ensure!(
        proof.plugin_digest == plugin_digest,
        "{}: E2E proof plugin mismatch",
        path.display()
    );
    ensure!(
        proof.preset == source.deployment.preset,
        "{}: E2E proof preset mismatch",
        path.display()
    );
    ensure!(
        proof.required_capabilities == source.deployment.required_capabilities,
        "{}: E2E proof capabilities mismatch",
        path.display()
    );
    ensure!(
        proof
            .workflow_run_url
            .starts_with("https://github.com/MySproutOS/Deployment-Templates/actions/runs/")
            && proof
                .workflow_run_url
                .rsplit_once('/')
                .is_some_and(|(_, run_id)| {
                    !run_id.is_empty() && run_id.bytes().all(|byte| byte.is_ascii_digit())
                }),
        "{}: E2E proof workflow_run_url must identify a Deployment-Templates Actions run",
        path.display()
    );
    ensure!(
        proof.tested_at.ends_with('Z') && proof.tested_at.contains('T'),
        "{}: E2E proof tested_at must be a UTC RFC3339 timestamp",
        path.display()
    );
    validate_digest("evidence_digest", &proof.evidence_digest)?;
    validate_relative_path("evidence_path", &proof.evidence_path)?;
    let evidence_path = options.e2e_proof_dir.join(&proof.evidence_path);
    let evidence_bytes = read_regular_file(&evidence_path)?;
    ensure!(
        sha256(&evidence_bytes) == proof.evidence_digest,
        "{}: E2E evidence digest does not match {}",
        path.display(),
        evidence_path.display()
    );
    let evidence = E2eEvidence {
        workflow_run_url: proof.workflow_run_url,
        tested_at: proof.tested_at,
        upstream_commit: proof.upstream_commit,
        plugin_digest: proof.plugin_digest,
    };
    Ok((
        evidence,
        vec![
            Material {
                uri: format!("catalogue/e2e-proofs/{}.json", source.id),
                digest: sha256(&bytes),
            },
            Material {
                uri: format!("catalogue/e2e-proofs/{}", proof.evidence_path),
                digest: proof.evidence_digest,
            },
        ],
    ))
}

fn catalogue_schema_without_external_manifest_ref(mut schema: Value) -> Result<Value> {
    let items = schema
        .pointer_mut("/properties/apps/items")
        .ok_or_else(|| anyhow!("catalogue schema must define properties.apps.items"))?;
    ensure!(
        items.get("$ref").and_then(Value::as_str) == Some("app-manifest-v1.schema.json"),
        "catalogue schema apps must reference app-manifest-v1.schema.json"
    );
    *items = serde_json::json!({});
    Ok(schema)
}

fn add_schema_materials(options: &GenerateOptions, materials: &mut Vec<Material>) -> Result<()> {
    for (name, path) in [
        ("app-manifest-v1.schema.json", &options.manifest_schema),
        ("catalogue-v1.schema.json", &options.catalogue_schema),
        ("provenance-v1.schema.json", &options.provenance_schema),
    ] {
        let bytes = read_regular_file(path)?;
        materials.push(Material {
            uri: format!("schema/{name}"),
            digest: sha256(&bytes),
        });
    }

    if options.protocol_schema_dir.exists() {
        let mut paths = fs::read_dir(&options.protocol_schema_dir)
            .with_context(|| {
                format!(
                    "read protocol schema directory {}",
                    options.protocol_schema_dir.display()
                )
            })?
            .map(|entry| entry.map(|value| value.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        for path in paths {
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let bytes = read_regular_file(&path)?;
            materials.push(Material {
                uri: format!(
                    "packages/sprout-template-protocol/schema/{}",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .expect("JSON filename")
                ),
                digest: sha256(&bytes),
            });
        }
    }
    Ok(())
}

fn add_source_tree_materials(
    options: &GenerateOptions,
    manifest_ids: &BTreeSet<String>,
    materials: &mut Vec<Material>,
) -> Result<()> {
    for id in manifest_ids {
        let path = options.plugin_source_dir.join(id);
        materials.push(Material {
            uri: format!("tree:plugins/{id}"),
            digest: source_tree_digest(&path)?,
        });
    }
    materials.push(Material {
        uri: "tree:packages/sprout-template-protocol".into(),
        digest: source_tree_digest(&options.protocol_source_dir)?,
    });
    Ok(())
}

fn source_tree_digest(root: &Path) -> Result<String> {
    let metadata = fs::symlink_metadata(root)
        .with_context(|| format!("inspect source tree {}", root.display()))?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "{} must be a non-symlink directory",
        root.display()
    );
    let mut files = Vec::new();
    collect_tree_files(root, root, &mut files)?;
    ensure!(
        !files.is_empty(),
        "source tree {} contains no files",
        root.display()
    );
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (relative, path) in files {
        let path_bytes = relative.as_bytes();
        let contents = read_regular_file(&path)?;
        hasher.update((path_bytes.len() as u64).to_be_bytes());
        hasher.update(path_bytes);
        hasher.update((contents.len() as u64).to_be_bytes());
        hasher.update(contents);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn collect_tree_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<()> {
    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("read source tree directory {}", directory.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file_type = entry.file_type()?;
        let path = entry.path();
        ensure!(
            !file_type.is_symlink(),
            "{}: symlinks are not allowed in hashed source trees",
            path.display()
        );
        if file_type.is_dir() {
            collect_tree_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .expect("recursive entry is below root")
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.push((relative, path));
        } else {
            bail!(
                "{}: only regular files are allowed in hashed source trees",
                path.display()
            );
        }
    }
    Ok(())
}

fn load_schema(path: &Path) -> Result<Value> {
    let bytes = read_regular_file(path)?;
    parse_json(path, &bytes)
}

fn validate_against_schema(path: &Path, schema: &Value, instance: &Value) -> Result<()> {
    let validator = jsonschema::validator_for(schema)
        .with_context(|| format!("compile JSON schema {}", path.display()))?;
    let errors = validator
        .iter_errors(instance)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    ensure!(
        errors.is_empty(),
        "{} validation failed: {}",
        path.display(),
        errors.join("; ")
    );
    Ok(())
}

fn read_regular_file(path: &Path) -> Result<Vec<u8>> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("inspect {}", path.display()))?;
    ensure!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "{} must be a regular non-symlink file",
        path.display()
    );
    fs::read(path).with_context(|| format!("read {}", path.display()))
}

fn parse_json<T: for<'de> Deserialize<'de>>(path: &Path, bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).with_context(|| format!("parse {}", path.display()))
}

fn canonical_json(value: &Value) -> Result<Vec<u8>> {
    // RFC 8785 JCS defines the payload bytes. The sole terminal LF is part of the
    // published blob contract and therefore part of its recorded digest.
    let mut bytes = serde_jcs::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn atomic_write_pair(
    first_path: &Path,
    first: &[u8],
    second_path: &Path,
    second: &[u8],
) -> Result<()> {
    ensure!(
        first_path != second_path,
        "catalogue and provenance output paths must differ"
    );
    if let Some(parent) = first_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = second_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let first_temp = temporary_path(first_path);
    let second_temp = temporary_path(second_path);
    fs::write(&first_temp, first).with_context(|| format!("write {}", first_temp.display()))?;
    if let Err(error) = fs::write(&second_temp, second) {
        let _ = fs::remove_file(&first_temp);
        return Err(error).with_context(|| format!("write {}", second_temp.display()));
    }
    fs::rename(&first_temp, first_path)
        .with_context(|| format!("publish {}", first_path.display()))?;
    fs::rename(&second_temp, second_path)
        .with_context(|| format!("publish {}", second_path.display()))?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output");
    path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()))
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn validate_nonempty(field: &str, value: &str) -> Result<()> {
    ensure!(!value.trim().is_empty(), "{field} must not be empty");
    ensure!(
        value.trim() == value,
        "{field} must not contain leading or trailing whitespace"
    );
    Ok(())
}

fn validate_https_url(field: &str, value: &str) -> Result<()> {
    let parsed = url::Url::parse(value).with_context(|| format!("{field} must be a valid URL"))?;
    ensure!(
        parsed.scheme() == "https"
            && parsed.host_str().is_some()
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && !value.contains(char::is_whitespace),
        "{field} must be an HTTPS URL without credentials"
    );
    Ok(())
}

fn validate_token(field: &str, value: &str) -> Result<()> {
    ensure!(
        is_token(value),
        "{field} must be a lowercase structural token"
    );
    Ok(())
}

fn validate_unique_tokens(field: &str, values: &[String]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        validate_token(field, value)?;
        ensure!(seen.insert(value), "{field} must contain unique values");
    }
    Ok(())
}

fn is_token(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'0'..=b'9'))
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn validate_environment(value: &str) -> Result<()> {
    let mut bytes = value.bytes();
    ensure!(
        matches!(bytes.next(), Some(b'A'..=b'Z' | b'_'))
            && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'),
        "environment '{value}' must match ^[A-Z_][A-Z0-9_]*$"
    );
    Ok(())
}

fn validate_relative_path(field: &str, value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && !value.starts_with('/')
            && !value.contains('\\')
            && !value.contains('\0')
            && !value
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
            && value.split('/').next() != Some(".git"),
        "{field} must be a normalized relative path outside .git"
    );
    Ok(())
}

fn validate_digest(field: &str, value: &str) -> Result<()> {
    ensure!(
        value
            .strip_prefix("sha256:")
            .is_some_and(|hex| is_lower_hex(hex, 64)),
        "{field} must contain an sha256 digest with 64 lowercase hexadecimal characters"
    );
    Ok(())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn service_supports(kind: ServiceKind, output: ServiceOutput) -> bool {
    match kind {
        ServiceKind::Postgres | ServiceKind::Valkey => output == ServiceOutput::ConnectionUrl,
        ServiceKind::Elasticsearch => matches!(
            output,
            ServiceOutput::Endpoint | ServiceOutput::Username | ServiceOutput::Password
        ),
        ServiceKind::ObjectStorage => matches!(
            output,
            ServiceOutput::Endpoint
                | ServiceOutput::Region
                | ServiceOutput::Bucket
                | ServiceOutput::AccessKeyId
                | ServiceOutput::SecretAccessKey
                | ServiceOutput::ForcePathStyle
        ),
    }
}
