use std::{collections::HashSet, path::Component};

use thiserror::Error;

use crate::v1::{
    ApplyRequest, ApplyResponse, ChangeKind, ServiceKind, ServiceOutput, TemplateIdentity,
};

#[derive(Debug, Error)]
pub enum ProtocolParseError {
    #[error("invalid protocol JSON: {0}")]
    Decode(#[from] serde_json::Error),
    #[error(transparent)]
    Validation(#[from] ValidationError),
}

/// Decode and semantically validate a v1 request. Protocol consumers should use this instead of
/// calling `serde_json` directly.
pub fn parse_request(bytes: &[u8]) -> Result<ApplyRequest, ProtocolParseError> {
    let request: ApplyRequest = serde_json::from_slice(bytes)?;
    request.validate()?;
    Ok(request)
}

/// Decode and semantically validate a v1 response.
pub fn parse_response(bytes: &[u8]) -> Result<ApplyResponse, ProtocolParseError> {
    let response: ApplyResponse = serde_json::from_slice(bytes)?;
    response.validate()?;
    Ok(response)
}

/// Semantic checks which JSON Schema alone cannot express clearly.
pub trait Validate {
    fn validate(&self) -> Result<(), ValidationError>;
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("{field}: {message}")]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl ValidationError {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl Validate for ApplyRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        if self.protocol_version != crate::PROTOCOL_VERSION {
            return Err(ValidationError::new(
                "protocol_version",
                "unsupported protocol version",
            ));
        }

        validate_workspace(&self.workspace)?;
        validate_template(&self.template)?;
        validate_token("template.id", &self.template.id)?;
        validate_token("deployment.preset", &self.deployment.preset)?;
        validate_sorted_unique("deployment.capabilities", &self.deployment.capabilities)?;

        let mut keys = HashSet::new();
        let mut environments = HashSet::new();

        validate_sorted_by_key(
            "services",
            self.services.iter().map(|value| value.key.as_str()),
        )?;
        for (index, service) in self.services.iter().enumerate() {
            validate_token(&format!("services[{index}].key"), &service.key)?;
            if !keys.insert(service.key.as_str()) {
                return Err(ValidationError::new(
                    format!("services[{index}].key"),
                    "duplicate structural key",
                ));
            }
            if service.bindings.is_empty() {
                return Err(ValidationError::new(
                    format!("services[{index}].bindings"),
                    "at least one binding is required",
                ));
            }
            validate_sorted_by_key(
                &format!("services[{index}].bindings"),
                service
                    .bindings
                    .iter()
                    .map(|value| value.environment.as_str()),
            )?;
            for (binding_index, binding) in service.bindings.iter().enumerate() {
                let field = format!("services[{index}].bindings[{binding_index}]");
                validate_environment(&format!("{field}.environment"), &binding.environment)?;
                if !environments.insert(binding.environment.as_str()) {
                    return Err(ValidationError::new(
                        format!("{field}.environment"),
                        "environment binding is not globally unique",
                    ));
                }
                if !service.kind.supports(binding.output) {
                    return Err(ValidationError::new(
                        format!("{field}.output"),
                        format!("output is not provided by {}", service.kind.as_str()),
                    ));
                }
            }
        }

        validate_sorted_by_key(
            "user_inputs",
            self.user_inputs.iter().map(|value| value.key.as_str()),
        )?;
        for (index, input) in self.user_inputs.iter().enumerate() {
            validate_token(&format!("user_inputs[{index}].key"), &input.key)?;
            if !keys.insert(input.key.as_str()) {
                return Err(ValidationError::new(
                    format!("user_inputs[{index}].key"),
                    "duplicate structural key",
                ));
            }
            validate_environment(
                &format!("user_inputs[{index}].environment"),
                &input.environment,
            )?;
            if !environments.insert(input.environment.as_str()) {
                return Err(ValidationError::new(
                    format!("user_inputs[{index}].environment"),
                    "environment binding is not globally unique",
                ));
            }
        }

        validate_sorted_by_key(
            "generated_inputs",
            self.generated_inputs.iter().map(|value| value.key.as_str()),
        )?;
        for (index, input) in self.generated_inputs.iter().enumerate() {
            validate_token(&format!("generated_inputs[{index}].key"), &input.key)?;
            if !keys.insert(input.key.as_str()) {
                return Err(ValidationError::new(
                    format!("generated_inputs[{index}].key"),
                    "duplicate structural key",
                ));
            }
            if !(32..=128).contains(&input.bytes) {
                return Err(ValidationError::new(
                    format!("generated_inputs[{index}].bytes"),
                    "must be between 32 and 128",
                ));
            }
            validate_environment(
                &format!("generated_inputs[{index}].environment"),
                &input.environment,
            )?;
            if !environments.insert(input.environment.as_str()) {
                return Err(ValidationError::new(
                    format!("generated_inputs[{index}].environment"),
                    "environment binding is not globally unique",
                ));
            }
        }

        Ok(())
    }
}

impl Validate for ApplyResponse {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            ApplyResponse::Ok {
                protocol_version,
                changes,
                warnings,
            } => {
                validate_version(*protocol_version)?;
                let mut previous: Option<&str> = None;
                for (index, change) in changes.iter().enumerate() {
                    validate_relative_path(&format!("changes[{index}].path"), &change.path)?;
                    if previous.is_some_and(|value| value >= change.path.as_str()) {
                        return Err(ValidationError::new(
                            format!("changes[{index}].path"),
                            "changes must be strictly sorted by path",
                        ));
                    }
                    previous = Some(&change.path);

                    match change.kind {
                        ChangeKind::Created => {
                            if change.before_sha256.is_some() || change.after_sha256.is_none() {
                                return Err(change_digest_error(index, "created"));
                            }
                        }
                        ChangeKind::Modified => {
                            if change.before_sha256.is_none() || change.after_sha256.is_none() {
                                return Err(change_digest_error(index, "modified"));
                            }
                        }
                        ChangeKind::Deleted => {
                            if change.before_sha256.is_none() || change.after_sha256.is_some() {
                                return Err(change_digest_error(index, "deleted"));
                            }
                        }
                    }
                    if let Some(digest) = &change.before_sha256 {
                        validate_digest(&format!("changes[{index}].before_sha256"), digest)?;
                    }
                    if let Some(digest) = &change.after_sha256 {
                        validate_digest(&format!("changes[{index}].after_sha256"), digest)?;
                    }
                }
                let mut previous_warning: Option<(&str, Option<&str>, &str)> = None;
                for (index, warning) in warnings.iter().enumerate() {
                    validate_token(&format!("warnings[{index}].code"), &warning.code)?;
                    if warning.message.trim().is_empty() {
                        return Err(ValidationError::new(
                            format!("warnings[{index}].message"),
                            "must not be empty",
                        ));
                    }
                    if let Some(path) = &warning.path {
                        validate_relative_path(&format!("warnings[{index}].path"), path)?;
                    }
                    let current = (
                        warning.code.as_str(),
                        warning.path.as_deref(),
                        warning.message.as_str(),
                    );
                    if previous_warning.is_some_and(|value| value >= current) {
                        return Err(ValidationError::new(
                            format!("warnings[{index}]"),
                            "warnings must be strictly sorted by code, path, and message",
                        ));
                    }
                    previous_warning = Some(current);
                }
            }
            ApplyResponse::Error {
                protocol_version,
                error,
            } => {
                validate_version(*protocol_version)?;
                if error.message.trim().is_empty() {
                    return Err(ValidationError::new("error.message", "must not be empty"));
                }
            }
        }
        Ok(())
    }
}

