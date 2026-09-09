//! Native interpretation of "simple" `.cs50.yaml` check pipelines
//! (check50: `_simple.py` compiles these into Python; the port runs them
//! directly against the check API, with the same fixed step order and
//! option defaults: `stdin` uses `prompt=False` and `stdout` uses
//! exact matching).

use anyhow::Context as _;
use serde::Deserialize;

use crate::api::{CheckContext, Failure, MatchInput, StdinInput};
use crate::plugin::{CheckSpec, RunKind, YamlStep};

/// The parsed `.cs50.yaml` config (the subset check50 uses).
#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub check50: Check50Config,
}

#[derive(Debug, Deserialize, Default)]
pub struct Check50Config {
    /// The checks file (string, like `check.py`), or a dict of simple
    /// checks (compiled to Python by check50, interpreted natively here).
    #[serde(default)]
    pub checks: Option<serde_yaml::Value>,
    /// pip dependencies installed before the run (a documented
    /// divergence: the port does not install check dependencies).
    #[serde(default)]
    #[allow(dead_code)]
    pub dependencies: Option<serde_yaml::Value>,
}

/// Loads and parses the `.cs50.yaml` of a check directory.
///
/// # Errors
/// Returns an error when the file is missing or invalid YAML.
pub fn load_config(check_dir: &std::path::Path) -> anyhow::Result<Config> {
    let path = check_dir.join(".cs50.yaml");
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("invalid {}", path.display()))
}

/// Builds the check set's specs from a `.cs50.yaml`, either by pointing
/// at a native checks file (`checks: check.py` — not executable without
/// Python, so an empty spec set is returned and the caller surfaces the
/// divergence) or by interpreting the simple-check dict natively.
///
/// # Errors
/// Returns an error when the YAML checks are malformed.
pub fn specs_from_config(config: &Config) -> anyhow::Result<Vec<CheckSpec>> {
    let Some(serde_yaml::Value::Mapping(checks)) = config
        .check50
        .checks
        .as_ref()
        .filter(|value| value.is_mapping())
    else {
        // `checks: <file>` names a Python checks file: the port's
        // documented divergence (Python checks are not executable here).
        return Ok(Vec::new());
    };
    let mut specs = Vec::new();
    for (name, value) in checks {
        let name = name
            .as_str()
            .context("check name must be a string")?
            .to_owned();
        let steps = serde_yaml::from_value::<Vec<YamlStepRaw>>(value.clone())
            .with_context(|| format!("invalid steps for check {name}"))?;
        let steps = steps
            .iter()
            .map(|step| YamlStep {
                run: step.run.clone(),
                stdin: step.stdin.clone(),
                stdout: step.stdout.clone(),
                exit: step.exit,
            })
            .collect();
        // The description defaults to the check name (check50 parity).
        specs.push(CheckSpec {
            name: name.clone(),
            description: name,
            dependency: None,
            timeout: None,
            hidden_rationale: None,
            run: RunKind::Yaml(steps),
        });
    }
    Ok(specs)
}

#[derive(Debug, Deserialize)]
struct YamlStepRaw {
    run: String,
    #[serde(default)]
    stdin: Option<String>,
    #[serde(default)]
    stdout: Option<String>,
    #[serde(default)]
    exit: Option<i32>,
}

/// Interprets a YAML simple-check pipeline against the check API (the
/// fixed step order and option defaults of check50's `_simple.py`:
/// `run` → `stdin(prompt=False)` → `stdout(exact)` → `exit`).
///
/// # Errors
/// Returns a [`Failure`] when any step's assertion fails.
pub fn run_steps(ctx: &mut CheckContext, steps: &[YamlStep]) -> Result<(), Failure> {
    for step in steps {
        let mut run = ctx.run(&step.run)?;
        if let Some(stdin) = &step.stdin {
            // check50's _simple.py joins multi-line stdin and uses
            // prompt=False + exact stdout matching.
            run.stdin(
                StdinInput::Line(stdin.clone()),
                false,
                crate::api::DEFAULT_IO_TIMEOUT,
            )?;
        }
        if let Some(stdout) = &step.stdout {
            run.stdout(
                MatchInput::Pattern(stdout.clone()),
                true,
                crate::api::DEFAULT_IO_TIMEOUT,
            )?;
        }
        // `.exit()` with no args just waits for exit; `.exit(N)` asserts.
        run.exit(step.exit, crate::api::DEFAULT_EXIT_TIMEOUT)?;
    }
    Ok(())
}
