use std::{
    fs,
    path::{Path, PathBuf},
};

use catalogue_generator::{GenerateOptions, generate};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("package is two levels below repository root")
        .to_owned()
}

fn copy_file(from: impl AsRef<Path>, to: impl AsRef<Path>) {
    let to = to.as_ref();
    fs::create_dir_all(to.parent().expect("fixture target parent")).unwrap();
    fs::copy(from, to).unwrap();
}

fn fixture() -> (TempDir, GenerateOptions) {
    let root = repository_root();
    let temp = tempfile::tempdir().unwrap();
    for id in ["umami", "memos"] {
        let manifest_path = temp.path().join(format!("apps/{id}/manifest-source.json"));
        copy_file(
            root.join(format!("apps/{id}/manifest-source.json")),
            &manifest_path,
        );
        let mut manifest = read_json(&manifest_path);
        let blocked_reason = match id {
            "memos" => {
                "The pinned Memos recipe has not completed a recorded production end-to-end pass covering controlled migration, fail-closed administrator bootstrap, first launch, database and attachment persistence across a second deployment, and the visible-tab polling lifecycle."
            }
            "umami" => {
                "The pinned Umami recipe has not completed a recorded production end-to-end pass covering its controlled migration, first publication, serving health, and a second deployment with persisted data."
            }
            _ => unreachable!(),
        };
        manifest["readiness"] = json!({"status":"blocked","blocked_reasons":[blocked_reason]});
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    }
    for name in [
        "app-manifest-v1.schema.json",
        "catalogue-v1.schema.json",
        "provenance-v1.schema.json",
    ] {
        copy_file(
            root.join("schema").join(name),
            temp.path().join("schema").join(name),
        );
    }
    for name in ["request-v1.schema.json", "response-v1.schema.json"] {
        copy_file(
            root.join("packages/sprout-template-protocol/schema")
                .join(name),
            temp.path().join("protocol").join(name),
        );
    }
    for path in [
        temp.path().join("protocol-source/src/lib.rs"),
        temp.path().join("plugins/memos/src/main.rs"),
        temp.path().join("plugins/umami/src/main.rs"),
    ] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"// deterministic source fixture\n").unwrap();
    }
    copy_file(
        root.join("tests/fixtures/plugin-lock.json"),
        temp.path().join("catalogue/plugin-lock.json"),
    );

    let options = GenerateOptions {
        apps_dir: temp.path().join("apps"),
        plugin_lock: temp.path().join("catalogue/plugin-lock.json"),
        output: temp.path().join("catalogue/catalogue.json"),
        provenance_output: temp.path().join("catalogue/provenance.json"),
        manifest_schema: temp.path().join("schema/app-manifest-v1.schema.json"),
        catalogue_schema: temp.path().join("schema/catalogue-v1.schema.json"),
        provenance_schema: temp.path().join("schema/provenance-v1.schema.json"),
        protocol_schema_dir: temp.path().join("protocol"),
        protocol_source_dir: temp.path().join("protocol-source"),
        plugin_source_dir: temp.path().join("plugins"),
        e2e_proof_dir: temp.path().join("catalogue/e2e-proofs"),
        source_repository: "MySproutOS/Deployment-Templates".into(),
        source_workflow: ".github/workflows/publish.yml".into(),
        source_ref: "refs/heads/main".into(),
        source_commit: COMMIT.into(),
    };
    (temp, options)
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[test]
fn generates_sorted_exact_plan_catalogue_and_acyclic_provenance() {
    let (_temp, options) = fixture();
    generate(&options).unwrap();

    let catalogue_bytes = fs::read(&options.output).unwrap();
    let first_catalogue = catalogue_bytes.clone();
    let catalogue = read_json(&options.output);
    let provenance = read_json(&options.provenance_output);
    let upstream_lock = read_json(&repository_root().join("tests/upstream-lock.json"));

    assert_eq!(catalogue["generated_from_commit"], COMMIT);
    assert_eq!(catalogue["apps"][0]["id"], "memos");
    assert_eq!(catalogue["apps"][1]["id"], "umami");
    assert!(catalogue.get("provenance").is_none());
    assert_eq!(provenance["subject"]["kind"], "catalogue");
    assert_eq!(provenance["subject"]["digest"], sha256(&catalogue_bytes));
    assert_eq!(
        provenance["source_commit"],
        catalogue["generated_from_commit"]
    );

    let memos = &catalogue["apps"][0];
    assert_eq!(
        memos["repository"]["commit"],
        "22a5f3385b9fc5bdf282eb597aa3db79798aa5ab"
    );
    assert_eq!(
        memos["repository"]["url"],
        upstream_lock["memos"]["repository"]
    );
    assert_eq!(
        memos["repository"]["commit"],
        upstream_lock["memos"]["commit"]
    );
    assert_eq!(memos["deployment"]["preset"], "web");
    assert_eq!(memos["deployment"]["runtime"], "provided.al2023");
    assert_eq!(
        memos["deployment"]["required_capabilities"],
        json!([
            "controlled_migrations",
            "generic_web",
            "object_storage",
            "provided_al2023"
        ])
    );
    assert_eq!(
        memos["deployment"]["migration"]["path"],
        ".sproutos/migration/bootstrap"
    );
    assert_eq!(memos["services"][0]["kind"], "object_storage");
    assert_eq!(
        memos["services"][1]["bindings"][0]["environment"],
        "MEMOS_DSN"
    );
    assert_eq!(
        memos["generated_inputs"][0],
        json!({
            "bytes": 32,
            "environment": "MEMOS_ADMIN_PASSWORD",
            "generator": "random_base64url",
            "key": "admin_password"
        })
    );
    assert_eq!(memos["readiness"]["status"], "blocked");
    assert!(memos["readiness"]["e2e_evidence"].is_null());
    assert!(
        memos["description_md"]
            .as_str()
            .unwrap()
            .contains("\n\nThis recipe builds the exact pinned upstream commit")
    );
    let memos_blockers = memos["readiness"]["blocked_reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reason| reason.as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(memos_blockers.len(), 1);
    assert!(memos_blockers[0].contains("recorded production end-to-end pass"));
    assert!(
        memos["description_md"]
            .as_str()
            .unwrap()
            .contains("visible-tab polling")
    );
    assert!(
        memos["description_md"]
            .as_str()
            .unwrap()
            .contains("sole initial account")
    );
    assert!(
        memos["description_md"]
            .as_str()
            .unwrap()
            .contains("Public registration starts disabled")
    );
    assert!(
        memos["description_md"]
            .as_str()
            .unwrap()
            .contains("never reset an owner-changed password")
    );
    assert!(
        memos["description_md"]
            .as_str()
            .unwrap()
            .contains("eventual refresh rather than instantaneous realtime delivery")
    );
    assert!(
        memos_blockers
            .iter()
            .all(|reason| !reason.contains("object-storage capabilities are not production-ready"))
    );

    let umami = &catalogue["apps"][1];
    assert_eq!(
        umami["repository"]["commit"],
        "ca661c7057984aa98ed4f7083d84dae2f65bfcb0"
    );
    assert_eq!(
        umami["repository"]["url"],
        upstream_lock["umami"]["repository"]
    );
    assert_eq!(
        umami["repository"]["commit"],
        upstream_lock["umami"]["commit"]
    );
    assert_eq!(umami["name"], "Umami");
    assert_eq!(
        umami["pitch"],
        "A simple, fast, privacy-focused alternative to Google Analytics."
    );
    assert_eq!(umami["homepage"], "https://umami.is");
    assert_eq!(umami["license"], "MIT");
    assert_eq!(umami["deployment"]["preset"], "next");
    assert_eq!(umami["deployment"]["runtime"], "nodejs22.x");
    assert_eq!(
        umami["deployment"]["required_capabilities"],
        json!(["controlled_migrations", "next_standalone"])
    );
    assert_eq!(
        umami["deployment"]["migration"]["path"],
        ".sproutos/build/migration/index.mjs"
    );
    assert_eq!(
        umami["services"][0]["bindings"][0]["environment"],
        "DATABASE_URL"
    );
    assert_eq!(
        umami["generated_inputs"],
        json!([
            {
                "bytes": 32,
                "environment": "UMAMI_ADMIN_PASSWORD",
                "generator": "random_base64url",
                "key": "admin_password"
            },
            {
                "bytes": 32,
                "environment": "APP_SECRET",
                "generator": "random_base64url",
                "key": "app_secret"
            }
        ])
    );
    assert_eq!(
        umami["plugin"]["repository"],
        "ghcr.io/mysproutos/umami-plugin"
    );
    assert_eq!(
        umami["plugin"]["digest"],
        format!("sha256:{}", "1".repeat(64))
    );
    assert_eq!(umami["readiness"]["status"], "blocked");
    assert!(umami["readiness"]["e2e_evidence"].is_null());
    assert!(
        umami["description_md"]
            .as_str()
            .unwrap()
            .contains("## First sign-in")
    );
    assert!(
        umami["description_md"]
            .as_str()
            .unwrap()
            .contains("`UMAMI_ADMIN_PASSWORD`")
    );
    assert_eq!(
        umami["readiness"]["blocked_reasons"],
        json!([
            "The pinned Umami recipe has not completed a recorded production end-to-end pass covering its controlled migration, first publication, serving health, and a second deployment with persisted data."
        ])
    );

    let material_uris = provenance["materials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|material| material["uri"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(material_uris.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(
        material_uris
            .iter()
            .any(|uri| uri.ends_with("request-v1.schema.json"))
    );
    assert!(
        material_uris
            .iter()
            .any(|uri| uri.ends_with("manifest-source.json"))
    );
    assert!(
        material_uris
            .iter()
            .any(|uri| uri.starts_with("ghcr.io/mysproutos/"))
    );

    generate(&options).unwrap();
    assert_eq!(fs::read(&options.output).unwrap(), first_catalogue);
}

#[test]
fn refuses_live_without_detached_e2e_proof_and_preserves_last_good_outputs() {
    let (_temp, options) = fixture();
    fs::write(&options.output, b"last-good-catalogue").unwrap();
    fs::write(&options.provenance_output, b"last-good-provenance").unwrap();
    let path = options.apps_dir.join("umami/manifest-source.json");
    let mut manifest = read_json(&path);
    manifest["readiness"] = json!({"status":"live","blocked_reasons":[]});
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let error = generate(&options).unwrap_err().to_string();
    assert!(
        error.contains("cannot be live without detached end-to-end proof"),
        "{error}"
    );
    assert_eq!(fs::read(&options.output).unwrap(), b"last-good-catalogue");
    assert_eq!(
        fs::read(&options.provenance_output).unwrap(),
        b"last-good-provenance"
    );
}

#[test]
fn rejects_tags_malformed_digests_missing_and_orphan_locks() {
    let cases = [
        "ghcr.io/mysproutos/umami-plugin:latest",
        "ghcr.io/mysproutos/umami-plugin@sha256:abcd",
        "ghcr.io/other/umami-plugin@sha256:1111111111111111111111111111111111111111111111111111111111111111",
        "ghcr.io/mysproutos/umami-plugin:tag@sha256:1111111111111111111111111111111111111111111111111111111111111111",
    ];
    for artifact in cases {
        let (_temp, options) = fixture();
        let mut lock = read_json(&options.plugin_lock);
        lock["plugins"]["umami"]["artifact"] = artifact.into();
        fs::write(&options.plugin_lock, serde_json::to_vec(&lock).unwrap()).unwrap();
        assert!(generate(&options).is_err(), "accepted {artifact}");
    }

    let (_temp, options) = fixture();
    let mut lock = read_json(&options.plugin_lock);
    lock["plugins"].as_object_mut().unwrap().remove("umami");
    fs::write(&options.plugin_lock, serde_json::to_vec(&lock).unwrap()).unwrap();
    assert!(
        generate(&options)
            .unwrap_err()
            .to_string()
            .contains("missing plugin lock")
    );

    let (_temp, options) = fixture();
    let mut lock = read_json(&options.plugin_lock);
    lock["plugins"]["orphan"] = json!({
        "artifact": format!("ghcr.io/mysproutos/orphan-plugin@sha256:{}", "3".repeat(64))
    });
    fs::write(&options.plugin_lock, serde_json::to_vec(&lock).unwrap()).unwrap();
    assert!(
        generate(&options)
            .unwrap_err()
            .to_string()
            .contains("do not exactly match")
    );
}

#[test]
fn rejects_source_authored_plugin_reference_duplicate_ids_and_duplicate_bindings() {
    let (_temp, options) = fixture();
    let path = options.apps_dir.join("umami/manifest-source.json");
    let mut manifest = read_json(&path);
    manifest["plugin"]["digest"] = format!("sha256:{}", "1".repeat(64)).into();
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let error = format!("{:#}", generate(&options).unwrap_err());
    assert!(error.contains("unknown field `digest`"), "{error}");

    let (_temp, options) = fixture();
    let duplicate = options.apps_dir.join("zz-duplicate/manifest-source.json");
    copy_file(
        options.apps_dir.join("umami/manifest-source.json"),
        &duplicate,
    );
    let error = generate(&options).unwrap_err().to_string();
    assert!(error.contains("duplicate app id 'umami'"), "{error}");

    let (_temp, options) = fixture();
    let path = options.apps_dir.join("memos/manifest-source.json");
    let mut manifest = read_json(&path);
    manifest["services"][1]["bindings"][0]["environment"] = "S3_ENDPOINT".into();
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let error = generate(&options).unwrap_err().to_string();
    assert!(
        error.contains("duplicate environment binding 'S3_ENDPOINT'"),
        "{error}"
    );
}

#[test]
fn canonicalizes_source_order_but_rejects_duplicate_capabilities() {
    let (_temp, options) = fixture();
    let path = options.apps_dir.join("umami/manifest-source.json");
    let mut manifest = read_json(&path);
    manifest["deployment"]["required_capabilities"] =
        json!(["next_standalone", "controlled_migrations"]);
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    generate(&options).unwrap();
    assert_eq!(
        read_json(&options.output)["apps"][1]["deployment"]["required_capabilities"],
        json!(["controlled_migrations", "next_standalone"])
    );

    manifest["deployment"]["required_capabilities"] = json!(["next_standalone", "next_standalone"]);
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(
        generate(&options)
            .unwrap_err()
            .to_string()
            .contains("must contain unique values")
    );
}

#[test]
fn checked_in_catalogue_and_provenance_goldens_are_current() {
    let root = repository_root();
    let temp = tempfile::tempdir().unwrap();
    let options = GenerateOptions {
        apps_dir: root.join("apps"),
        plugin_lock: root.join("tests/fixtures/live-plugin-lock.json"),
        output: temp.path().join("catalogue.json"),
        provenance_output: temp.path().join("provenance.json"),
        manifest_schema: root.join("schema/app-manifest-v1.schema.json"),
        catalogue_schema: root.join("schema/catalogue-v1.schema.json"),
        provenance_schema: root.join("schema/provenance-v1.schema.json"),
        protocol_schema_dir: root.join("packages/sprout-template-protocol/schema"),
        protocol_source_dir: root.join("packages/sprout-template-protocol"),
        plugin_source_dir: root.join("plugins"),
        e2e_proof_dir: root.join("catalogue/e2e-proofs"),
        source_repository: "MySproutOS/Deployment-Templates".into(),
        source_workflow: ".github/workflows/publish.yml".into(),
        source_ref: "refs/heads/main".into(),
        source_commit: COMMIT.into(),
    };
    generate(&options).unwrap();
    assert_eq!(
        fs::read(&options.output).unwrap(),
        fs::read(root.join("tests/fixtures/catalogue.json")).unwrap()
    );
    assert_eq!(
        fs::read(&options.provenance_output).unwrap(),
        fs::read(root.join("tests/fixtures/provenance.json")).unwrap()
    );
}

#[test]
fn live_requires_bound_evidence_bytes_and_emits_only_verified_evidence() {
    let (_temp, options) = fixture();
    let manifest_path = options.apps_dir.join("umami/manifest-source.json");
    let mut manifest = read_json(&manifest_path);
    manifest["readiness"] = json!({"status":"live","blocked_reasons":[]});
    fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();

    let evidence_path = options
        .e2e_proof_dir
        .join("artifacts/umami-attestation.json");
    fs::create_dir_all(evidence_path.parent().unwrap()).unwrap();
    let evidence = br#"{"conclusion":"success","kind":"recorded-live-deploy"}"#;
    fs::write(&evidence_path, evidence).unwrap();
    fs::write(
        options.e2e_proof_dir.join("umami.json"),
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "app_id": "umami",
            "upstream_commit": "ca661c7057984aa98ed4f7083d84dae2f65bfcb0",
            "plugin_digest": format!("sha256:{}", "1".repeat(64)),
            "preset": "next",
            "required_capabilities": ["controlled_migrations", "next_standalone"],
            "passed": true,
            "workflow_run_url": "https://github.com/MySproutOS/Deployment-Templates/actions/runs/12345",
            "tested_at": "2026-08-28T12:00:00Z",
            "evidence_path": "artifacts/umami-attestation.json",
            "evidence_digest": sha256(evidence)
        }))
        .unwrap(),
    )
    .unwrap();

    generate(&options).unwrap();
    let catalogue = read_json(&options.output);
    assert_eq!(catalogue["apps"][1]["readiness"]["status"], "live");
    assert_eq!(
        catalogue["apps"][1]["readiness"]["e2e_evidence"]["plugin_digest"],
        format!("sha256:{}", "1".repeat(64))
    );
    assert!(
        read_json(&options.provenance_output)["materials"]
            .as_array()
            .unwrap()
            .iter()
            .any(|material| material["uri"]
                == "catalogue/e2e-proofs/artifacts/umami-attestation.json")
    );

    fs::write(&evidence_path, b"tampered").unwrap();
    let error = generate(&options).unwrap_err().to_string();
    assert!(
        error.contains("E2E evidence digest does not match"),
        "{error}"
    );
}

