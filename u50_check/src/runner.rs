//! Check execution: per-check isolation, filesystem inheritance,
//! timeout enforcement, concurrency, and the failure skip cascade
//! (check50 parity: `runner.py`).
//!
//! The scheduler dispatches dependency-free checks first, dispatches
//! dependents as dependencies pass, and immediately finalizes skipped
//! checks (with their transitive dependents) when a dependency fails —
//! preventing the scheduler from hanging.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;

use crate::api::CheckContext;
use crate::graph::Graph;
use crate::plugin::{CheckSpec, RunKind};
use crate::result::{Cause, CheckResult, ErrorInfo};

/// check50 truncates logs to `max_log_lines` (100) with a `...` head.
const MAX_LOG_LINES: usize = 100;

/// The completion message from a check thread.
enum Message {
    Done(String, CheckResult),
}

/// Per-check runtime state.
struct CheckState {
    result: Option<CheckResult>,
    run_dir: Option<PathBuf>,
}

/// The scheduler: owns all per-check state and drives execution.
struct Scheduler<'a> {
    graph: Graph,
    checks: &'a [CheckSpec],
    check_dir: PathBuf,
    run_root: PathBuf,
    seed_dir: PathBuf,
    log_arcs: HashMap<String, Arc<Mutex<Vec<String>>>>,
    data_arcs: HashMap<String, Arc<Mutex<serde_json::Map<String, serde_json::Value>>>>,
    state: Arc<Mutex<HashMap<String, CheckState>>>,
    sender: mpsc::Sender<Message>,
    receiver: mpsc::Receiver<Message>,
    /// Deadlines for in-flight checks (enforced by the receive loop).
    deadlines: HashMap<String, Instant>,
    /// Number of checks that have been finalized (passed, failed, or
    /// skipped).
    finalized: usize,
    /// Total checks that will run (after target filtering).
    total: usize,
    /// The target subgraph (None = run all checks).
    scheduled: Option<HashMap<Option<String>, Vec<String>>>,
}

impl<'a> Scheduler<'a> {
    /// Creates a scheduler with a temp working area seeded from
    /// `work_dir`.
    ///
    /// # Errors
    /// Returns an error when the temp dir or seed copy fails.
    pub fn new(
        checks: &'a [CheckSpec],
        graph: Graph,
        check_dir: &Path,
        work_dir: &Path,
        scheduled: Option<HashMap<Option<String>, Vec<String>>>,
    ) -> anyhow::Result<Self> {
        let temp = tempfile::Builder::new()
            .prefix("u50-check-")
            .tempdir()
            .context("could not create the check working area")?;
        let run_root = temp.path().to_path_buf();
        let seed_dir = run_root.join("-");
        crate::api::copy_tree(work_dir, &seed_dir)
            .unwrap_or_else(|e| tracing::warn!(error = %e, "could not seed the working area"));
        // Forget the TempDir so the run dirs outlive this function; the
        // caller cleans up via `cleanup`.
        let _ = temp.keep();

        let log_arcs: HashMap<String, Arc<Mutex<Vec<String>>>> = graph
            .order
            .iter()
            .map(|name| (name.clone(), Arc::new(Mutex::new(Vec::new()))))
            .collect();
        let data_arcs: HashMap<String, Arc<Mutex<serde_json::Map<String, serde_json::Value>>>> =
            graph
                .order
                .iter()
                .map(|name| (name.clone(), Arc::new(Mutex::new(serde_json::Map::new()))))
                .collect();

        let total = graph.order.len();
        let (sender, receiver) = mpsc::channel();

        Ok(Self {
            graph,
            checks,
            check_dir: check_dir.to_path_buf(),
            run_root,
            seed_dir,
            log_arcs,
            data_arcs,
            state: Arc::new(Mutex::new(HashMap::new())),
            sender,
            receiver,
            deadlines: HashMap::new(),
            finalized: 0,
            total,
            scheduled,
        })
    }

