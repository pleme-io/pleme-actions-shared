# pleme-actions-shared

Shared toolkit crate for pleme-io's reusable GitHub Actions surface. Every
published pleme-io action's Rust binary depends on this crate.

**Companion to** the typescape's `Action` domain at
[`pleme-io/arch-synthesizer/src/action_domain/`](https://github.com/pleme-io/arch-synthesizer/tree/main/src/action_domain).
**Canonical design** at
[`pleme-io/theory/CONSTRUCTIVE-ACTIONS.md`](https://github.com/pleme-io/theory/blob/main/CONSTRUCTIVE-ACTIONS.md).
**Implementation plan** at
[`pleme-io/theory/PLEME-ACTIONS-PLAN.md`](https://github.com/pleme-io/theory/blob/main/PLEME-ACTIONS-PLAN.md).

## What it provides

| Module | Concern |
|---|---|
| [`input`](src/input.rs) | Typed input parsing — read `INPUT_<NAME>` env vars from the GitHub Actions runtime into a `serde::Deserialize` struct. Auto-coerces booleans / numbers / JSON. |
| [`output`](src/output.rs) | Append `name=value` (or heredoc multi-line) pairs to `$GITHUB_OUTPUT`. JSON via `set_json`. |
| [`summary`](src/summary.rs) | `StepSummary` builder — typed markdown for `$GITHUB_STEP_SUMMARY`. Heading/paragraph/code-block/table/bullets/status-badge primitives. |
| [`error`](src/error.rs) | `ActionError` enum (`Error` / `Warning` / `Notice`) — renders as GitHub workflow commands (`::error::`, `::warning::`, `::notice::`) with optional file/line/col context. |
| [`log`](src/log.rs) | Structured logging respecting `RUNNER_DEBUG=1`. `debug` emits only when on; `info`/`warn`/`error` always emit. |

## Example action

```rust
use pleme_actions_shared::{Input, Output, StepSummary};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Inputs {
    working_directory: String,
    #[serde(default)]
    auto_approve: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let inputs = Input::<Inputs>::from_env()?;

    // … action logic …

    let output = Output::from_runner_env()?;
    output.set("plan-summary", "+5 -2 ~3")?;

    let mut summary = StepSummary::from_runner_env()?;
    summary
        .heading(1, "Plan summary")
        .table(
            &["Operation", "Count"],
            vec![vec!["add", "5"], vec!["change", "2"], vec!["destroy", "3"]],
        );
    summary.commit()?;

    Ok(())
}
```

## Test discipline

Every module has unit tests covering happy path + every error mode.
Tests use:

- `tempfile::tempdir()` for `$GITHUB_OUTPUT` / `$GITHUB_STEP_SUMMARY`
  destinations (no env-var manipulation in the common path)
- An `EnvSource` trait + `MapEnv` test double so `Input::from_env_with`
  can run deterministic env-free tests

```bash
cargo test
```

## License

MIT.
