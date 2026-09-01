//! Small, deliberately boring runtime shared by app-specific template plugins.
//!
//! The app plugins decide *what* a compatible checkout should contain. This crate owns the
//! security-sensitive mechanics around that decision: bounded protocol input, normalized workspace
//! paths, conflict detection, atomic file writes, executable modes, sorted content digests, and one
//! JSON response on stdout. The trusted caller verifies checkout identity before invoking a plugin;
//! recipes independently pin the identity in the request and any upstream source they rewrite.

use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use sha2::{Digest, Sha256};
use sprout_template_protocol::{
    ApplyRequest, ApplyResponse, Change, ChangeKind, ErrorCode, ErrorResponse, PROTOCOL_VERSION,
    parse_request,
};
use thiserror::Error;

const MAX_REQUEST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub enum Mutation {
    /// Create a recipe-owned file, or accept it when it already has exactly these bytes and mode.
    OwnFile {
        path: &'static str,
        contents: Vec<u8>,
        mode: u32,
    },
    /// Replace one exact upstream file with one exact recipe result.
    ExactPatch {
        path: &'static str,
        before: Vec<u8>,
        after: Vec<u8>,
        mode: u32,
    },
    /// Replace one unique fragment in a file whose complete before/after digests are pinned.
    ExactRewrite {
        path: &'static str,
        before_sha256: &'static str,
        after_sha256: &'static str,
        before_fragment: &'static [u8],
        after_fragment: &'static [u8],
        mode: u32,
    },
}

impl Mutation {
    pub fn own(path: &'static str, contents: impl Into<Vec<u8>>) -> Self {
        Self::OwnFile {
            path,
            contents: contents.into(),
            mode: 0o644,
        }
    }

    pub fn executable(path: &'static str, contents: impl Into<Vec<u8>>) -> Self {
        Self::OwnFile {
            path,
            contents: contents.into(),
            mode: 0o755,
        }
    }

    pub fn patch(
        path: &'static str,
        before: impl Into<Vec<u8>>,
        after: impl Into<Vec<u8>>,
    ) -> Self {
        Self::ExactPatch {
            path,
            before: before.into(),
            after: after.into(),
            mode: 0o644,
        }
    }

    pub fn rewrite(
        path: &'static str,
        before_sha256: &'static str,
        after_sha256: &'static str,
        before_fragment: &'static [u8],
        after_fragment: &'static [u8],
    ) -> Self {
        Self::ExactRewrite {
            path,
            before_sha256,
            after_sha256,
            before_fragment,
            after_fragment,
            mode: 0o644,
        }
    }

    fn path(&self) -> &'static str {
        match self {
            Self::OwnFile { path, .. }
            | Self::ExactPatch { path, .. }
            | Self::ExactRewrite { path, .. } => path,
        }
    }
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("unsupported protocol: {0}")]
    UnsupportedProtocol(String),
    #[error("unsupported upstream: {0}")]
    UnsupportedUpstream(String),
    #[error("unsafe workspace: {0}")]
    UnsafeWorkspace(String),
    #[error("conflicting change: {0}")]
    ConflictingChange(String),
    #[error("I/O failure: {0}")]
    Io(String),
    #[error("internal failure: {0}")]
    Internal(String),
}

impl RuntimeError {
    fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidRequest(_) => ErrorCode::InvalidRequest,
            Self::UnsupportedProtocol(_) => ErrorCode::UnsupportedProtocol,
            Self::UnsupportedUpstream(_) => ErrorCode::UnsupportedUpstream,
            Self::UnsafeWorkspace(_) => ErrorCode::UnsafeWorkspace,
            Self::ConflictingChange(_) => ErrorCode::ConflictingChange,
            Self::Io(_) => ErrorCode::Io,
            Self::Internal(_) => ErrorCode::Internal,
        }
    }
}

pub type Recipe = fn(&ApplyRequest) -> Result<Vec<Mutation>, RuntimeError>;

/// Process one request from stdin and emit exactly one response to stdout.
pub fn run(recipe: Recipe) {
    let result = read_request().and_then(|request| apply(&request, recipe));
    let failed = result.is_err();
    let response = match result {
        Ok(changes) => ApplyResponse::Ok {
            protocol_version: PROTOCOL_VERSION,
            changes,
            warnings: Vec::new(),
        },
        Err(error) => ApplyResponse::Error {
            protocol_version: PROTOCOL_VERSION,
            error: ErrorResponse {
                code: error.code(),
                message: error.to_string(),
            },
        },
    };

    // Every variant assembled above satisfies the response contract. If serialization itself fails,
    // there is no second channel on which a plugin can safely recover.
    serde_json::to_writer(io::stdout().lock(), &response).expect("serialize protocol response");
    println!();
    if failed {
        std::process::exit(1);
    }
}

