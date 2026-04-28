//! `ActionError` — the canonical error type for pleme-io action binaries.
//!
//! When emitted via [`emit_to_stdout`], the error renders as a GitHub
//! Actions workflow command (`::error::`, `::warning::`, `::notice::`)
//! that the runner picks up + surfaces in the run UI as an annotated
//! file/line/column reference.
//!
//! Reference:
//! https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions

use std::fmt;
use thiserror::Error;

/// Convenience alias.
pub type ActionResult<T> = Result<T, ActionError>;

/// All errors a pleme-io action binary can surface, organized by
/// workflow-command level. Each variant carries the structured fields
/// the workflow command expects (file, line, col, title).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ActionError {
    /// `::error::` — action fails. CI step exits non-zero.
    #[error("{message}")]
    Error {
        message: String,
        #[source]
        source: Option<ErrorContext>,
    },
    /// `::warning::` — non-fatal. CI step continues but the run shows
    /// a yellow warning annotation.
    #[error("{message}")]
    Warning {
        message: String,
        #[source]
        source: Option<ErrorContext>,
    },
    /// `::notice::` — informational. CI step continues, run shows a
    /// blue info annotation.
    #[error("{message}")]
    Notice {
        message: String,
        #[source]
        source: Option<ErrorContext>,
    },
}

/// Optional context attached to an action error. All fields optional —
/// GitHub renders whatever's provided.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ErrorContext {
    pub title: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub end_line: Option<u32>,
    pub end_col: Option<u32>,
}

impl fmt::Display for ErrorContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(title) = &self.title {
            write!(f, "title={title}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ErrorContext {}

impl ActionError {
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            source: None,
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self::Warning {
            message: message.into(),
            source: None,
        }
    }

    pub fn notice(message: impl Into<String>) -> Self {
        Self::Notice {
            message: message.into(),
            source: None,
        }
    }

    /// Attach context (file/line/col/title) to this error.
    pub fn with_context(self, context: ErrorContext) -> Self {
        match self {
            Self::Error { message, .. } => Self::Error {
                message,
                source: Some(context),
            },
            Self::Warning { message, .. } => Self::Warning {
                message,
                source: Some(context),
            },
            Self::Notice { message, .. } => Self::Notice {
                message,
                source: Some(context),
            },
        }
    }

    /// Render this error as a GitHub Actions workflow command line.
    /// Format: `::<level> <key>=<value>,...::<message>`.
    /// Newlines in the message are encoded as `%0A` per the workflow
    /// command grammar (otherwise the runner would split the annotation).
    pub fn as_workflow_command(&self) -> String {
        let (level, message, source) = match self {
            Self::Error { message, source } => ("error", message, source),
            Self::Warning { message, source } => ("warning", message, source),
            Self::Notice { message, source } => ("notice", message, source),
        };
        let mut out = String::with_capacity(level.len() + message.len() + 16);
        out.push_str("::");
        out.push_str(level);
        if let Some(ctx) = source {
            let parts = context_parts(ctx);
            if !parts.is_empty() {
                out.push(' ');
                out.push_str(&parts.join(","));
            }
        }
        out.push_str("::");
        out.push_str(&encode_message(message));
        out
    }

    /// Print the workflow-command-formatted line to stdout — what the
    /// runner picks up as an annotation.
    pub fn emit_to_stdout(&self) {
        println!("{}", self.as_workflow_command());
    }

    /// Returns true if this error level should make the CI step exit
    /// non-zero (only [`ActionError::Error`] is fatal).
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

fn context_parts(ctx: &ErrorContext) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(title) = &ctx.title {
        parts.push(format!("title={}", encode_property(title)));
    }
    if let Some(file) = &ctx.file {
        parts.push(format!("file={}", encode_property(file)));
    }
    if let Some(line) = ctx.line {
        parts.push(format!("line={line}"));
    }
    if let Some(col) = ctx.col {
        parts.push(format!("col={col}"));
    }
    if let Some(end_line) = ctx.end_line {
        parts.push(format!("endLine={end_line}"));
    }
    if let Some(end_col) = ctx.end_col {
        parts.push(format!("endColumn={end_col}"));
    }
    parts
}

/// Encode a message body for the workflow-command format. Newlines must
/// be escaped or the runner splits the annotation.
fn encode_message(s: &str) -> String {
    s.replace('%', "%25").replace('\n', "%0A").replace('\r', "%0D")
}

/// Encode a property value (title, file). Comma + colon also need
/// escaping since they're delimiters in the property list.
fn encode_property(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
        .replace(':', "%3A")
        .replace(',', "%2C")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_constructor_no_context() {
        let e = ActionError::error("something broke");
        assert_eq!(e.as_workflow_command(), "::error::something broke");
        assert!(e.is_fatal());
    }

    #[test]
    fn warning_constructor_no_context() {
        let e = ActionError::warning("look out");
        assert_eq!(e.as_workflow_command(), "::warning::look out");
        assert!(!e.is_fatal());
    }

    #[test]
    fn notice_constructor_no_context() {
        let e = ActionError::notice("fyi");
        assert_eq!(e.as_workflow_command(), "::notice::fyi");
        assert!(!e.is_fatal());
    }

    #[test]
    fn error_with_full_context() {
        let e = ActionError::error("bad input").with_context(ErrorContext {
            title: Some("Validation failed".into()),
            file: Some("manifests/rio.yaml".into()),
            line: Some(12),
            col: Some(5),
            end_line: Some(12),
            end_col: Some(20),
        });
        let cmd = e.as_workflow_command();
        assert!(cmd.starts_with("::error "));
        assert!(cmd.contains("title=Validation failed"));
        assert!(cmd.contains("file=manifests/rio.yaml"));
        assert!(cmd.contains("line=12"));
        assert!(cmd.contains("col=5"));
        assert!(cmd.contains("endLine=12"));
        assert!(cmd.contains("endColumn=20"));
        assert!(cmd.ends_with("::bad input"));
    }

    #[test]
    fn newline_in_message_is_encoded() {
        let e = ActionError::error("line one\nline two");
        let cmd = e.as_workflow_command();
        assert!(!cmd.contains('\n'), "newline must be encoded as %0A");
        assert!(cmd.contains("line one%0Aline two"));
    }

    #[test]
    fn percent_in_message_is_encoded_first() {
        // Otherwise %0A in user input would be confused with our newline encoding
        let e = ActionError::error("100% sure");
        let cmd = e.as_workflow_command();
        assert!(cmd.contains("100%25 sure"));
    }

    #[test]
    fn comma_in_property_is_encoded() {
        let e = ActionError::error("ok").with_context(ErrorContext {
            title: Some("a, b, c".into()),
            ..Default::default()
        });
        let cmd = e.as_workflow_command();
        assert!(cmd.contains("title=a%2C b%2C c"));
    }

    #[test]
    fn colon_in_property_is_encoded() {
        let e = ActionError::error("ok").with_context(ErrorContext {
            file: Some("C:\\path\\file.yml".into()),
            ..Default::default()
        });
        let cmd = e.as_workflow_command();
        assert!(cmd.contains("file=C%3A\\path\\file.yml"));
    }

    #[test]
    fn fatal_only_for_error_level() {
        assert!(ActionError::error("x").is_fatal());
        assert!(!ActionError::warning("x").is_fatal());
        assert!(!ActionError::notice("x").is_fatal());
    }
}
