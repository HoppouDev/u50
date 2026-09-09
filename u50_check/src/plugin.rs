//! The `CheckSetPlugin` trait: one plugin per problem's check set,
//! registered in [`crate::registry`] (the same model as `u50_style`'s
//! language and renderer plugins).

use std::path::PathBuf;
use std::time::Duration;

use crate::api::{CheckContext, Failure};

/// How a single check executes.
#[derive(Clone)]
pub enum RunKind {
    /// A compiled-in native check function (the Rust analog of a
    /// decorated Python check function).
    Native(fn(&mut CheckContext) -> Result<(), Failure>),
    /// A "simple" YAML check pipeline (check50: `_simple.py`'s compiled
    /// `run`/`stdin`/`stdout`/`exit` sequences), interpreted natively.
    Yaml(Vec<YamlStep>),
}

/// One check of a check set.
pub struct CheckSpec {
    /// Unique name (check50: the Python function name); also the
    /// dependency-graph node key and the results entry's `name`.
    pub name: String,
    /// Human-readable description (check50: the docstring); shown to the
    /// student in every output format.
    pub description: String,
    /// The name of the check this one depends on; the dependency's
    /// filesystem state is inherited and it must pass first.
    pub dependency: Option<String>,
    /// Per-check timeout (check50: `@check(timeout=...)`, default 60s).
    pub timeout: Option<Duration>,
    /// When set, the check is *hidden*: its log is suppressed and any
    /// failure is replaced with this generic rationale (check50:
    /// `@check50.hidden(...)`).
    pub hidden_rationale: Option<String>,
    /// How the check executes.
    pub run: RunKind,
}

/// One step of a YAML simple-check pipeline (`run`/`stdin`/`stdout`/
/// `exit`, executed in that fixed order).
#[derive(Clone)]
pub struct YamlStep {
    /// The command to run.
    pub run: String,
    /// Optional stdin line(s) (newlines joined; prompt=False, per
    /// check50's `_simple.py`).
    pub stdin: Option<String>,
    /// Optional stdout assertion (exact match, per `_simple.py`).
    pub stdout: Option<String>,
    /// Optional exit-code assertion (`None` = just wait for exit, per
    /// `_simple.py`'s `.exit()`).
    pub exit: Option<i32>,
}

/// One problem's set of checks, fully self-contained: its id, the
/// directory its own files live in (for `include`), and its checks.
pub trait CheckSetPlugin: Sync {
    /// Stable machine id (`"hello"`), matched against the request slug.
    fn id(&self) -> &str;
    /// The directory containing the check set's own files (source of
    /// `include` in its checks).
    fn check_dir(&self) -> PathBuf;
    /// The checks, in declaration order (the order results are emitted
    /// in, like check50's declaration-order results).
    fn checks(&self) -> Vec<CheckSpec>;
}

/// The `hello` example check set — the authoring template: one module
/// (or section) declaring a zero-sized plugin struct, its checks in
/// declaration order, and one registry line. The check50 README's
/// exists-then-verify chain, expressed natively.
pub struct HelloPlugin;

impl CheckSetPlugin for HelloPlugin {
    fn id(&self) -> &'static str {
        "hello"
    }

    fn check_dir(&self) -> PathBuf {
        // The example ships no extra files.
        PathBuf::new()
    }

    fn checks(&self) -> Vec<CheckSpec> {
        fn exists(ctx: &mut CheckContext) -> Result<(), Failure> {
            ctx.exists(["hello.txt"])
        }

        fn prints_content(ctx: &mut CheckContext) -> Result<(), Failure> {
            ctx.log("checking that hello.txt contains hello...");
            let content = std::fs::read_to_string("hello.txt")
                .map_err(|_| Failure::new("could not read hello.txt"))?;
            if content.contains("hello") {
                Ok(())
            } else {
                Err(Failure::with_help(
                    "expected \"hello\" in hello.txt",
                    "the file should contain the word hello",
                ))
            }
        }

        vec![
            CheckSpec {
                name: "exists".to_owned(),
                description: "hello.txt exists".to_owned(),
                dependency: None,
                timeout: None,
                hidden_rationale: None,
                run: RunKind::Native(exists),
            },
            CheckSpec {
                name: "prints_content".to_owned(),
                description: "hello.txt contains hello".to_owned(),
                dependency: Some("exists".to_owned()),
                timeout: None,
                hidden_rationale: None,
                run: RunKind::Native(prints_content),
            },
        ]
    }
}
