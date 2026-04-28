//! `StepSummary` — typed builder for `$GITHUB_STEP_SUMMARY` markdown.
//!
//! GitHub Actions reads this file at end-of-step and renders it as
//! markdown in the run UI. Every pleme-io action emits a structured
//! summary so operators have a quick read of what happened without
//! scrolling the raw step output.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{ActionError, ActionResult};

/// Builder for the step-summary markdown. Buffer the markdown via the
/// declarative methods, then call [`StepSummary::commit`] once at the
/// end to flush to `$GITHUB_STEP_SUMMARY`.
#[derive(Debug)]
pub struct StepSummary {
    target: PathBuf,
    buf: String,
}

impl StepSummary {
    /// Open the runner-supplied `$GITHUB_STEP_SUMMARY` path.
    pub fn from_runner_env() -> ActionResult<Self> {
        let path = std::env::var_os("GITHUB_STEP_SUMMARY").ok_or_else(|| {
            ActionError::error(
                "GITHUB_STEP_SUMMARY env var not set — is this binary running inside GitHub Actions?",
            )
        })?;
        Ok(Self {
            target: PathBuf::from(path),
            buf: String::new(),
        })
    }

    /// Construct pointing at an arbitrary file. Used by tests.
    pub fn to_file(path: impl Into<PathBuf>) -> Self {
        Self {
            target: path.into(),
            buf: String::new(),
        }
    }

    /// `# Heading` (level 1) → `###### Heading` (level 6).
    pub fn heading(&mut self, level: usize, text: &str) -> &mut Self {
        let level = level.clamp(1, 6);
        for _ in 0..level {
            self.buf.push('#');
        }
        self.buf.push(' ');
        self.buf.push_str(text);
        self.buf.push_str("\n\n");
        self
    }

    /// Plain paragraph. Trailing blank line emitted.
    pub fn paragraph(&mut self, text: &str) -> &mut Self {
        self.buf.push_str(text);
        self.buf.push_str("\n\n");
        self
    }

    /// Inline code block fenced with the given language tag.
    pub fn code_block(&mut self, lang: &str, body: &str) -> &mut Self {
        self.buf.push_str("```");
        self.buf.push_str(lang);
        self.buf.push('\n');
        self.buf.push_str(body);
        if !body.ends_with('\n') {
            self.buf.push('\n');
        }
        self.buf.push_str("```\n\n");
        self
    }

    /// Markdown table. `headers` becomes the column titles; each row
    /// in `rows` is a sequence of cells matching the header count.
    pub fn table<I, R, S>(&mut self, headers: &[&str], rows: I) -> &mut Self
    where
        I: IntoIterator<Item = R>,
        R: AsRef<[S]>,
        S: AsRef<str>,
    {
        self.buf.push('|');
        for h in headers {
            self.buf.push(' ');
            self.buf.push_str(h);
            self.buf.push_str(" |");
        }
        self.buf.push_str("\n|");
        for _ in headers {
            self.buf.push_str("---|");
        }
        self.buf.push('\n');
        for row in rows {
            self.buf.push('|');
            for cell in row.as_ref() {
                self.buf.push(' ');
                self.buf.push_str(cell.as_ref());
                self.buf.push_str(" |");
            }
            self.buf.push('\n');
        }
        self.buf.push('\n');
        self
    }

    /// Bullet list.
    pub fn bullets<I, S>(&mut self, items: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for item in items {
            self.buf.push_str("- ");
            self.buf.push_str(item.as_ref());
            self.buf.push('\n');
        }
        self.buf.push('\n');
        self
    }

    /// Append a status badge line (✓ / ✗ / ⊘ etc.). Use semantic
    /// constants for consistency across actions.
    pub fn status(&mut self, text: &str) -> &mut Self {
        self.buf.push_str(text);
        self.buf.push_str("\n\n");
        self
    }

    /// Returns the accumulated markdown buffer (peek without flushing).
    pub fn as_str(&self) -> &str {
        &self.buf
    }

