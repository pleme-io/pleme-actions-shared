//! Typed input parsing — read GitHub Actions inputs from `INPUT_<NAME>`
//! env vars into a typed Rust value.
//!
//! The composite action.yml (rendered by arch-synthesizer's
//! [`action_domain::render`]) hoists every workflow input to an
//! environment variable named `INPUT_<UPPERCASE_NAME>` (kebab-case
//! names get hyphens replaced with underscores). The binary's
//! `main` reads them via [`Input::from_env`] / [`Input::from_env_with`].

use std::collections::BTreeMap;
use std::env;

use serde::de::DeserializeOwned;

use crate::error::{ActionError, ActionResult, ErrorContext};

/// The typed input surface. Each pleme-io action declares an
/// `Input` struct that derives `serde::Deserialize`; calling
/// [`Input::from_env`] reads `INPUT_<NAME>` env vars into the typed
/// value.
///
/// ## Example
///
/// ```
/// use pleme_actions_shared::Input;
/// use serde::Deserialize;
///
/// #[derive(Debug, Deserialize)]
/// struct MyInputs {
///     working_directory: String,
///     #[serde(default)]
///     auto_approve: bool,
/// }
///
/// // In the action's main:
/// // let inputs = Input::<MyInputs>::from_env()?;
/// ```
pub struct Input<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Input<T>
where
    T: DeserializeOwned,
{
    /// Read all `INPUT_<NAME>` env vars from the process environment
    /// and deserialize into `T`. Field names on `T` are matched by
    /// translating snake_case → SCREAMING_SNAKE_CASE → `INPUT_<NAME>`.
    pub fn from_env() -> ActionResult<T> {
        Self::from_env_with(&EnvVarSource)
    }

    /// Same as [`Self::from_env`] but reads from a custom source — used
    /// in tests via [`MapEnv`].
    pub fn from_env_with(source: &dyn EnvSource) -> ActionResult<T> {
        let map = source.collect_input_vars();
        let json = env_map_to_json(map);
        serde_json::from_value(json).map_err(|e| {
            ActionError::error(format!("failed to parse INPUT_* env vars: {e}")).with_context(
                ErrorContext {
                    title: Some("Action input parse error".into()),
                    ..Default::default()
                },
            )
        })
    }
}

/// Pluggable env-var source so tests can substitute a deterministic map.
pub trait EnvSource {
    /// Return all `INPUT_<NAME>` env vars (or `INPUT_`-prefixed pairs
    /// from a test source) keyed by the un-prefixed lowercase form.
    fn collect_input_vars(&self) -> BTreeMap<String, String>;
}

/// Reads from `std::env`. The default source.
pub struct EnvVarSource;

impl EnvSource for EnvVarSource {
    fn collect_input_vars(&self) -> BTreeMap<String, String> {
        env::vars()
            .filter_map(|(k, v)| {
                k.strip_prefix("INPUT_")
                    .map(|stripped| (stripped.to_ascii_lowercase(), v))
            })
            .collect()
    }
}

/// In-memory env source for tests. Insert raw `INPUT_<NAME>` keys; the
/// trait impl strips the prefix + lowercases the same way the real
/// source does.
#[derive(Debug, Default, Clone)]
pub struct MapEnv {
    pub vars: BTreeMap<String, String>,
}

impl MapEnv {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, name: &str, value: impl Into<String>) -> Self {
        self.vars.insert(format!("INPUT_{name}"), value.into());
        self
    }
}

impl EnvSource for MapEnv {
    fn collect_input_vars(&self) -> BTreeMap<String, String> {
        self.vars
            .iter()
            .filter_map(|(k, v)| {
                k.strip_prefix("INPUT_")
                    .map(|stripped| (stripped.to_ascii_lowercase(), v.clone()))
            })
            .collect()
    }
}

/// Convert a flat `INPUT_<NAME>` map (lowercased keys) to a JSON object
/// serde can deserialize from. String values stay strings; serde's
/// type coercion handles bool/number on the destination type.
fn env_map_to_json(map: BTreeMap<String, String>) -> serde_json::Value {
    let mut obj = serde_json::Map::new();
    for (k, v) in map {
        obj.insert(k, parse_scalar(&v));
    }
    serde_json::Value::Object(obj)
}