fn validate_version(version: u32) -> Result<(), ValidationError> {
    if version == crate::PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(ValidationError::new(
            "protocol_version",
            "unsupported protocol version",
        ))
    }
}

fn validate_workspace(workspace: &str) -> Result<(), ValidationError> {
    let path = std::path::Path::new(workspace);
    if !path.is_absolute() {
        return Err(ValidationError::new("workspace", "must be absolute"));
    }
    if workspace.contains('\0')
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(ValidationError::new(
            "workspace",
            "must be normalized and contain no traversal components",
        ));
    }
    Ok(())
}

fn validate_template(template: &TemplateIdentity) -> Result<(), ValidationError> {
    validate_digest("template.catalogue_digest", &template.catalogue_digest)?;
    validate_digest("template.manifest_digest", &template.manifest_digest)?;
    validate_digest("template.plugin_digest", &template.plugin_digest)?;
    if !template.upstream_repository.starts_with("https://") {
        return Err(ValidationError::new(
            "template.upstream_repository",
            "must be an HTTPS URL",
        ));
    }
    if !is_lower_hex(&template.upstream_commit, 40) {
        return Err(ValidationError::new(
            "template.upstream_commit",
            "must be an exact 40-character lowercase hexadecimal commit",
        ));
    }
    Ok(())
}