    /// Dispatches a check thread and records its deadline. Called for
    /// both root checks (no dependency) and dependents of passing
    /// checks.
    fn dispatch(&mut self, name: &str, dep_run_dir: Option<&Path>) {
        let index = self.graph.specs[name];
        let spec = &self.checks[index];
        let run_dir = self.run_root.join(name);
        let source = dep_run_dir.unwrap_or(&self.seed_dir).to_path_buf();
        if let Err(e) = crate::api::copy_tree(&source, &run_dir) {
            tracing::warn!(error = %e, check = name, "could not set up the run dir");
        }

        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                name.to_owned(),
                CheckState {
                    result: None,
                    run_dir: Some(run_dir.clone()),
                },
            );

        // Record the deadline for the timeout enforcement loop.
        self.deadlines
            .insert(name.to_owned(), Instant::now() + self.graph.timeouts[name]);

        let ctx = CheckContext::new(
            run_dir,
            self.check_dir.clone(),
            Arc::clone(&self.log_arcs[name]),
            Arc::clone(&self.data_arcs[name]),
        );
        let _hidden = self.graph.hidden.get(name).cloned();
        let kind = spec.run.clone();
        let sender = self.sender.clone();
        let name_owned = name.to_owned();

        std::thread::spawn(move || {
            let started = Instant::now();
            let mut ctx = ctx;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match &kind {
                RunKind::Native(run) => run(&mut ctx),
                RunKind::Yaml(steps) => crate::yaml::run_steps(&mut ctx, steps),
            }));
            let elapsed = started.elapsed();
            let mut result = CheckResult {
                name: name_owned.clone(),
                description: String::new(),
                passed: None,
                log: Vec::new(),
                cause: None,
                data: serde_json::Map::new(),
                dependency: None,
            };
            match outcome {
                Ok(Ok(())) => result.passed = Some(true),
                Ok(Err(failure)) => {
                    result.passed = Some(false);
                    result.cause = Some(match failure.expected {
                        Some(expected) => Cause::Mismatch {
                            rationale: failure.rationale,
                            help: failure.help,
                            expected,
                            actual: failure.actual.unwrap_or_default(),
                        },
                        None => Cause::Failure {
                            rationale: failure.rationale,
                            help: failure.help,
                        },
                    });
                }
                Err(panic) => {
                    let message = panic
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
                        .unwrap_or_else(|| "unknown panic".to_owned());
                    result.cause = Some(Cause::Error {
                        rationale: "check50 ran into an error while running checks!".to_owned(),
                        error: ErrorInfo {
                            kind: "Panic".to_owned(),
                            value: message,
                            traceback: vec![format!(
                                "check panicked after {:.1}s",
                                elapsed.as_secs_f32()
                            )],
                            data: serde_json::Map::new(),
                        },
                    });
                }
            }
            let _ = sender.send(Message::Done(name_owned, result));
        });
    }

    /// Marks `name` (and transitively its dependents) as skipped when a
    /// dependency did not pass. Called inside the receive loop so the
    /// scheduler never hangs waiting for checks that will never be
    /// dispatched.
    fn cascade_skip(&mut self, name: &str) {
        // Start from `name`'s dependents, not `name` itself: the failed
        // check already has a result recorded by the caller, so seeding
        // the stack with `name` would hit the `contains_key` guard on
        // the very first pop and stop before ever reaching a dependent
        // (leaving the scheduler waiting on results that will never
        // arrive).
        let mut stack: Vec<String> = self
            .graph
            .dependents
            .get(&Some(name.to_owned()))
            .cloned()
            .unwrap_or_default();
        while let Some(current) = stack.pop() {
            if self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(&current)
            {
                continue;
            }
            self.finalized += 1;
            self.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(
                    current.clone(),
                    CheckState {
                        result: Some(CheckResult {
                            name: current.clone(),
                            description: self.graph.descriptions[&current].clone(),
                            passed: None,
                            log: Vec::new(),
                            cause: Some(Cause::skipped()),
                            data: serde_json::Map::new(),
                            dependency: self.graph.dependency_of[&current].clone(),
                        }),
                        run_dir: None,
                    },
                );
            if let Some(dependents) = self.graph.dependents.get(&Some(current.clone())) {
                stack.extend(dependents.iter().cloned());
            }
        }
    }

    /// Enforces per-check timeouts: marks expired checks as timed out
    /// and cascades skips over their dependents.
    fn enforce_timeouts(&mut self) {
        let now = Instant::now();
        let expired: Vec<String> = self
            .deadlines
            .iter()
            .filter(|(_, deadline)| **deadline <= now)
            .map(|(name, _)| name.clone())
            .collect();
        for name in expired {
            self.deadlines.remove(&name);
            self.finalized += 1;
            let timeout = self.graph.timeouts[&name].as_secs();
            self.state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(
                    name.clone(),
                    CheckState {
                        result: Some(CheckResult {
                            name: name.clone(),
                            description: self.graph.descriptions[&name].clone(),
                            passed: Some(false),
                            log: Vec::new(),
                            cause: Some(Cause::failure(format!(
                                "check timed out after {timeout} seconds"
                            ))),
                            data: serde_json::Map::new(),
                            dependency: self.graph.dependency_of[&name].clone(),
                        }),
                        run_dir: None,
                    },
                );
            self.cascade_skip(&name);
        }
    }

    /// Runs the event loop until all checks are finalized.
    pub fn run_loop(&mut self) {
        // Dispatch the root checks (respecting the target subgraph).
        let roots = match &self.scheduled {
            Some(subgraph) => subgraph.get(&None).cloned().unwrap_or_default(),
            None => self
                .graph
                .dependents
                .get(&None)
                .cloned()
                .unwrap_or_default(),
        };
        for name in &roots {
            self.dispatch(name, None);
        }

        loop {
            if self.finalized >= self.total {
                break;
            }
            // Enforce timeouts, then wait for a completion.
            self.enforce_timeouts();
            if self.finalized >= self.total {
                break;
            }
            let next_deadline = self.deadlines.values().copied().min();
            let wait = next_deadline.map_or(Duration::from_millis(50), |d| {
                d.saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50))
            });
            match self.receiver.recv_timeout(wait) {
                Ok(Message::Done(name, result)) => {
                    if self
                        .state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .get(name.as_str())
                        .is_some_and(|s| s.result.is_some())
                    {
                        continue; // late result for a timed-out check
                    }
                    self.finalized += 1;
                    self.deadlines.remove(&name);
                    let passed = result.passed == Some(true);
                    self.state
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .get_mut(name.as_str())
                        .expect("state was just checked")
                        .result = Some(result);
                    if passed {
                        // Dispatch dependents, inheriting the run dir.
                        let dep_dir = self
                            .state
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .get(name.as_str())
                            .and_then(|s| s.run_dir.clone());
                        let dependents = match &self.scheduled {
                            Some(subgraph) => subgraph
                                .get(&Some(name.clone()))
                                .cloned()
                                .unwrap_or_default(),
                            None => self
                                .graph
                                .dependents
                                .get(&Some(name.clone()))
                                .cloned()
                                .unwrap_or_default(),
                        };
                        for dependent in &dependents {
                            self.dispatch(dependent, dep_dir.as_deref());
                        }
                    } else {
                        // Failure: immediately cascade skips over all
                        // transitive dependents (prevents hangs).
                        self.cascade_skip(&name);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Handled by enforce_timeouts at the top of the loop.
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    /// Collects the results in declaration order and cleans up.
    #[must_use]
    pub fn collect(self) -> Vec<CheckResult> {
        let state = std::mem::take(
            &mut *self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        // Clean up the working area.
        let _ = std::fs::remove_dir_all(&self.run_root);
        self.graph
            .order
            .iter()
            .map(|name| {
                let entry = state.get(name);
                let mut result =
                    entry
                        .and_then(|s| s.result.clone())
                        .unwrap_or_else(|| CheckResult {
                            name: name.clone(),
                            description: self.graph.descriptions[name].clone(),
                            passed: None,
                            log: Vec::new(),
                            cause: Some(Cause::skipped()),
                            data: serde_json::Map::new(),
                            dependency: self.graph.dependency_of[name].clone(),
                        });
                let log = self.log_arcs[name]
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                result.log = if log.len() > MAX_LOG_LINES {
                    let mut lines = vec!["...".to_owned()];
                    lines.extend(log[log.len() - MAX_LOG_LINES..].iter().cloned());
                    lines
                } else {
                    log.clone()
                };
                drop(log);
                result.data.clone_from(
                    &self.data_arcs[name]
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner),
                );
                result
                    .dependency
                    .clone_from(&self.graph.dependency_of[name]);
                result
                    .description
                    .clone_from(&self.graph.descriptions[name]);
                result
            })
            .collect()
    }
}

/// Runs `checks` (one plugin's check set, in declaration order) against
/// the student files in `work_dir`, with the check set's own files in
/// `check_dir`. `targets` restricts which checks run (plus their
/// dependency chains); empty means all.
///
/// Returns the results in declaration order, mirroring check50.
#[must_use]
pub fn run_checks(
    checks: &[CheckSpec],
    check_dir: &Path,
    work_dir: &Path,
    targets: &[String],
) -> Vec<CheckResult> {
    let graph = Graph::new(checks);
    let scheduled: Option<HashMap<Option<String>, Vec<String>>> = if targets.is_empty() {
        None
    } else {
        graph.subgraph(targets)
    };
    if !targets.is_empty() && scheduled.is_none() {
        return graph
            .order
            .iter()
            .map(|name| CheckResult {
                name: name.clone(),
                description: graph.descriptions[name].clone(),
                passed: None,
                log: Vec::new(),
                cause: Some(Cause::skipped()),
                data: serde_json::Map::new(),
                dependency: graph.dependency_of[name].clone(),
            })
            .collect();
    }

    let mut sched = match Scheduler::new(checks, graph, check_dir, work_dir, scheduled) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "could not create the check working area");
            return checks
                .iter()
                .map(|spec| CheckResult {
                    name: spec.name.clone(),
                    description: spec.description.clone(),
                    passed: None,
                    log: Vec::new(),
                    cause: Some(Cause::failure(format!(
                        "could not create the working area: {e}"
                    ))),
                    data: serde_json::Map::new(),
                    dependency: spec.dependency.clone(),
                })
                .collect();
        }
    };

    sched.run_loop();
    sched.collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::RunKind;

    fn native(f: fn(&mut CheckContext) -> Result<(), crate::api::Failure>) -> RunKind {
        RunKind::Native(f)
    }

    #[test]
    fn dependents_inherit_the_dependency_run_dir_and_results_are_declaration_ordered() {
        fn seed(ctx: &mut CheckContext) -> Result<(), crate::api::Failure> {
            std::fs::write(ctx.run_dir.join("artifact.txt"), "hello")
                .map_err(|_| crate::api::Failure::new("could not write artifact"))?;
            Ok(())
        }
        fn reads(ctx: &mut CheckContext) -> Result<(), crate::api::Failure> {
            let content = std::fs::read_to_string(ctx.run_dir.join("artifact.txt"))
                .map_err(|_| crate::api::Failure::new("artifact.txt missing (no inheritance)"))?;
            if content == "hello" {
                Ok(())
            } else {
                Err(crate::api::Failure::new("unexpected artifact content"))
            }
        }

        let checks = vec![
            CheckSpec {
                name: "seed".to_owned(),
                description: "seed".to_owned(),
                dependency: None,
                timeout: None,
                hidden_rationale: None,
                run: native(seed),
            },
            CheckSpec {
                name: "reads".to_owned(),
                description: "reads".to_owned(),
                dependency: Some("seed".to_owned()),
                timeout: None,
                hidden_rationale: None,
                run: native(reads),
            },
        ];
        let check_dir = tempfile::tempdir().expect("check dir");
        let work_dir = tempfile::tempdir().expect("work dir");
        let results = run_checks(&checks, check_dir.path(), work_dir.path(), &[]);
        assert_eq!(
            results.iter().map(|r| r.name.clone()).collect::<Vec<_>>(),
            vec!["seed", "reads"],
            "results must be in declaration order"
        );
        assert_eq!(results[0].passed, Some(true));
        assert_eq!(
            results[1].passed,
            Some(true),
            "reads must see seed's run_dir contents: {:?}",
            results[1].cause
        );
    }

    #[test]
    fn a_failed_dependency_cascades_skip_to_every_transitive_dependent() {
        fn fails(ctx: &mut CheckContext) -> Result<(), crate::api::Failure> {
            ctx.log("about to fail");
            Err(crate::api::Failure::new("deliberate failure"))
        }
        // The `Result` return is fixed by `RunKind::Native`'s function
        // pointer type; this check happens to never fail.
        #[allow(clippy::unnecessary_wraps)]
        fn never_runs(_ctx: &mut CheckContext) -> Result<(), crate::api::Failure> {
            Ok(())
        }

        let checks = vec![
            CheckSpec {
                name: "exists".to_owned(),
                description: "exists".to_owned(),
                dependency: None,
                timeout: None,
                hidden_rationale: None,
                run: native(fails),
            },
            CheckSpec {
                name: "compiles".to_owned(),
                description: "compiles".to_owned(),
                dependency: Some("exists".to_owned()),
                timeout: None,
                hidden_rationale: None,
                run: native(never_runs),
            },
            CheckSpec {
                name: "runs".to_owned(),
                description: "runs".to_owned(),
                dependency: Some("compiles".to_owned()),
                timeout: None,
                hidden_rationale: None,
                run: native(never_runs),
            },
        ];
        let check_dir = tempfile::tempdir().expect("check dir");
        let work_dir = tempfile::tempdir().expect("work dir");
        let results = run_checks(&checks, check_dir.path(), work_dir.path(), &[]);
        assert_eq!(results[0].passed, Some(false));
        for skipped in &results[1..] {
            assert_eq!(skipped.passed, None, "{} should be skipped", skipped.name);
            match &skipped.cause {
                Some(Cause::Skipped { rationale }) => {
                    assert_eq!(rationale, "can't check until a frown turns upside down");
                }
                other => panic!("expected a skip cause for {}, got {other:?}", skipped.name),
            }
        }
    }

    #[test]
    fn a_check_that_exceeds_its_timeout_is_reported_as_failed() {
        // The `Result` return is fixed by `RunKind::Native`'s function
        // pointer type; this check happens to never fail.
        #[allow(clippy::unnecessary_wraps)]
        fn sleeps(_ctx: &mut CheckContext) -> Result<(), crate::api::Failure> {
            std::thread::sleep(Duration::from_millis(300));
            Ok(())
        }

        let checks = vec![CheckSpec {
            name: "slow".to_owned(),
            description: "slow".to_owned(),
            dependency: None,
            timeout: Some(Duration::from_millis(50)),
            hidden_rationale: None,
            run: native(sleeps),
        }];
        let check_dir = tempfile::tempdir().expect("check dir");
        let work_dir = tempfile::tempdir().expect("work dir");
        let results = run_checks(&checks, check_dir.path(), work_dir.path(), &[]);
        assert_eq!(results.len(), 1);
        assert_ne!(
            results[0].passed,
            Some(true),
            "a check that overruns its timeout must not pass"
        );
    }
}
