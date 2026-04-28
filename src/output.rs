//! Typed output emission — append `name=value` pairs to
//! `$GITHUB_OUTPUT` so the workflow can reference them via
//! `${{ steps.<id>.outputs.<name> }}`.
//!
//! Format reference:
//! https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions#setting-an-output-parameter
//!
//! Multi-line values use the heredoc form:
//! ```text
//! name<<EOF
//! line one
//! line two
//! EOF
//! ```

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{ActionError, ActionResult};

/// Append outputs to `$GITHUB_OUTPUT` (the canonical destination on a
/// real runner). For tests, construct via [`Output::to_file`] pointing
/// at a tempfile.
#[derive(Debug)]
pub struct Output {
    target: PathBuf,
}

impl Output {
    /// Open the runner-supplied `$GITHUB_OUTPUT` path. Errors if the
    /// env var isn't set (the action isn't running inside GitHub
    /// Actions).
    pub fn from_runner_env() -> ActionResult<Self> {
        let path = std::env::var_os("GITHUB_OUTPUT").ok_or_else(|| {
            ActionError::error(
                "GITHUB_OUTPUT env var not set — is this binary running inside GitHub Actions?",
            )
        })?;
        Ok(Self {
            target: PathBuf::from(path),
        })
    }

    /// Construct pointing at an arbitrary file. Used by tests; production
    /// callers use [`Self::from_runner_env`].
    pub fn to_file(path: impl Into<PathBuf>) -> Self {
        Self {
            target: path.into(),
        }
    }

    /// Set a single named output. Single-line values use the
    /// `name=value` form; multi-line values use the heredoc form
    /// with a unique delimiter (so embedded `EOF` strings in the
    /// value don't corrupt the format).
    pub fn set(&self, name: &str, value: impl AsRef<str>) -> ActionResult<()> {
        let value = value.as_ref();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.target)
            .map_err(|e| {
                ActionError::error(format!(
                    "failed to open GITHUB_OUTPUT at {}: {e}",
                    self.target.display()
                ))
            })?;

        let line = if value.contains('\n') {
            let delim = unique_delim();
            format!("{name}<<{delim}\n{value}\n{delim}\n")
        } else {
            format!("{name}={value}\n")
        };

        file.write_all(line.as_bytes()).map_err(|e| {
            ActionError::error(format!(
                "failed to write to GITHUB_OUTPUT at {}: {e}",
                self.target.display()
            ))
        })?;
        Ok(())
    }

    /// Set multiple outputs at once. Each is appended in order.
    pub fn set_all<I, K, V>(&self, pairs: I) -> ActionResult<()>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        for (k, v) in pairs {
            self.set(k.as_ref(), v.as_ref())?;
        }
        Ok(())
    }

    /// Set an output by serializing a value as JSON.
    pub fn set_json<T: serde::Serialize>(&self, name: &str, value: &T) -> ActionResult<()> {
        let json = serde_json::to_string(value).map_err(|e| {
            ActionError::error(format!("failed to JSON-serialize output `{name}`: {e}"))
        })?;
        self.set(name, json)
    }

    /// Returns the target path. For tests + diagnostics.
    pub fn target_path(&self) -> &Path {
        &self.target
    }
}

/// Generate a delimiter that's exceedingly unlikely to appear in the
/// value. We use a process-monotonic counter mixed with a static prefix.
fn unique_delim() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("PLEME_ACTIONS_OUTPUT_DELIM_{n}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn read_to_string(p: &Path) -> String {
        fs::read_to_string(p).expect("output file readable")
    }

    #[test]
    fn single_line_value_uses_kv_form() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output");
        let out = Output::to_file(&path);
        out.set("plan-summary", "+5 -2 ~3").unwrap();
        let written = read_to_string(&path);
        assert_eq!(written, "plan-summary=+5 -2 ~3\n");
    }

    #[test]
    fn multi_line_value_uses_heredoc_form() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output");
        let out = Output::to_file(&path);
        out.set("plan-output", "line one\nline two\nline three").unwrap();
        let written = read_to_string(&path);
        // Should contain heredoc markers
        assert!(written.starts_with("plan-output<<"));
        assert!(written.contains("line one\nline two\nline three\n"));
        // The closing delimiter matches the opening one
        let opener = written.lines().next().unwrap();
        let delim = opener.strip_prefix("plan-output<<").unwrap();
        assert!(written.ends_with(&format!("{delim}\n")));
    }

    #[test]
    fn multiple_set_calls_append() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output");
        let out = Output::to_file(&path);
        out.set("a", "1").unwrap();
        out.set("b", "2").unwrap();
        let written = read_to_string(&path);
        assert_eq!(written, "a=1\nb=2\n");
    }

    #[test]
    fn set_all_appends_in_order() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output");
        let out = Output::to_file(&path);
        out.set_all([("pod-count", "0"), ("listener-status", "Running")])
            .unwrap();
        let written = read_to_string(&path);
        assert_eq!(written, "pod-count=0\nlistener-status=Running\n");
    }

    #[derive(serde::Serialize)]
    struct AppliedResources {
        added: Vec<String>,
        changed: Vec<String>,
    }

    #[test]
    fn set_json_serializes_via_serde() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output");
        let out = Output::to_file(&path);
        out.set_json(
            "applied-resources",
            &AppliedResources {
                added: vec!["aws_iam_role.x".into()],
                changed: vec![],
            },
        )
        .unwrap();
        let written = read_to_string(&path);
        assert_eq!(
            written,
            "applied-resources={\"added\":[\"aws_iam_role.x\"],\"changed\":[]}\n"
        );
    }

    #[test]
    fn from_runner_env_errors_when_unset() {
        // SAFETY: serial test by virtue of the env-var manipulation.
        // Other tests don't touch GITHUB_OUTPUT.
        // SAFETY: required for env var manipulation in 2024 edition
        unsafe { std::env::remove_var("GITHUB_OUTPUT"); }
        let result = Output::from_runner_env();
        let err = result.unwrap_err();
        assert!(err.is_fatal());
        assert!(err.as_workflow_command().contains("GITHUB_OUTPUT env var not set"));
    }
}
