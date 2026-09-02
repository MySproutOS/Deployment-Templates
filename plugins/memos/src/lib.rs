use sprout_template_protocol::{ApplyRequest, Generator, ServiceKind, ServiceOutput};
use sprout_template_runtime::{Mutation, RuntimeError};

const ID: &str = "memos";
const REPOSITORY: &str = "https://github.com/usememos/memos";
const COMMIT: &str = "22a5f3385b9fc5bdf282eb597aa3db79798aa5ab";
const MAIN_BEFORE_SHA256: &str = "a6de9e558eb5f19626e87c6c3edcfce008ed0ce147f41e1ca3130d639fd859be";
const MAIN_AFTER_SHA256: &str = "ece466288c4adb1adadc89ce537d29ac25180dc6fef3d3955ecd893b65f37d92";
const MAIN_BEFORE: &[u8] = b"if err := storeInstance.Migrate(ctx); err != nil {\n\t\treturn errors.Wrap(err, \"failed to migrate database\")\n\t}\n\tif err := storeInstance.LoadDeploymentConfiguration(ctx); err != nil {";
const MAIN_AFTER: &[u8] = b"if os.Getenv(\"MEMOS_SPROUTOS_CONTROLLED_MIGRATIONS\") != \"true\" {\n\t\tif err := storeInstance.Migrate(ctx); err != nil {\n\t\t\treturn errors.Wrap(err, \"failed to migrate database\")\n\t\t}\n\t}\n\tif err := storeInstance.LoadSproutOSDeploymentConfiguration(ctx); err != nil {";
const LIVE_REFRESH_BEFORE_SHA256: &str =
    "02f6d9fe1b20511de2aaf12cfb30ace32139c80414e9e536a39d629775384d57";
const LIVE_REFRESH_AFTER_SHA256: &str =
    "05b2cbef1a164ea21c2aecdc9c6cb1522069bf914815baf4fcb000f88cf31067";
const SSE_FETCH_BEFORE: &[u8] =
    br#"function fetchSSEStream(token: string, signal: AbortSignal): Promise<Response> {
  return fetch("/api/v1/sse", {
    headers: {
      Accept: "text/event-stream",
      Authorization: `Bearer ${token}`,
    },
    signal,
    credentials: "include",
  });
}"#;
const SSE_FETCH_AFTER: &[u8] = br#"// SproutOS invokes Lambda through a buffered request boundary, so a long-lived SSE response cannot
// reach the browser incrementally. The generated application keeps Memos' cache invalidation path
// but feeds it a bounded visible-tab polling pulse instead. Five seconds is deliberately explicit
// in the listing: this is eventual live refresh, not equivalent real-time streaming.
export const SPROUTOS_LIVE_POLL_INTERVAL_MS = 5000;

function fetchSSEStream(token: string, signal: AbortSignal): Promise<Response> {
  // Authentication remains checked before this transport is created, preserving the hook's
  // signed-out and refresh boundaries. Polling itself invalidates authenticated queries, whose
  // ordinary requests carry the current token through the shared Connect client.
  void token;
  const encoder = new TextEncoder();
  let timer: ReturnType<typeof setInterval> | undefined;
  let removeAbortListener = () => {};
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      let closed = false;
      const pulse = () => {
        if (!closed) controller.enqueue(encoder.encode('data: {"type":"space.changed"}\n\n'));
      };
      const close = () => {
        if (closed) return;
        closed = true;
        if (timer !== undefined) clearInterval(timer);
        controller.close();
      };
      removeAbortListener = () => signal.removeEventListener("abort", close);
      if (signal.aborted) {
        close();
        return;
      }
      signal.addEventListener("abort", close, { once: true });
      pulse();
      timer = setInterval(pulse, SPROUTOS_LIVE_POLL_INTERVAL_MS);
    },
    cancel() {
      if (timer !== undefined) clearInterval(timer);
      removeAbortListener();
    },
  });
  return Promise.resolve(new Response(stream, { headers: { "Content-Type": "text/event-stream" } }));
}"#;