    /// Flush the accumulated markdown to `$GITHUB_STEP_SUMMARY`.
    /// Appends rather than overwrites so multiple `commit()` calls
    /// (or multiple steps emitting summaries) compose cleanly.
    pub fn commit(self) -> ActionResult<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.target)
            .map_err(|e| {
                ActionError::error(format!(
                    "failed to open GITHUB_STEP_SUMMARY at {}: {e}",
                    self.target.display()
                ))
            })?;
        file.write_all(self.buf.as_bytes()).map_err(|e| {
            ActionError::error(format!(
                "failed to write to GITHUB_STEP_SUMMARY at {}: {e}",
                self.target.display()
            ))
        })?;
        Ok(())
    }

    /// Returns the target path. For tests + diagnostics.
    pub fn target_path(&self) -> &Path {
        &self.target
    }
}

/// Common status badges. Use these instead of raw emoji strings so
/// every action's summary uses the same visual language.
pub mod status {
    pub const PASSED: &str = "✓ Passed";
    pub const FAILED: &str = "✗ Failed";
    pub const SKIPPED: &str = "⊘ Skipped";
    pub const PAUSED: &str = "❚❚ Paused";
    pub const PENDING: &str = "… Pending";
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn read_to_string(p: &Path) -> String {
        fs::read_to_string(p).expect("summary file readable")
    }

    #[test]
    fn empty_summary_writes_nothing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summary");
        let s = StepSummary::to_file(&path);
        s.commit().unwrap();
        assert_eq!(read_to_string(&path), "");
    }

    #[test]
    fn heading_levels_clamp_to_1_through_6() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summary");
        let mut s = StepSummary::to_file(&path);
        s.heading(0, "below").heading(7, "above").heading(3, "ok");
        s.commit().unwrap();
        let out = read_to_string(&path);
        assert!(out.contains("# below\n"));
        assert!(out.contains("###### above\n"));
        assert!(out.contains("### ok\n"));
    }

    #[test]
    fn table_renders_with_separators() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summary");
        let mut s = StepSummary::to_file(&path);
        s.table(
            &["Metric", "Count"],
            vec![
                vec!["pod-count", "0"],
                vec!["listener-status", "Running"],
            ],
        );
        s.commit().unwrap();
        let out = read_to_string(&path);
        assert!(out.contains("| Metric | Count |\n|---|---|\n"));
        assert!(out.contains("| pod-count | 0 |\n"));
        assert!(out.contains("| listener-status | Running |\n"));
    }

    #[test]
    fn code_block_fences_with_lang_tag() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summary");
        let mut s = StepSummary::to_file(&path);
        s.code_block("yaml", "key: value\nnumber: 42");
        s.commit().unwrap();
        let out = read_to_string(&path);
        assert!(out.contains("```yaml\nkey: value\nnumber: 42\n```\n"));
    }

    #[test]
    fn bullets_render_dashes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summary");
        let mut s = StepSummary::to_file(&path);
        s.bullets(["a", "b", "c"]);
        s.commit().unwrap();
        let out = read_to_string(&path);
        assert!(out.contains("- a\n- b\n- c\n"));
    }

    #[test]
    fn commit_appends_to_existing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summary");
        // First step writes a heading
        let mut s1 = StepSummary::to_file(&path);
        s1.heading(2, "first step");
        s1.commit().unwrap();
        // Second step appends another heading
        let mut s2 = StepSummary::to_file(&path);
        s2.heading(2, "second step");
        s2.commit().unwrap();
        let out = read_to_string(&path);
        assert!(out.contains("## first step\n"));
        assert!(out.contains("## second step\n"));
    }

    #[test]
    fn as_str_returns_buffer_without_flushing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("summary");
        let mut s = StepSummary::to_file(&path);
        s.heading(1, "test");
        let buffered = s.as_str().to_string();
        assert!(buffered.contains("# test\n"));
        // File hasn't been touched yet
        assert!(!path.exists());
    }

    #[test]
    fn from_runner_env_errors_when_unset() {
        // SAFETY: required for env var manipulation in 2024 edition
        unsafe { std::env::remove_var("GITHUB_STEP_SUMMARY"); }
        let result = StepSummary::from_runner_env();
        let err = result.unwrap_err();
        assert!(err.is_fatal());
        assert!(err.as_workflow_command().contains("GITHUB_STEP_SUMMARY"));
    }

    #[test]
    fn status_constants_use_typed_strings() {
        assert_eq!(status::PASSED, "✓ Passed");
        assert_eq!(status::FAILED, "✗ Failed");
        assert_eq!(status::PAUSED, "❚❚ Paused");
    }
}
