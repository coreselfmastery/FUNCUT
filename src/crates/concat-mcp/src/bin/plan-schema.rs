// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The plan-schema generator's command line (SCH-02): regenerate the two
//! committed plan schemas from the plan types and the template.
//!
//! Run from `src/`:
//!
//! ```text
//! cargo run -p concat-mcp --bin plan-schema            # regenerate + write
//! cargo run -p concat-mcp --bin plan-schema -- --check # fail if stale
//! ```
//!
//! CI runs the same comparison as the `plan_schema` module's drift test;
//! this binary exists for the developer whose editor just rejected a
//! hand edit and for a workflow step that wants the diff on stdout.

use std::{env, fs, path::PathBuf, process::ExitCode};

fn main() -> ExitCode {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // src/crates/concat-mcp sits three levels under the repository root.
    let root = manifest
        .ancestors()
        .nth(3)
        .expect("the crate sits three levels under the root")
        .to_path_buf();
    let check = env::args().any(|flag| flag == "--check");
    let canonical_path =
        root.join(".claude/skills/funcut-director/templates/edit_plan.schema.json");
    let compact_path = manifest.join("schema/plan_validate.schema.json");

    let results = [
        write_or_check(
            &canonical_path,
            &concat_mcp::plan_schema::canonical_pretty(),
            check,
            "the canonical edit-plan schema",
        ),
        write_or_check(
            &compact_path,
            &concat_mcp::plan_schema::compact_pretty(),
            check,
            "the plan_validate input schema",
        ),
    ];
    let failures: Vec<String> = results
        .into_iter()
        .filter_map(|result| result.err())
        .collect();
    if failures.is_empty() {
        if check {
            println!("the committed plan schemas are what the types generate");
        } else {
            println!("regenerated the committed plan schemas from the types");
        }
        ExitCode::SUCCESS
    } else {
        for failure in failures {
            eprintln!("{failure}");
        }
        eprintln!(
            "run `cargo run -p concat-mcp --bin plan-schema` from src/ to reconcile the artifacts"
        );
        ExitCode::FAILURE
    }
}

/// Write the regenerated artifact, or - with `--check` - refuse when the
/// committed file is not what the generator produces.
fn write_or_check(
    path: &std::path::Path,
    content: &str,
    check: bool,
    name: &str,
) -> Result<(), String> {
    if check {
        match fs::read_to_string(path) {
            Ok(current) if current == content => Ok(()),
            Ok(current) => Err(format!(
                "{name} is stale: {} bytes committed, {} bytes regenerated ({})",
                current.len(),
                content.len(),
                path.display()
            )),
            Err(error) => Err(format!(
                "{name} could not be read for the check: {error} ({})",
                path.display()
            )),
        }
    } else {
        fs::write(path, content)
            .map_err(|error| format!("{name} could not be written: {error} ({})", path.display()))
    }
}