const BUILD: &str = include_str!("../assets/build.sh");
const RUN: &str = include_str!("../assets/run.sh");
const STORAGE_BRIDGE: &str = include_str!("../assets/sproutos_deployment_config.go");
const MIGRATOR: &str = include_str!("../assets/sproutos_migrate.go");
const MIGRATOR_TEST: &str = include_str!("../assets/sproutos_migrate_test.go");
const LIVE_POLLING_TEST: &str = include_str!("../assets/sproutos_live_polling.test.ts");
const DEPLOY_WORKFLOW: &str = include_str!("../assets/sproutos-deploy.yml");

pub fn recipe(request: &ApplyRequest) -> Result<Vec<Mutation>, RuntimeError> {
    validate_request(request)?;
    Ok(vec![
        Mutation::own(".config/sproutos.toml", configuration(request)),
        Mutation::rewrite(
            "cmd/memos/main.go",
            MAIN_BEFORE_SHA256,
            MAIN_AFTER_SHA256,
            MAIN_BEFORE,
            MAIN_AFTER,
        ),
        Mutation::rewrite(
            "web/src/hooks/useLiveMemoRefresh.ts",
            LIVE_REFRESH_BEFORE_SHA256,
            LIVE_REFRESH_AFTER_SHA256,
            SSE_FETCH_BEFORE,
            SSE_FETCH_AFTER,
        ),
        Mutation::executable("sproutos/build.sh", BUILD),
        Mutation::executable("sproutos/run.sh", RUN),
        Mutation::own("store/sproutos_deployment_config.go", STORAGE_BRIDGE),
        Mutation::own("cmd/sproutos-migrate/main.go", MIGRATOR),
        Mutation::own("cmd/sproutos-migrate/main_test.go", MIGRATOR_TEST),
        Mutation::own("web/tests/sproutos-live-polling.test.ts", LIVE_POLLING_TEST),
        Mutation::own(".github/workflows/sproutos-deploy.yml", DEPLOY_WORKFLOW),
    ])
}

fn validate_request(request: &ApplyRequest) -> Result<(), RuntimeError> {
    if request.template.id != ID
        || request
            .template
            .upstream_repository
            .trim_end_matches(".git")
            != REPOSITORY
        || request.template.upstream_commit != COMMIT
    {
        return Err(RuntimeError::UnsupportedUpstream(format!(
            "Memos recipe requires {REPOSITORY}@{COMMIT}"
        )));
    }
    if request.deployment.preset != "web"
        || request.deployment.capabilities
            != [
                "controlled_migrations",
                "generic_web",
                "object_storage",
                "provided_al2023",
            ]
    {
        return Err(RuntimeError::InvalidRequest(
            "Memos requires the web preset with controlled_migrations, generic_web, object_storage, and provided_al2023 capabilities".into(),
        ));
    }
    if !request.user_inputs.is_empty() {
        return Err(RuntimeError::InvalidRequest(
            "Memos v1 recipe has no user inputs".into(),
        ));
    }
    if request.services.len() != 2 {
        return Err(RuntimeError::InvalidRequest(
            "Memos requires managed object storage and Postgres".into(),
        ));
    }
    let object = &request.services[0];
    let expected_object_bindings = [
        ("S3_ACCESS_KEY_ID", ServiceOutput::AccessKeyId),
        ("S3_BUCKET_NAME", ServiceOutput::Bucket),
        ("S3_ENDPOINT", ServiceOutput::Endpoint),
        ("S3_FORCE_PATH_STYLE", ServiceOutput::ForcePathStyle),
        ("S3_REGION", ServiceOutput::Region),
        ("S3_SECRET_ACCESS_KEY", ServiceOutput::SecretAccessKey),
    ];
    if object.key != "object_storage"
        || object.kind != ServiceKind::ObjectStorage
        || object.bindings.len() != expected_object_bindings.len()
        || !object
            .bindings
            .iter()
            .zip(expected_object_bindings)
            .all(|(actual, expected)| {
                actual.environment == expected.0 && actual.output == expected.1
            })
    {
        return Err(RuntimeError::InvalidRequest(
            "Memos object storage bindings do not match the v1 contract".into(),
        ));
    }
    let postgres = &request.services[1];
    if postgres.key != "postgres"
        || postgres.kind != ServiceKind::Postgres
        || postgres.bindings.len() != 1
        || postgres.bindings[0].environment != "MEMOS_DSN"
        || postgres.bindings[0].output != ServiceOutput::ConnectionUrl
    {
        return Err(RuntimeError::InvalidRequest(
            "Memos Postgres must bind connection_url to MEMOS_DSN".into(),
        ));
    }
    if request.generated_inputs.len() != 1 {
        return Err(RuntimeError::InvalidRequest(
            "Memos requires the generated MEMOS_ADMIN_PASSWORD input".into(),
        ));
    }
    let admin_password = &request.generated_inputs[0];
    if admin_password.key != "admin_password"
        || admin_password.environment != "MEMOS_ADMIN_PASSWORD"
        || admin_password.generator != Generator::RandomBase64url
        || admin_password.bytes != 32
    {
        return Err(RuntimeError::InvalidRequest(
            "Memos MEMOS_ADMIN_PASSWORD must be 32 random_base64url bytes".into(),
        ));
    }
    Ok(())
}

