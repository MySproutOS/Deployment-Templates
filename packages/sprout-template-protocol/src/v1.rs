use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyRequest {
    pub protocol_version: u32,
    pub workspace: String,
    pub template: TemplateIdentity,
    pub deployment: Deployment,
    pub services: Vec<ManagedService>,
    pub user_inputs: Vec<UserInput>,
    pub generated_inputs: Vec<GeneratedInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateIdentity {
    pub id: String,
    pub catalogue_digest: String,
    pub manifest_digest: String,
    pub plugin_digest: String,
    pub upstream_repository: String,
    pub upstream_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
    pub preset: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedService {
    pub key: String,
    pub kind: ServiceKind,
    pub bindings: Vec<EnvironmentBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Postgres,
    Valkey,
    Elasticsearch,
    ObjectStorage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvironmentBinding {
    pub environment: String,
    pub output: ServiceOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceOutput {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserInput {
    pub key: String,
    #[serde(rename = "type")]
    pub input_type: UserInputType,
    pub environment: String,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserInputType {
    String,
    Url,
    Integer,
    Boolean,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedInput {
    pub key: String,
    pub generator: Generator,
    pub bytes: u16,
    pub environment: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Generator {
    RandomBase64url,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApplyResponse {
    Ok {
        protocol_version: u32,
        changes: Vec<Change>,
        warnings: Vec<Warning>,
    },
    Error {
        protocol_version: u32,
        error: ErrorResponse,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub path: String,
    pub kind: ChangeKind,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub before_sha256: Option<String>,
    #[serde(deserialize_with = "deserialize_nullable")]
    pub after_sha256: Option<String>,
}

fn deserialize_nullable<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Warning {
    pub code: String,
    pub message: String,
    pub path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorResponse {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedProtocol,
    UnsupportedUpstream,
    UnsafeWorkspace,
    ConflictingChange,
    Io,
    Internal,
}