pub fn apply(request: &ApplyRequest, recipe: Recipe) -> Result<Vec<Change>, RuntimeError> {
    if request.protocol_version != PROTOCOL_VERSION {
        return Err(RuntimeError::UnsupportedProtocol(format!(
            "expected {PROTOCOL_VERSION}, received {}",
            request.protocol_version
        )));
    }
    let root = canonical_workspace(&request.workspace)?;
    let mut mutations = recipe(request)?;
    mutations.sort_by_key(Mutation::path);
    if mutations
        .windows(2)
        .any(|pair| pair[0].path() == pair[1].path())
    {
        return Err(RuntimeError::Internal(
            "recipe returned duplicate mutation paths".into(),
        ));
    }

    // Resolve every mutation before the first write. A conflict therefore cannot leave half a
    // recipe applied.
    let planned = mutations
        .into_iter()
        .map(|mutation| plan_mutation(&root, mutation))
        .collect::<Result<Vec<_>, _>>()?;

    let mut changes = Vec::new();
    for plan in planned {
        if let Some(change) = apply_plan(plan)? {
            changes.push(change);
        }
    }
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(changes)
}

fn read_request() -> Result<ApplyRequest, RuntimeError> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_REQUEST_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| RuntimeError::Io(error.to_string()))?;
    if bytes.len() as u64 > MAX_REQUEST_BYTES {
        return Err(RuntimeError::InvalidRequest(format!(
            "request exceeds {MAX_REQUEST_BYTES} bytes"
        )));
    }
    parse_request(&bytes).map_err(|error| {
        let message = error.to_string();
        if message.starts_with("protocol_version:") {
            RuntimeError::UnsupportedProtocol(message)
        } else {
            RuntimeError::InvalidRequest(message)
        }
    })
}

fn canonical_workspace(raw: &str) -> Result<PathBuf, RuntimeError> {
    let path = Path::new(raw);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
    {
        return Err(RuntimeError::UnsafeWorkspace(
            "workspace must be a normalized absolute path".into(),
        ));
    }
    #[cfg(windows)]
    {
        // Both Rust canonicalization and root metadata inspection use Windows handle operations
        // that AppContainer tokens can be denied even when this exact directory has an explicit
        // writable package ACE. The trusted Sprout caller creates a fresh staging root inside the
        // per-run AppContainer profile, rejects links while copying into it, snapshots it before
        // execution, and passes this normalized absolute path. Every recipe-owned descendant is
        // checked again before use. Keep that usable path instead of weakening the AppContainer's
        // object-manager or host-filesystem access.
        Ok(path.to_path_buf())
    }
    #[cfg(not(windows))]
    {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| RuntimeError::UnsafeWorkspace(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(RuntimeError::UnsafeWorkspace(
                "workspace must not be a symbolic link".into(),
            ));
        }
        if !metadata.is_dir() {
            return Err(RuntimeError::UnsafeWorkspace(
                "workspace is not a directory".into(),
            ));
        }
        path.canonicalize()
            .map_err(|error| RuntimeError::UnsafeWorkspace(error.to_string()))
    }
}

struct PlannedMutation {
    relative: String,
    absolute: PathBuf,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
    mode: u32,
    needs_write: bool,
}

fn plan_mutation(root: &Path, mutation: Mutation) -> Result<PlannedMutation, RuntimeError> {
    let relative = mutation.path().to_owned();
    validate_relative(&relative)?;
    let absolute = root.join(&relative);
    reject_symlink_ancestors(root, &absolute)?;
    let existing = match fs::symlink_metadata(&absolute) {
        Ok(metadata) => {
            if !metadata.file_type().is_file() {
                return Err(RuntimeError::ConflictingChange(format!(
                    "{} is not a regular file",
                    relative
                )));
            }
            Some(
                fs::read(&absolute)
                    .map_err(|error| RuntimeError::Io(format!("{relative}: {error}")))?,
            )
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(RuntimeError::Io(format!("{}: {error}", relative))),
    };

    let (after, mode, valid_existing) = match mutation {
        Mutation::OwnFile { contents, mode, .. } => {
            let valid = existing.as_deref().is_none_or(|bytes| bytes == contents);
            (contents, mode, valid)
        }
        Mutation::ExactPatch {
            before,
            after,
            mode,
            ..
        } => {
            let valid = existing
                .as_deref()
                .is_some_and(|bytes| bytes == before || bytes == after);
            (after, mode, valid)
        }
        Mutation::ExactRewrite {
            before_sha256,
            after_sha256,
            before_fragment,
            after_fragment,
            mode,
            ..
        } => {
            let Some(current) = existing.as_deref() else {
                return Err(RuntimeError::ConflictingChange(format!(
                    "{} is missing",
                    relative
                )));
            };
            let current_sha = sha256(current);
            if current_sha == format!("sha256:{after_sha256}") {
                (current.to_vec(), mode, true)
            } else if current_sha == format!("sha256:{before_sha256}") {
                let occurrences = current
                    .windows(before_fragment.len())
                    .filter(|window| *window == before_fragment)
                    .count();
                if occurrences != 1 {
                    return Err(RuntimeError::ConflictingChange(format!(
                        "{} does not contain exactly one pinned rewrite location",
                        relative
                    )));
                }
                let start = current
                    .windows(before_fragment.len())
                    .position(|window| window == before_fragment)
                    .expect("one occurrence");
                let mut rewritten = Vec::with_capacity(
                    current.len() - before_fragment.len() + after_fragment.len(),
                );
                rewritten.extend_from_slice(&current[..start]);
                rewritten.extend_from_slice(after_fragment);
                rewritten.extend_from_slice(&current[start + before_fragment.len()..]);
                if sha256(&rewritten) != format!("sha256:{after_sha256}") {
                    return Err(RuntimeError::Internal(format!(
                        "{} rewrite did not produce its pinned digest",
                        relative
                    )));
                }
                (rewritten, mode, true)
            } else {
                (Vec::new(), mode, false)
            }
        }
    };
    if !valid_existing {
        return Err(RuntimeError::ConflictingChange(format!(
            "{} differs from both the pinned upstream and the recipe output",
            relative
        )));
    }

    let mode_matches = mode_matches(&absolute, mode);
    let needs_write = existing.as_deref() != Some(after.as_slice()) || !mode_matches;
    Ok(PlannedMutation {
        relative,
        absolute,
        before: existing,
        after,
        mode,
        needs_write,
    })
}

fn validate_relative(value: &str) -> Result<(), RuntimeError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('\\')
        || path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
        || path.components().next() == Some(Component::Normal(".git".as_ref()))
    {
        return Err(RuntimeError::Internal(format!(
            "recipe contains unsafe path {value:?}"
        )));
    }
    Ok(())
}