/// Parse a scalar string into the most-natural JSON type:
/// "true"/"false" → bool, decimal numbers → number, JSON-shaped
/// → parsed JSON, everything else → string.
///
/// This preserves operator ergonomics — declaring an action input as
/// `r#type: bool` in the catalog and consuming it as `bool` in Rust
/// works without manual coercion.
fn parse_scalar(s: &str) -> serde_json::Value {
    match s {
        "true" => return serde_json::Value::Bool(true),
        "false" => return serde_json::Value::Bool(false),
        _ => {}
    }
    // ── ★ ONLY CANONICAL DECIMALS BECOME NUMBERS ─────────────────────────
    // `"0042".parse::<i64>()` succeeds and yields 42, which is a DIFFERENT
    // IDENTIFIER than the one the operator typed. Round-tripping through
    // Display and requiring the text back tells the two apart: a genuine
    // number is unchanged, a zero-padded or `+`-prefixed one is not, and only
    // the former is safe to retype.
    //
    // This keeps the ergonomics the coercion exists for — a catalog input
    // declared `r#type: int` still arrives as a number — while refusing to
    // renumber an identifier that merely looks numeric. Padded ids are common
    // enough (zero-padded run numbers, some issue keys) that the silent case
    // is worth closing rather than documenting.
    if let Ok(n) = s.parse::<i64>() {
        if n.to_string() == s {
            return serde_json::Value::Number(n.into());
        }
        return serde_json::Value::String(s.to_string());
    }
    if let Ok(f) = s.parse::<f64>() {
        if let Some(n) = serde_json::Number::from_f64(f) {
            return serde_json::Value::Number(n);
        }
    }
    // JSON-shaped (starts with { or [) — try parsing
    let trimmed = s.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
            return v;
        }
    }
    serde_json::Value::String(s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct SimpleInputs {
        working_directory: String,
        action: String,
    }

    #[test]
    fn basic_string_inputs() {
        let env = MapEnv::new()
            .with("WORKING_DIRECTORY", "/tmp/x")
            .with("ACTION", "plan");
        let inputs: SimpleInputs = Input::from_env_with(&env).unwrap();
        assert_eq!(inputs.working_directory, "/tmp/x");
        assert_eq!(inputs.action, "plan");
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct TypedInputs {
        max_runners: u32,
        auto_approve: bool,
        threshold: f64,
    }

    #[test]
    fn typed_coercion_from_strings() {
        let env = MapEnv::new()
            .with("MAX_RUNNERS", "5")
            .with("AUTO_APPROVE", "true")
            .with("THRESHOLD", "0.5");
        let inputs: TypedInputs = Input::from_env_with(&env).unwrap();
        assert_eq!(inputs.max_runners, 5);
        assert!(inputs.auto_approve);
        assert_eq!(inputs.threshold, 0.5);
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct WithDefault {
        required_field: String,
        #[serde(default)]
        optional_field: bool,
    }

    #[test]
    fn missing_optional_uses_serde_default() {
        let env = MapEnv::new().with("REQUIRED_FIELD", "x");
        let inputs: WithDefault = Input::from_env_with(&env).unwrap();
        assert_eq!(inputs.required_field, "x");
        assert!(!inputs.optional_field);
    }

    #[test]
    fn missing_required_field_errors() {
        let env = MapEnv::new();
        let result: ActionResult<SimpleInputs> = Input::from_env_with(&env);
        let err = result.unwrap_err();
        assert!(err.is_fatal());
        let cmd = err.as_workflow_command();
        assert!(cmd.starts_with("::error "));
        assert!(cmd.contains("title=Action input parse error"));
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct WithJson {
        tf_vars: serde_json::Value,
    }

    #[test]
    fn json_object_input_is_parsed() {
        let env = MapEnv::new().with("TF_VARS", r#"{"github_token": "ghp_abc"}"#);
        let inputs: WithJson = Input::from_env_with(&env).unwrap();
        assert_eq!(
            inputs.tf_vars["github_token"].as_str().unwrap(),
            "ghp_abc"
        );
    }

    #[test]
    fn json_array_input_is_parsed() {
        let env = MapEnv::new().with("TF_VARS", r#"["a", "b", "c"]"#);
        let inputs: WithJson = Input::from_env_with(&env).unwrap();
        assert_eq!(inputs.tf_vars.as_array().unwrap().len(), 3);
    }

    #[test]
    fn non_input_prefixed_vars_are_ignored() {
        // Real GitHub Actions runs have GITHUB_*, RUNNER_*, etc. set;
        // those should NEVER leak into our typed input parsing.
        let mut env = MapEnv::new();
        env.vars.insert("GITHUB_ACTOR".into(), "drzzln".into());
        env.vars.insert("RUNNER_OS".into(), "Linux".into());
        env = env.with("WORKING_DIRECTORY", "/tmp/x").with("ACTION", "plan");
        let inputs: SimpleInputs = Input::from_env_with(&env).unwrap();
        assert_eq!(inputs.working_directory, "/tmp/x");
        // GITHUB_ACTOR shouldn't have polluted the deserialization
    }

    /// MapEnv.with() vs raw INPUT_ key — both produce the same shape.
    #[test]
    fn mapenv_with_helper_matches_raw_insertion() {
        let a = MapEnv::new()
            .with("WORKING_DIRECTORY", "x")
            .with("ACTION", "y");
        let mut b = MapEnv::new();
        b.vars.insert("INPUT_WORKING_DIRECTORY".into(), "x".into());
        b.vars.insert("INPUT_ACTION".into(), "y".into());
        let parsed_a: SimpleInputs = Input::from_env_with(&a).unwrap();
        let parsed_b: SimpleInputs = Input::from_env_with(&b).unwrap();
        assert_eq!(parsed_a, parsed_b);
    }

    #[test]
    fn parse_scalar_recognizes_booleans() {
        assert_eq!(parse_scalar("true"), serde_json::Value::Bool(true));
        assert_eq!(parse_scalar("false"), serde_json::Value::Bool(false));
        // Edge cases: capitalized / spaced strings stay strings
        assert!(parse_scalar("True").is_string());
        assert!(parse_scalar("FALSE").is_string());
    }

    #[test]
    fn parse_scalar_recognizes_integers_and_floats() {
        assert!(parse_scalar("42").is_number());
        assert!(parse_scalar("-1").is_number());
        assert!(parse_scalar("3.14").is_number());
        // Non-numeric stays string
        assert!(parse_scalar("v1.2.3").is_string());
    }

    #[test]
    fn parse_scalar_falls_back_to_string_for_invalid_json() {
        // Looks like JSON-start but isn't — fall back to string
        assert_eq!(
            parse_scalar("{invalid"),
            serde_json::Value::String("{invalid".into())
        );
    }
}

/// Deserializers for inputs whose *text* matters even when it looks like
/// something else.
///
/// ── ★ WHY THIS EXISTS ─────────────────────────────────────────────────
/// [`parse_scalar`] deliberately types scalars by shape: `"true"` becomes a
/// bool and `"4683416"` becomes a number, so an action declaring
/// `r#type: bool` in the catalog can consume a `bool` in Rust with no manual
/// coercion. That ergonomics win is real and stays.
///
/// It has one sharp edge, and it is invisible until it fires: a `String`
/// field whose value happens to be all digits no longer deserializes.
/// Measured 2026-08-23 — `github-app-installation-token` failed at runtime
/// with `invalid type: integer 4683416, expected a string` for an App ID,
/// which is a string to GitHub and to this crate's own struct, and a number
/// only because it looked like one.
///
/// The class is "identifier-shaped input": App IDs, installation IDs, run
/// IDs, issue numbers, ports. Every one of them is text that must survive
/// as text, and every one of them will trip the same wire. So the fix is a
/// reusable deserializer rather than a per-action workaround — the second
/// action to need it should find it here, not rediscover it.
pub mod de {
    use serde::{Deserialize, Deserializer};

    /// Accept a JSON string OR a number, yielding a `String` either way.
    ///
    /// # Errors
    /// Returns the underlying deserializer error for any other type — a
    /// bool, array or object here is a genuine mistake, not a shape to
    /// coerce.
    pub fn string_or_number<'de, D>(d: D) -> Result<String, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            S(String),
            I(i64),
            F(f64),
        }
        Ok(match Either::deserialize(d)? {
            Either::S(s) => s,
            Either::I(i) => i.to_string(),
            // A float here means the input had a decimal point. Rendering it
            // back through Display rather than a fixed precision keeps
            // "1.5" as "1.5" instead of "1.50000".
            Either::F(f) => f.to_string(),
        })
    }

    /// The `Option` form, for an input that is genuinely optional.
    ///
    /// # Errors
    /// As [`string_or_number`].
    pub fn opt_string_or_number<'de, D>(d: D) -> Result<Option<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            S(String),
            I(i64),
            F(f64),
        }
        Ok(Option::<Either>::deserialize(d)?.map(|e| match e {
            Either::S(s) => s,
            Either::I(i) => i.to_string(),
            Either::F(f) => f.to_string(),
        }))
    }
}

#[cfg(test)]
mod identifier_shaped_input_tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Ids {
        #[serde(deserialize_with = "de::string_or_number")]
        app_id: String,
        #[serde(default, deserialize_with = "de::opt_string_or_number")]
        run_id: Option<String>,
    }

    #[test]
    fn an_all_digits_input_survives_as_text() {
        // ★ THE REGRESSION. Without the deserializer this is
        // "invalid type: integer 4683416, expected a string" at RUNTIME —
        // after the action has already been built, published and consumed.
        let env = MapEnv::new().with("APP_ID", "4683416");
        let got: Ids = Input::from_env_with(&env).expect("digits must parse as text");
        assert_eq!(got.app_id, "4683416");
        assert_eq!(got.run_id, None);
    }

    #[test]
    fn a_normal_string_is_unaffected() {
        let env = MapEnv::new().with("APP_ID", "app-4683416");
        let got: Ids = Input::from_env_with(&env).expect("plain text still parses");
        assert_eq!(got.app_id, "app-4683416");
    }

    #[test]
    fn a_leading_zero_identifier_is_not_silently_renumbered() {
        // "0042" parses as i64 42 and would come back as "42" — a DIFFERENT
        // identifier. parse_scalar leaves it a string precisely because it is
        // not canonical decimal, and this pins that so a future "smarter"
        // number parser cannot quietly break padded ids.
        let env = MapEnv::new().with("APP_ID", "0042");
        let got: Ids = Input::from_env_with(&env).expect("padded id parses");
        assert_eq!(got.app_id, "0042", "a padded identifier must not be renumbered");
    }

    #[test]
    fn an_optional_identifier_round_trips_both_shapes() {
        let env = MapEnv::new()
            .with("APP_ID", "1")
            .with("RUN_ID", "99887766");
        let got: Ids = Input::from_env_with(&env).expect("both present");
        assert_eq!(got.run_id.as_deref(), Some("99887766"));
    }
}
