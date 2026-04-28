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
    if let Ok(n) = s.parse::<i64>() {
        return serde_json::Value::Number(n.into());
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
