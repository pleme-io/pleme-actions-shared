//! Shared toolkit for pleme-io GitHub Actions.
//!
//! Every published pleme-io action's Rust binary depends on this crate.
//! It provides the typed bridge between GitHub Actions' runtime
//! conventions (`INPUT_<NAME>` env vars, `$GITHUB_OUTPUT` file,
//! `$GITHUB_STEP_SUMMARY` file, `::error::`/`::warning::`/`::notice::`
//! workflow commands) and ergonomic Rust types.
//!
//! Companion to the typescape's `Action` domain at
//! `arch-synthesizer/src/action_domain/`. The composite `action.yml`
//! rendered for each action hoists every input to `env:` (per
//! Semgrep's `yaml.github-actions.security.run-shell-injection` rule);
//! the binary then reads them via [`Input::from_env`].
//!
//! ## Design discipline
//!
//! - **No magic.** Every conversion (env var → typed value) is explicit
//!   and tested. Failing inputs surface clear errors at the boundary,
//!   never panic.
//! - **No global state.** Every helper takes the IO surface as a
//!   parameter (file path, writer) so tests can substitute a tempfile
//!   without env-var races.
//! - **Pillar 12 compliance.** Output rendering uses serde / typed
//!   builders; no `format!()` for structural content.

pub mod error;
pub mod input;
pub mod log;
pub mod output;
pub mod summary;

pub use error::{ActionError, ActionResult};
pub use input::Input;
pub use output::Output;
pub use summary::StepSummary;