fn validate_digest(field: &str, value: &str) -> Result<(), ValidationError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(ValidationError::new(field, "must use the sha256 algorithm"));
    };
    if !is_lower_hex(hex, 64) {
        return Err(ValidationError::new(
            field,
            "must contain 64 lowercase hexadecimal characters",
        ));
    }
    Ok(())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn validate_environment(field: &str, value: &str) -> Result<(), ValidationError> {
    let mut bytes = value.bytes();
    if !matches!(bytes.next(), Some(b'A'..=b'Z' | b'_'))
        || !bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(ValidationError::new(field, "must match ^[A-Z_][A-Z0-9_]*$"));
    }
    Ok(())
}

fn validate_token(field: &str, value: &str) -> Result<(), ValidationError> {
    let mut bytes = value.bytes();
    if !matches!(bytes.next(), Some(b'a'..=b'z' | b'0'..=b'9'))
        || !bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
    {
        return Err(ValidationError::new(
            field,
            "must be a lowercase structural token",
        ));
    }
    Ok(())
}

fn validate_sorted_unique(field: &str, values: &[String]) -> Result<(), ValidationError> {
    if values
        .windows(2)
        .any(|pair| pair.first().expect("pair") >= pair.get(1).expect("pair"))
    {
        return Err(ValidationError::new(
            field,
            "must be strictly sorted and unique",
        ));
    }
    for value in values {
        validate_token(field, value)?;
    }
    Ok(())
}

fn validate_sorted_by_key<'a>(
    field: &str,
    values: impl Iterator<Item = &'a str>,
) -> Result<(), ValidationError> {
    let mut previous: Option<&str> = None;
    for value in values {
        if previous.is_some_and(|prior| prior >= value) {
            return Err(ValidationError::new(
                field,
                "must be strictly sorted and unique",
            ));
        }
        previous = Some(value);
    }
    Ok(())
}

fn validate_relative_path(field: &str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty()
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains('\0')
        || value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || value.split('/').next() == Some(".git")
    {
        return Err(ValidationError::new(
            field,
            "must be a normalized relative workspace path outside .git",
        ));
    }
    Ok(())
}

fn change_digest_error(index: usize, kind: &str) -> ValidationError {
    ValidationError::new(
        format!("changes[{index}]"),
        format!("{kind} change has invalid before/after digest presence"),
    )
}

impl ServiceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Valkey => "valkey",
            Self::Elasticsearch => "elasticsearch",
            Self::ObjectStorage => "object_storage",
        }
    }

    fn supports(self, output: ServiceOutput) -> bool {
        match self {
            Self::Postgres | Self::Valkey => output == ServiceOutput::ConnectionUrl,
            Self::Elasticsearch => matches!(
                output,
                ServiceOutput::Endpoint | ServiceOutput::Username | ServiceOutput::Password
            ),
            Self::ObjectStorage => matches!(
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
}
