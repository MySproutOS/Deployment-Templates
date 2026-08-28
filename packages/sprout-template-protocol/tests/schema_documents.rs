use std::{fs, path::PathBuf};

use serde_json::Value;

#[test]
fn every_shipped_schema_is_valid_json_and_compiles() {
    let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = package.join("../../schema/app-manifest-v1.schema.json");
    if !manifest_path.exists() {
        // The published protocol crate intentionally contains only its protocol schemas. Catalogue
        // schemas live in the owning repository and are checked when running from that checkout.
        return;
    }
    let manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).expect("schema exists"))
        .expect("manifest schema is JSON");

    for path in [
        package.join("schema/request-v1.schema.json"),
        package.join("schema/response-v1.schema.json"),
        manifest_path,
        package.join("../../schema/provenance-v1.schema.json"),
    ] {
        let document: Value = serde_json::from_slice(&fs::read(&path).expect("schema exists"))
            .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()));
        jsonschema::validator_for(&document)
            .unwrap_or_else(|error| panic!("{} does not compile: {error}", path.display()));
    }

    let catalogue_path = package.join("../../schema/catalogue-v1.schema.json");
    let catalogue: Value =
        serde_json::from_slice(&fs::read(&catalogue_path).expect("schema exists"))
            .expect("catalogue schema is JSON");
    jsonschema::options()
        .with_resource(
            "https://schemas.sproutos.me/deployment-templates/app-manifest-v1.schema.json",
            jsonschema::Resource::from_contents(manifest).expect("manifest schema is a resource"),
        )
        .build(&catalogue)
        .unwrap_or_else(|error| panic!("{} does not compile: {error}", catalogue_path.display()));
}