fn reject_symlink_ancestors(root: &Path, target: &Path) -> Result<(), RuntimeError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| RuntimeError::UnsafeWorkspace("mutation escaped workspace".into()))?;
    let mut cursor = root.to_path_buf();
    for part in relative.components() {
        cursor.push(part);
        match fs::symlink_metadata(&cursor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(RuntimeError::UnsafeWorkspace(format!(
                    "{} traverses a symbolic link",
                    relative.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(RuntimeError::Io(error.to_string())),
        }
    }
    Ok(())
}

fn apply_plan(plan: PlannedMutation) -> Result<Option<Change>, RuntimeError> {
    if !plan.needs_write {
        return Ok(None);
    }
    let parent = plan
        .absolute
        .parent()
        .ok_or_else(|| RuntimeError::Internal("mutation has no parent".into()))?;
    fs::create_dir_all(parent).map_err(|error| RuntimeError::Io(error.to_string()))?;

    let temporary = parent.join(format!(
        ".sprout-template-{}-{}.tmp",
        std::process::id(),
        plan.absolute
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("file")
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| RuntimeError::Io(error.to_string()))?;
    if let Err(error) = file.write_all(&plan.after).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(RuntimeError::Io(error.to_string()));
    }
    if let Err(error) = set_mode(&temporary, plan.mode) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temporary, &plan.absolute) {
        // Windows does not replace an existing destination. The preflight already proved this exact
        // file is recipe-owned, so remove only that resolved file and retry the rename.
        if plan.before.is_some() {
            if let Err(remove_error) = fs::remove_file(&plan.absolute) {
                let _ = fs::remove_file(&temporary);
                return Err(RuntimeError::Io(format!(
                    "rename failed ({error}); removing destination failed ({remove_error})"
                )));
            }
            if let Err(rename_error) = fs::rename(&temporary, &plan.absolute) {
                let _ = fs::remove_file(&temporary);
                return Err(RuntimeError::Io(rename_error.to_string()));
            }
        } else {
            let _ = fs::remove_file(&temporary);
            return Err(RuntimeError::Io(error.to_string()));
        }
    }

    Ok(Some(Change {
        path: plan.relative,
        kind: if plan.before.is_some() {
            ChangeKind::Modified
        } else {
            ChangeKind::Created
        },
        before_sha256: plan.before.as_deref().map(sha256),
        after_sha256: Some(sha256(&plan.after)),
    }))
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(unix)]
fn mode_matches(path: &Path, mode: u32) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o777 == mode)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn mode_matches(path: &Path, _mode: u32) -> bool {
    path.is_file()
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<(), RuntimeError> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|error| RuntimeError::Io(error.to_string()))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<(), RuntimeError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_protocol_style() {
        assert_eq!(
            sha256(b"hello"),
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn refuses_traversal() {
        assert!(validate_relative("../outside").is_err());
        assert!(validate_relative(".git/config").is_err());
        assert!(validate_relative("valid/path").is_ok());
    }

    #[test]
    fn accepts_a_normalized_absolute_workspace_directory() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().to_str().unwrap();
        let accepted = canonical_workspace(path).unwrap();
        #[cfg(windows)]
        assert_eq!(accepted, workspace.path());
        #[cfg(not(windows))]
        assert_eq!(accepted, workspace.path().canonicalize().unwrap());
    }
}
