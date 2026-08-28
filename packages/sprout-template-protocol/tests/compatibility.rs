use std::{fs, path::PathBuf};

use serde_json::Value;
use sprout_template_protocol::{
    ApplyRequest, ApplyResponse, ProtocolParseError, Validate, parse_request, parse_response,
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn bytes(relative: &str) -> Vec<u8> {
    fs::read(root().join(relative)).expect("fixture must be readable")
}

fn value(relative: &str) -> Value {
    serde_json::from_slice(&bytes(relative)).expect("fixture must be JSON")
}

fn schema(relative: &str) -> Value {
    value(relative)
}

#[test]
fn valid_request_is_schema_and_semantically_valid() {
    let fixture = value("fixtures/v1/valid/apply-request.json");
    let validator = jsonschema::validator_for(&schema("schema/request-v1.schema.json"))
        .expect("request schema compiles");
    assert!(validator.is_valid(&fixture));

    let request = parse_request(&bytes("fixtures/v1/valid/apply-request.json"))
        .expect("valid fixture parses");
    request.validate().expect("valid fixture validates");
    assert_eq!(serde_json::to_value(request).unwrap(), fixture);
}

#[test]
fn valid_responses_are_schema_and_semantically_valid() {
    let validator = jsonschema::validator_for(&schema("schema/response-v1.schema.json"))
        .expect("response schema compiles");
    for name in [
        "changed-response.json",
        "idempotent-response.json",
        "error-response.json",
    ] {
        let relative = format!("fixtures/v1/valid/{name}");
        let fixture = value(&relative);
        assert!(validator.is_valid(&fixture), "schema rejected {name}");
        let response = parse_response(&bytes(&relative)).expect("valid fixture parses");
        response.validate().expect("valid fixture validates");
        assert_eq!(serde_json::to_value(response).unwrap(), fixture);
    }
}

#[test]
fn schema_rejects_fields_which_could_carry_values_or_debug_data() {
    let request_validator = jsonschema::validator_for(&schema("schema/request-v1.schema.json"))
        .expect("request schema compiles");
    let secret = value("fixtures/v1/invalid/request-secret-value.json");
    assert!(!request_validator.is_valid(&secret));
    assert!(serde_json::from_value::<ApplyRequest>(secret).is_err());

    let response_validator = jsonschema::validator_for(&schema("schema/response-v1.schema.json"))
        .expect("response schema compiles");
    let extra = value("fixtures/v1/invalid/response-extra-field.json");
    assert!(!response_validator.is_valid(&extra));
    assert!(serde_json::from_value::<ApplyResponse>(extra).is_err());
}

#[test]
fn semantic_validation_rejects_cross_collection_collisions() {
    let fixture = value("fixtures/v1/invalid/request-duplicate-environment.json");
    let validator = jsonschema::validator_for(&schema("schema/request-v1.schema.json"))
        .expect("request schema compiles");
    assert!(
        validator.is_valid(&fixture),
        "collision is deliberately semantic"
    );
    assert!(
        parse_request(&bytes(
            "fixtures/v1/invalid/request-duplicate-environment.json"
        ))
        .is_err()
    );
}

#[test]
fn unsupported_version_never_becomes_a_valid_request() {
    let error = parse_request(&bytes("fixtures/v1/invalid/request-v2.json"))
        .expect_err("v2 must not validate through the v1 parser");
    assert!(matches!(error, ProtocolParseError::Validation(_)));
}

#[test]
fn unsafe_and_incomplete_changes_are_rejected() {
    for name in [
        "response-path-traversal.json",
        "response-git-path.json",
        "response-digest-kind-mismatch.json",
        "response-omitted-digest.json",
    ] {
        assert!(
            parse_response(&bytes(&format!("fixtures/v1/invalid/{name}"))).is_err(),
            "accepted {name}"
        );
    }
}

#[test]
fn workspace_absolute_path_is_host_aware() {
    let mut fixture = value("fixtures/v1/valid/apply-request.json");
    fixture["workspace"] = Value::String("relative/workspace".into());
    assert!(parse_request(&serde_json::to_vec(&fixture).unwrap()).is_err());

    #[cfg(windows)]
    {
        fixture["workspace"] = Value::String(r"C:\workspace".into());
        assert!(parse_request(&serde_json::to_vec(&fixture).unwrap()).is_ok());
    }
    #[cfg(not(windows))]
    {
        fixture["workspace"] = Value::String("/workspace".into());
        assert!(parse_request(&serde_json::to_vec(&fixture).unwrap()).is_ok());
    }
}
