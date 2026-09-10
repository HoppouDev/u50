//! The `hello` example check set — the authoring template: one module
//! declaring a zero-sized plugin struct, its checks in declaration
//! order, and one registry line (in `crate::registry`). The check50
//! README's exists-then-verify chain, expressed natively.

use std::path::PathBuf;

use crate::api::{CheckContext, Failure};
use crate::plugin::{CheckSetPlugin, CheckSpec, RunKind};

/// The `hello` example check set: `exists` then `prints_content`.
pub struct HelloPlugin;

impl CheckSetPlugin for HelloPlugin {
    fn id(&self) -> &'static str {
        "hello"
    }

    fn check_dir(&self) -> PathBuf {
        // The example ships no extra files (the empty path is the
        // documented sentinel for "no check-directory files").
        PathBuf::new()
    }

    fn checks(&self) -> Vec<CheckSpec> {
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

fn exists(ctx: &mut CheckContext) -> Result<(), Failure> {
    ctx.exists(["hello.txt"])
}

fn prints_content(ctx: &mut CheckContext) -> Result<(), Failure> {
    ctx.log("checking that hello.txt contains hello...");
    let content = std::fs::read_to_string(ctx.run_dir.join("hello.txt"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::Cause;

    #[test]
    fn declares_checks_in_declaration_order() {
        let plugin = HelloPlugin;
        let names: Vec<_> = plugin.checks().into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["exists", "prints_content"]);
    }

    #[test]
    fn the_hello_checks_pass_end_to_end() {
        let plugin = HelloPlugin;
        let dir = tempfile::tempdir().expect("work dir");
        std::fs::write(dir.path().join("hello.txt"), "hello world").expect("write");
        let results = crate::runner::run_checks(&plugin.checks(), dir.path(), dir.path(), &[]);
        for result in &results {
            assert_eq!(result.passed, Some(true), "{} failed", result.name);
        }
    }

    #[test]
    fn a_missing_file_fails_exists_and_skips_its_dependent() {
        let plugin = HelloPlugin;
        let dir = tempfile::tempdir().expect("work dir");
        let results = crate::runner::run_checks(&plugin.checks(), dir.path(), dir.path(), &[]);
        assert_eq!(results[0].passed, Some(false));
        match &results[0].cause {
            Some(Cause::Failure { rationale, .. }) => {
                assert!(rationale.contains("not found"), "{rationale}");
            }
            other => panic!("expected a failure cause, got {other:?}"),
        }
        assert_eq!(results[1].passed, None, "dependent must be skipped");
    }

    #[test]
    fn prints_content_dependency_is_exists() {
        let plugin = HelloPlugin;
        let checks = plugin.checks();
        let dependent = checks
            .iter()
            .find(|c| c.name == "prints_content")
            .expect("prints_content is declared");
        assert_eq!(dependent.dependency.as_deref(), Some("exists"));
    }
}