#[test]
fn validates_spdx_retired_semantics_and_rfc8785_unicode() {
    let (_temp, options) = fixture();
    let path = options.apps_dir.join("memos/manifest-source.json");
    let mut manifest = read_json(&path);
    manifest["license"] = "definitely not an SPDX expression".into();
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let error = format!("{:#}", generate(&options).unwrap_err());
    assert!(error.contains("valid SPDX expression"), "{error}");

    manifest["license"] = "MIT OR Apache-2.0".into();
    manifest["readiness"] = json!({"status":"retired","blocked_reasons":["old"]});
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let error = generate(&options).unwrap_err().to_string();
    assert!(
        error.contains("retired readiness cannot contain blocked reasons"),
        "{error}"
    );

    manifest["readiness"] = json!({"status":"blocked","blocked_reasons":["Still blocked."]});
    manifest["name"] = "Mémos \"Notes\"".into();
    fs::write(&path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    generate(&options).unwrap();
    let first = fs::read(&options.output).unwrap();
    assert!(
        std::str::from_utf8(&first)
            .unwrap()
            .contains("Mémos \\\"Notes\\\"")
    );
    assert!(!std::str::from_utf8(&first).unwrap().contains("\\u00e9"));
    generate(&options).unwrap();
    assert_eq!(first, fs::read(&options.output).unwrap());
}