fn configuration(request: &ApplyRequest) -> String {
    format!(
        r#"# Generated by the signed SproutOS Memos deployment template. No secret values belong here.
schema_version = 1

[template]
id = "memos"
catalogue_digest = "{}"
manifest_digest = "{}"
plugin_digest = "{}"
upstream_repository = "{}"
upstream_commit = "{}"

[deployment]
preset = "web"
runtime = "provided.al2023"
architecture = "arm64"
capabilities = ["controlled_migrations", "generic_web", "object_storage", "provided_al2023"]
build_command = "sh sproutos/build.sh"
directory = ".sproutos/dist"
handler = "run.sh"
health_path = "/healthz"

[deployment.environment]
MEMOS_DRIVER = "postgres"
MEMOS_DATA = "/tmp/memos"

[deployment.migration]
directory = ".sproutos/migration"
handler = "bootstrap"
runtime = "provided.al2023"

[[services]]
key = "object_storage"
kind = "object_storage"

[[services.bindings]]
environment = "S3_ACCESS_KEY_ID"
output = "access_key_id"

[[services.bindings]]
environment = "S3_BUCKET_NAME"
output = "bucket"

[[services.bindings]]
environment = "S3_ENDPOINT"
output = "endpoint"

[[services.bindings]]
environment = "S3_FORCE_PATH_STYLE"
output = "force_path_style"

[[services.bindings]]
environment = "S3_REGION"
output = "region"

[[services.bindings]]
environment = "S3_SECRET_ACCESS_KEY"
output = "secret_access_key"

[[services]]
key = "postgres"
kind = "postgres"

[[services.bindings]]
environment = "MEMOS_DSN"
output = "connection_url"

[[generated_inputs]]
key = "admin_password"
environment = "MEMOS_ADMIN_PASSWORD"
generator = "random_base64url"
bytes = 32
"#,
        request.template.catalogue_digest,
        request.template.manifest_digest,
        request.template.plugin_digest,
        REPOSITORY,
        COMMIT,
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use sprout_template_protocol::{
        ApplyRequest, Deployment, EnvironmentBinding, GeneratedInput, ManagedService,
        TemplateIdentity,
    };
    use sprout_template_runtime::apply;
    use tempfile::tempdir;

    use super::*;

    const UPSTREAM_MAIN: &[u8] = include_bytes!("../fixtures/upstream-main.go");
    const UPSTREAM_LIVE_REFRESH: &[u8] = include_bytes!("../fixtures/upstream-live-refresh.ts");

    fn request(workspace: &str) -> ApplyRequest {
        ApplyRequest {
            protocol_version: 1,
            workspace: workspace.into(),
            template: TemplateIdentity {
                id: ID.into(),
                catalogue_digest: format!("sha256:{}", "1".repeat(64)),
                manifest_digest: format!("sha256:{}", "2".repeat(64)),
                plugin_digest: format!("sha256:{}", "3".repeat(64)),
                upstream_repository: REPOSITORY.into(),
                upstream_commit: COMMIT.into(),
            },
            deployment: Deployment {
                preset: "web".into(),
                capabilities: vec![
                    "controlled_migrations".into(),
                    "generic_web".into(),
                    "object_storage".into(),
                    "provided_al2023".into(),
                ],
            },
            services: vec![
                ManagedService {
                    key: "object_storage".into(),
                    kind: ServiceKind::ObjectStorage,
                    bindings: vec![
                        ("S3_ACCESS_KEY_ID", ServiceOutput::AccessKeyId),
                        ("S3_BUCKET_NAME", ServiceOutput::Bucket),
                        ("S3_ENDPOINT", ServiceOutput::Endpoint),
                        ("S3_FORCE_PATH_STYLE", ServiceOutput::ForcePathStyle),
                        ("S3_REGION", ServiceOutput::Region),
                        ("S3_SECRET_ACCESS_KEY", ServiceOutput::SecretAccessKey),
                    ]
                    .into_iter()
                    .map(|(environment, output)| EnvironmentBinding {
                        environment: environment.into(),
                        output,
                    })
                    .collect(),
                },
                ManagedService {
                    key: "postgres".into(),
                    kind: ServiceKind::Postgres,
                    bindings: vec![EnvironmentBinding {
                        environment: "MEMOS_DSN".into(),
                        output: ServiceOutput::ConnectionUrl,
                    }],
                },
            ],
            user_inputs: vec![],
            generated_inputs: vec![GeneratedInput {
                key: "admin_password".into(),
                generator: Generator::RandomBase64url,
                bytes: 32,
                environment: "MEMOS_ADMIN_PASSWORD".into(),
            }],
        }
    }

    fn workspace_with_upstream_main() -> tempfile::TempDir {
        let workspace = tempdir().unwrap();
        fs::create_dir_all(workspace.path().join("cmd/memos")).unwrap();
        fs::create_dir_all(workspace.path().join("web/src/hooks")).unwrap();
        fs::write(workspace.path().join("cmd/memos/main.go"), UPSTREAM_MAIN).unwrap();
        fs::write(
            workspace.path().join("web/src/hooks/useLiveMemoRefresh.ts"),
            UPSTREAM_LIVE_REFRESH,
        )
        .unwrap();
        workspace
    }

    #[test]
    fn golden_apply_patches_pinned_source_and_second_apply_is_empty() {
        let workspace = workspace_with_upstream_main();
        let request = request(workspace.path().to_str().unwrap());
        let first = apply(&request, recipe).unwrap();
        assert_eq!(first.len(), 10);
        assert_eq!(
            sprout_template_runtime::sha256(
                &fs::read(workspace.path().join("cmd/memos/main.go")).unwrap()
            ),
            format!("sha256:{MAIN_AFTER_SHA256}")
        );
        let bridge =
            fs::read_to_string(workspace.path().join("store/sproutos_deployment_config.go"))
                .unwrap();
        for environment in super::STORAGE_BRIDGE
            .lines()
            .filter(|line| line.contains("S3_"))
        {
            assert!(bridge.contains(environment));
        }
        assert!(!bridge.contains("os.WriteFile"));
        let workflow = fs::read_to_string(
            workspace
                .path()
                .join(".github/workflows/sproutos-deploy.yml"),
        )
        .unwrap();
        assert_eq!(workflow, DEPLOY_WORKFLOW);
        assert!(workflow.contains("on:\n  push:\n  workflow_dispatch:"));
        assert!(workflow.contains(
            "if: github.ref == format('refs/heads/{0}', github.event.repository.default_branch)"
        ));
        assert!(workflow.contains("permissions:\n      contents: read\n      id-token: write"));
        assert!(workflow.contains("preset: web"));
        assert!(workflow.contains("directory: .sproutos/dist"));
        assert!(workflow.contains("runtime: provided.al2023"));
        assert!(workflow.contains("handler: run.sh"));
        assert!(workflow.contains("migration-directory: .sproutos/migration"));
        assert!(workflow.contains("migration-handler: bootstrap"));
        assert!(workflow.contains("test -x .sproutos/migration/bootstrap"));
        assert_eq!(workflow.matches("contents: read").count(), 2);
        assert_eq!(workflow.matches("id-token: write").count(), 1);
        assert!(workflow.contains("artifact-ids: ${{ needs.build.outputs.artifact-id }}"));
        assert!(workflow.contains("tar -xzf .sproutos/handoff/memos-deploy.tar.gz"));
        assert!(!workflow.contains("secrets."));
        assert!(!workflow.contains("SPROUTOS_TOKEN"));
        assert!(!workflow.contains("\n          token:"));
        assert!(!workflow.contains("\n          project:"));
        assert!(!workflow.contains("/v1/"));
        let action_commit = workflow
            .lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix("uses: MySproutOS/sproutos-deploy-action@")
            })
            .unwrap();
        assert_eq!(action_commit.len(), 40);
        assert!(action_commit.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(action_commit, "0000000000000000000000000000000000000000");
        let config = fs::read_to_string(workspace.path().join(".config/sproutos.toml")).unwrap();
        assert!(config.contains("[deployment.migration]"));
        assert!(config.contains("directory = \".sproutos/migration\""));
        assert!(config.contains("handler = \"bootstrap\""));
        assert!(config.contains("runtime = \"provided.al2023\""));
        assert!(config.contains("key = \"admin_password\""));
        assert!(config.contains("environment = \"MEMOS_ADMIN_PASSWORD\""));
        assert!(!config.contains("MEMOS_ADMIN_PASSWORD ="));
        assert!(!config.contains("realtime_sse"));
        assert!(!config.contains("serialized_startup_migrations"));
        let live_refresh =
            fs::read_to_string(workspace.path().join("web/src/hooks/useLiveMemoRefresh.ts"))
                .unwrap();
        assert_eq!(
            sprout_template_runtime::sha256(live_refresh.as_bytes()),
            format!("sha256:{LIVE_REFRESH_AFTER_SHA256}")
        );
        assert!(!live_refresh.contains("fetch(\"/api/v1/sse\""));
        assert!(live_refresh.contains("SPROUTOS_LIVE_POLL_INTERVAL_MS = 5000"));
        assert!(apply(&request, recipe).unwrap().is_empty());
    }

    #[test]
    fn refuses_wrong_commit_before_writing() {
        let workspace = workspace_with_upstream_main();
        let mut request = request(workspace.path().to_str().unwrap());
        request.template.upstream_commit = "0".repeat(40);
        assert!(matches!(
            apply(&request, recipe),
            Err(RuntimeError::UnsupportedUpstream(_))
        ));
        assert!(!workspace.path().join(".config").exists());
    }

    #[test]
    fn refuses_modified_upstream_before_any_write() {
        let workspace = workspace_with_upstream_main();
        let request = request(workspace.path().to_str().unwrap());
        fs::write(workspace.path().join("cmd/memos/main.go"), "modified\n").unwrap();
        assert!(matches!(
            apply(&request, recipe),
            Err(RuntimeError::ConflictingChange(_))
        ));
        assert!(!workspace.path().join(".config").exists());
    }
}
