use std::path::PathBuf;

use anyhow::Result;
use catalogue_generator::{GenerateOptions, generate};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "catalogue-generator",
    about = "Build the deterministic SproutOS deployment-template catalogue"
)]
struct Arguments {
    #[arg(long, default_value = "apps")]
    apps_dir: PathBuf,

    #[arg(long)]
    plugin_lock: PathBuf,

    #[arg(long)]
    output: PathBuf,

    #[arg(long)]
    provenance_output: PathBuf,

    #[arg(long, default_value = "schema")]
    schema_dir: PathBuf,

    #[arg(long, default_value = "packages/sprout-template-protocol/schema")]
    protocol_schema_dir: PathBuf,

    #[arg(long, default_value = "packages/sprout-template-protocol")]
    protocol_source_dir: PathBuf,

    #[arg(long, default_value = "plugins")]
    plugin_source_dir: PathBuf,

    #[arg(long, default_value = "catalogue/e2e-proofs")]
    e2e_proof_dir: PathBuf,

    #[arg(long)]
    source_repository: String,

    #[arg(long)]
    source_workflow: String,

    #[arg(long)]
    source_ref: String,

    #[arg(long)]
    source_commit: String,
}

fn main() -> Result<()> {
    let arguments = Arguments::parse();
    generate(&GenerateOptions {
        apps_dir: arguments.apps_dir,
        plugin_lock: arguments.plugin_lock,
        output: arguments.output,
        provenance_output: arguments.provenance_output,
        manifest_schema: arguments.schema_dir.join("app-manifest-v1.schema.json"),
        catalogue_schema: arguments.schema_dir.join("catalogue-v1.schema.json"),
        provenance_schema: arguments.schema_dir.join("provenance-v1.schema.json"),
        protocol_schema_dir: arguments.protocol_schema_dir,
        protocol_source_dir: arguments.protocol_source_dir,
        plugin_source_dir: arguments.plugin_source_dir,
        e2e_proof_dir: arguments.e2e_proof_dir,
        source_repository: arguments.source_repository,
        source_workflow: arguments.source_workflow,
        source_ref: arguments.source_ref,
        source_commit: arguments.source_commit,
    })?;
    Ok(())
}
