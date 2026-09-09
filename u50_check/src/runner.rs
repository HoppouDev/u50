//! Check execution: per-check isolation, filesystem inheritance,
//! timeout enforcement, concurrency, and the failure skip cascade
//! (check50 parity: `runner.py`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

/// Shared mutable state for the running checks.
struct Shared {
    results: HashMap<String, CheckResult>,
    run_dirs: HashMap<String, PathBuf>,
    finalized: HashMap<String, bool>,
}

/// Runs `checks` (one plugin's check set, in declaration order) against
/// the student files in `work_dir`, with the check set's own files in
/// `check_dir`. `targets` restricts which checks run (plus their
/// dependency chains); empty means all.
///
/// Returns the results in declaration order, mirroring check50:
///
/// (Long by necessity: the dispatch loop, timeout enforcement, and skip
/// cascade form one interleaved scheduling pass. One function keeps the
/// state machine readable.)
#[allow(clippy::too_many_lines)]
/// passing checks dispatch their dependents, failures cascade as skips
/// (`"can't check until a frown turns upside down"`), and per-check
/// timeouts fail with `"check timed out after N seconds"`.
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
    let should_run = |name: &str| -> bool {
        match &scheduled {
            None => true,
            Some(subgraph) => subgraph
                .values()
                .any(|children| children.iter().any(|child| child == name)),
        }
    };

    let temp = tempfile::Builder::new()
        .prefix("u50-check-")
        .tempdir()
        .expect("could not create the check working area");
    let run_root = temp.path().to_path_buf();
    let seed_dir = run_root.join("-");
    crate::api::copy_tree(work_dir, &seed_dir)
        .unwrap_or_else(|e| tracing::warn!(error = %e, "could not seed the working area"));

    let log_arcs: HashMap<String, Arc<Mutex<Vec<String>>>> = graph
        .order
        .iter()
        .map(|name| (name.clone(), Arc::new(Mutex::new(Vec::new()))))
        .collect();
    let data_arcs: HashMap<String, Arc<Mutex<serde_json::Map<String, serde_json::Value>>>> = graph
        .order
        .iter()
        .map(|name| (name.clone(), Arc::new(Mutex::new(serde_json::Map::new()))))
        .collect();

    let (sender, receiver) = mpsc::channel::<Message>();
    let shared = Arc::new(Mutex::new(Shared {
        results: HashMap::new(),
        run_dirs: HashMap::new(),
        finalized: HashMap::new(),
    }));

    let total = graph.order.iter().filter(|name| should_run(name)).count();
    let mut deadlines: HashMap<String, Instant> = HashMap::new();

    if let Some(children) = scheduled.as_ref().and_then(|s| s.get(&None)) {
        for name in children {
            dispatch_check(
                name,
                None,
                checks,
                &graph,
                &log_arcs,
                &data_arcs,
                check_dir,
                &run_root,
                &seed_dir,
                &shared,
                sender.clone(),
            );
        }
    } else if let Some(children) = graph.dependents.get(&None) {
        for name in children {
            if should_run(name) {
                dispatch_check(
                    name,
                    None,
                    checks,
                    &graph,
                    &log_arcs,
                    &data_arcs,
                    check_dir,
                    &run_root,
                    &seed_dir,
                    &shared,
                    sender.clone(),
                );
            }
        }
    }

    loop {
        let done = shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .finalized
            .len();
        if done >= total {
            break;
        }
        let next_deadline = deadlines.values().copied().min();
        let wait = next_deadline.map_or(Duration::from_millis(50), |d| {
            d.saturating_duration_since(Instant::now())
                .min(Duration::from_millis(50))
        });
        match receiver.recv_timeout(wait) {
            Ok(Message::Done(name, result)) => {
                let mut guard = shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if guard.finalized.contains_key(&name) {
                    continue;
                }
                guard.finalized.insert(name.clone(), true);
                let passed = result.passed == Some(true);
                guard.results.insert(name.clone(), result);
                drop(guard);
                deadlines.remove(&name);
                if passed
                    && let Some(children) = scheduled
                        .as_ref()
                        .and_then(|s| s.get(&Some(name.clone())))
                        .or_else(|| graph.dependents.get(&Some(name.clone())))
                {
                    for child in children {
                        if !should_run(child) {
                            continue;
                        }
                        let dep_dir = shared
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .run_dirs
                            .get(&name)
                            .cloned();
                        dispatch_check(
                            child,
                            dep_dir,
                            checks,
                            &graph,
                            &log_arcs,
                            &data_arcs,
                            check_dir,
                            &run_root,
                            &seed_dir,
                            &shared,
                            sender.clone(),
                        );
                        let timeout = graph.timeouts[child.as_str()];
                        deadlines.insert(child.clone(), Instant::now() + timeout);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                let expired: Vec<String> = deadlines
                    .iter()
                    .filter(|(_, deadline)| **deadline <= now)
                    .map(|(name, _)| name.clone())
                    .collect();
                for name in expired {
                    let mut guard = shared
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if guard.finalized.contains_key(&name) {
                        deadlines.remove(&name);
                        continue;
                    }
                    guard.finalized.insert(name.clone(), true);
                    let timeout = graph.timeouts[&name].as_secs();
                    guard.results.insert(
                        name.clone(),
                        CheckResult {
                            name: name.clone(),
                            description: graph.descriptions[&name].clone(),
                            passed: Some(false),
                            log: Vec::new(),
                            cause: Some(Cause::failure(format!(
                                "check timed out after {timeout} seconds"
                            ))),
                            data: serde_json::Map::new(),
                            dependency: graph.dependency_of[&name].clone(),
                        },
                    );
                    deadlines.remove(&name);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let guard = shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut results: HashMap<String, CheckResult> = guard.results.clone();
    drop(guard);
    for name in &graph.order {
        if !should_run(name) {
            continue;
        }
        let Some(result) = results.get(name) else {
            results.insert(
                name.clone(),
                CheckResult {
                    name: name.clone(),
                    description: graph.descriptions[name].clone(),
                    passed: None,
                    log: Vec::new(),
                    cause: Some(Cause::skipped()),
                    data: serde_json::Map::new(),
                    dependency: graph.dependency_of[name].clone(),
                },
            );
            continue;
        };
        if result.passed != Some(true) {
            let mut stack = vec![name.clone()];
            while let Some(current) = stack.pop() {
                for dependent in graph
                    .dependents
                    .get(&Some(current.clone()))
                    .into_iter()
                    .flatten()
                {
                    if should_run(dependent)
                        && results
                            .get(dependent)
                            .is_none_or(|r| r.passed.is_none() && r.cause.is_none())
                    {
                        results.insert(
                            dependent.clone(),
                            CheckResult {
                                name: dependent.clone(),
                                description: graph.descriptions[dependent].clone(),
                                passed: None,
                                log: Vec::new(),
                                cause: Some(Cause::skipped()),
                                data: serde_json::Map::new(),
                                dependency: graph.dependency_of[dependent].clone(),
                            },
                        );
                        stack.push(dependent.clone());
                    }
                }
            }
        }
    }

    graph
        .order
        .iter()
        .filter(|name| should_run(name))
        .map(|name| {
            let mut result = results.remove(name).unwrap_or_else(|| CheckResult {
                name: name.clone(),
                description: graph.descriptions[name].clone(),
                passed: None,
                log: Vec::new(),
                cause: Some(Cause::skipped()),
                data: serde_json::Map::new(),
                dependency: graph.dependency_of[name].clone(),
            });
            let log = log_arcs[name]
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
                &data_arcs[name]
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            result.dependency.clone_from(&graph.dependency_of[name]);
            result.description.clone_from(&graph.descriptions[name]);
            result
        })
        .collect()
}

/// Spawns the check thread for `name`. The thread creates the check's
/// run dir (inheriting the dependency's filesystem), runs the check, and
/// sends the result back through the channel.
#[allow(clippy::too_many_arguments)]
fn dispatch_check(
    name: &str,
    dependency_run_dir: Option<PathBuf>,
    checks: &[CheckSpec],
    graph: &Graph,
    log_arcs: &HashMap<String, Arc<Mutex<Vec<String>>>>,
    data_arcs: &HashMap<String, Arc<Mutex<serde_json::Map<String, serde_json::Value>>>>,
    check_dir: &Path,
    run_root: &Path,
    seed_dir: &Path,
    shared: &Arc<Mutex<Shared>>,
    sender: mpsc::Sender<Message>,
) {
    let index = graph.specs[name];
    let spec = &checks[index];
    let run_dir = run_root.join(name);
    let source = dependency_run_dir.unwrap_or_else(|| seed_dir.to_path_buf());
    if let Err(e) = crate::api::copy_tree(&source, &run_dir) {
        tracing::warn!(error = %e, check = name, "could not set up the run dir");
    }
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .run_dirs
        .insert(name.to_owned(), run_dir.clone());

    let ctx = CheckContext::new(
        run_dir,
        check_dir.to_path_buf(),
        Arc::clone(&log_arcs[name]),
        Arc::clone(&data_arcs[name]),
    );
    let hidden = graph.hidden.get(name).cloned();
    let kind = spec.run.clone();
    let _sender2 = sender.clone();
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
                if let Some(rationale) = hidden {
                    result.cause = Some(Cause::failure(rationale));
                } else {
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
