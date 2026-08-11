//! Rules normalization (PRD R4).
//!
//! Projects keep **one** canonical rules/skills source at
//! `.openade/rules.md`. OpenADE materializes it to each harness's native
//! rules file (`CLAUDE.md`, `AGENTS.md`, `GEMINI.md`) so all harnesses see
//! equivalent instructions and behavior does not change when switching
//! models.
//!
//! Generated files carry a marker header; materialization never overwrites a
//! hand-written rules file unless the caller passes `force`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::harness::Harness;

/// Relative path of the canonical rules source inside a project.
pub const CANONICAL_RULES_PATH: &str = ".openade/rules.md";

/// Marker embedded in generated files so we can tell ours from hand-written
/// ones. Do not change without a migration path.
pub const GENERATED_MARKER: &str = "OpenADE:generated";

fn generated_header() -> String {
    format!(
        "<!-- {GENERATED_MARKER} — this file is materialized from {CANONICAL_RULES_PATH}.\n     \
         Edit that file instead; changes here will be overwritten. -->\n\n"
    )
}

/// Why a target file was not (re)written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// The target exists and does not carry our marker — it is hand-written.
    /// Pass `force` to overwrite anyway.
    HandWritten,
    /// The target already has exactly the content we would write.
    UpToDate,
}

/// Outcome of one materialization run.
#[derive(Debug, Default)]
pub struct MaterializeReport {
    /// Files written (created or updated).
    pub written: Vec<PathBuf>,
    /// Files left alone, with the reason.
    pub skipped: Vec<(PathBuf, SkipReason)>,
}

/// Errors from rules materialization.
#[derive(Debug, thiserror::Error)]
pub enum RulesError {
    #[error("canonical rules source not found at {0} — create it first (see docs)")]
    MissingCanonical(PathBuf),
    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn read(path: &Path) -> Result<String, RulesError> {
    fs::read_to_string(path).map_err(|source| RulesError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn write(path: &Path, content: &str) -> Result<(), RulesError> {
    fs::write(path, content).map_err(|source| RulesError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Write the canonical rules source for a project, creating `.openade/`.
pub fn init_canonical_rules(project_root: &Path, content: &str) -> Result<PathBuf, RulesError> {
    let path = project_root.join(CANONICAL_RULES_PATH);
    // CANONICAL_RULES_PATH nests under the project root, so a parent exists.
    let parent = path.parent().expect("canonical rules path has a parent");
    fs::create_dir_all(parent).map_err(|source| RulesError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    write(&path, content)?;
    Ok(path)
}

/// Materialize the canonical rules source to each harness's rules file in
/// `project_root`.
///
/// Idempotent: unchanged targets are reported as [`SkipReason::UpToDate`].
/// Hand-written targets (no [`GENERATED_MARKER`]) are never touched unless
/// `force` is set.
pub fn materialize_rules(
    project_root: &Path,
    harnesses: &[Harness],
    force: bool,
) -> Result<MaterializeReport, RulesError> {
    let canonical_path = project_root.join(CANONICAL_RULES_PATH);
    if !canonical_path.is_file() {
        return Err(RulesError::MissingCanonical(canonical_path));
    }
    let canonical = read(&canonical_path)?;
    let rendered = format!("{}{}", generated_header(), canonical);

    let mut report = MaterializeReport::default();
    for harness in harnesses {
        let target = project_root.join(harness.rules_filename());
        if target.exists() {
            let existing = read(&target)?;
            if existing == rendered {
                report.skipped.push((target, SkipReason::UpToDate));
                continue;
            }
            if !existing.contains(GENERATED_MARKER) && !force {
                report.skipped.push((target, SkipReason::HandWritten));
                continue;
            }
        }
        write(&target, &rendered)?;
        report.written.push(target);
    }
    Ok(report)
}

#[cfg(test)]
#[path = "rules_tests.rs"]
mod tests;
