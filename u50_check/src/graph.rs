//! The check dependency graph: declaration order, scheduling order, and
//! the failure skip cascade (check50 parity).

use std::collections::HashMap;

use crate::plugin::CheckSpec;

/// The dependency graph of a check set: every check name (plus the
/// `None` root) mapped to the names of checks that depend on it.
pub(crate) struct Graph {
    /// Declaration order of all check names.
    pub order: Vec<String>,
    /// `dependency (None-rooted) -> dependents`.
    pub dependents: HashMap<Option<String>, Vec<String>>,
    /// `check name -> its dependency`, inverse of `dependents`.
    pub dependency_of: HashMap<String, Option<String>>,
    /// `check name -> description`.
    pub descriptions: HashMap<String, String>,
    /// `check name -> per-check timeout` (defaults applied).
    pub timeouts: HashMap<String, std::time::Duration>,
    /// `check name -> hidden rationale`, when the check is hidden.
    pub hidden: HashMap<String, String>,
    /// `check name -> execution kind` index.
    pub specs: HashMap<String, usize>,
}

/// The default per-check timeout (check50: 60 seconds).
pub(crate) const DEFAULT_CHECK_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(1);

impl Graph {
    /// Builds the graph from the plugin's checks (in declaration order).
    /// Panics on duplicate or path-unsafe check names, and on any check
    /// that is unreachable from the implicit root (dangling dependency
    /// or dependency cycle) — all plugin-authoring bugs, caught by the
    /// registry tests.
    #[must_use]
    pub fn new(checks: &[CheckSpec]) -> Self {
        let mut seen = std::collections::HashSet::new();
        let mut order = Vec::new();
        let mut dependents: HashMap<Option<String>, Vec<String>> = HashMap::new();
        let mut dependency_of = HashMap::new();
        let mut descriptions = HashMap::new();
        let mut timeouts = HashMap::new();
        let mut hidden = HashMap::new();
        let mut specs = HashMap::new();
        for (index, check) in checks.iter().enumerate() {
            assert!(
                seen.insert(check.name.clone()),
                "duplicate check name `{}` in the check set",
                check.name
            );
            // Check names are used as run-directory path components.
            assert!(
                !check.name.is_empty()
                    && check.name != "."
                    && check.name != ".."
                    && !check.name.contains(['/', '\\']),
                "check name `{}` is not a valid path component",
                check.name
            );
            order.push(check.name.clone());
            dependents
                .entry(check.dependency.clone())
                .or_default()
                .push(check.name.clone());
            dependency_of.insert(check.name.clone(), check.dependency.clone());
            descriptions.insert(check.name.clone(), check.description.clone());
            timeouts.insert(
                check.name.clone(),
                check.timeout.unwrap_or(DEFAULT_CHECK_TIMEOUT),
            );
            if let Some(rationale) = &check.hidden_rationale {
                hidden.insert(check.name.clone(), rationale.clone());
            }
            specs.insert(check.name.clone(), index);
        }
        // Every check must be reachable from the implicit root: a
        // dangling dependency or a dependency cycle would otherwise
        // never be dispatched and hang the scheduler.
        let mut reachable = std::collections::HashSet::new();
        let mut stack: Vec<String> = dependents.get(&None).cloned().unwrap_or_default();
        while let Some(name) = stack.pop() {
            if reachable.insert(name.clone())
                && let Some(children) = dependents.get(&Some(name))
            {
                stack.extend(children.iter().cloned());
            }
        }
        for check in checks {
            assert!(
                reachable.contains(&check.name),
                "check `{}` is unreachable (dangling dependency or dependency cycle?)",
                check.name
            );
        }
        Self {
            order,
            dependents,
            dependency_of,
            descriptions,
            timeouts,
            hidden,
            specs,
        }
    }

    /// The minimal subgraph containing every target and all of their
    /// (transitive) dependencies (check50: `build_subgraph` +
    /// `dependencies_of`). Returns the dependency-rooted `dependents`
    /// map restricted to that subgraph.
    #[must_use]
    pub fn subgraph(&self, targets: &[String]) -> Option<HashMap<Option<String>, Vec<String>>> {
        // Walk up the dependency chain from every target; unknown names
        // are an error (check50: "Unknown check").
        let mut keep: std::collections::HashSet<String> = std::collections::HashSet::new();
        for target in targets {
            let mut current = Some(target.clone());
            while let Some(name) = current {
                if !keep.insert(name.clone()) {
                    break;
                }
                if !self.dependency_of.contains_key(&name) {
                    return None; // unknown check
                }
                current = self.dependency_of.get(&name).cloned().flatten();
            }
        }
        let mut subgraph: HashMap<Option<String>, Vec<String>> = HashMap::new();
        for name in &self.order {
            if keep.contains(name) {
                subgraph
                    .entry(self.dependency_of[name].clone())
                    .or_default()
                    .push(name.clone());
            }
        }
        Some(subgraph)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::RunKind;

    fn spec(name: &str, dependency: Option<&str>) -> CheckSpec {
        CheckSpec {
            name: name.to_owned(),
            description: name.to_owned(),
            dependency: dependency.map(str::to_owned),
            timeout: None,
            hidden_rationale: None,
            run: RunKind::Native(|_ctx| Ok(())),
        }
    }

    #[test]
    fn declaration_order_is_preserved() {
        let checks = vec![spec("exists", None), spec("compiles", Some("exists"))];
        let graph = Graph::new(&checks);
        assert_eq!(graph.order, vec!["exists", "compiles"]);
    }

    #[test]
    fn root_dependents_are_the_dependency_free_checks() {
        let checks = vec![spec("exists", None), spec("compiles", Some("exists"))];
        let graph = Graph::new(&checks);
        assert_eq!(graph.dependents[&None], vec!["exists"]);
        assert_eq!(
            graph.dependents[&Some("exists".to_owned())],
            vec!["compiles"]
        );
    }

    #[test]
    fn subgraph_includes_transitive_dependencies_of_a_target() {
        let checks = vec![
            spec("exists", None),
            spec("compiles", Some("exists")),
            spec("runs", Some("compiles")),
            spec("unrelated", None),
        ];
        let graph = Graph::new(&checks);
        let subgraph = graph
            .subgraph(&["runs".to_owned()])
            .expect("runs is a known check");
        assert_eq!(subgraph[&None], vec!["exists"]);
        assert_eq!(subgraph[&Some("exists".to_owned())], vec!["compiles"]);
        assert_eq!(subgraph[&Some("compiles".to_owned())], vec!["runs"]);
        assert!(
            !subgraph.contains_key(&None) || !subgraph[&None].contains(&"unrelated".to_owned())
        );
    }

    #[test]
    fn subgraph_of_an_unknown_target_is_none() {
        let checks = vec![spec("exists", None)];
        let graph = Graph::new(&checks);
        assert!(graph.subgraph(&["nope".to_owned()]).is_none());
    }

    #[test]
    #[should_panic(expected = "duplicate check name")]
    fn duplicate_check_names_are_rejected() {
        let checks = vec![spec("exists", None), spec("exists", None)];
        let _ = Graph::new(&checks);
    }

    #[test]
    #[should_panic(expected = "unreachable")]
    fn a_dangling_dependency_is_rejected() {
        let checks = vec![spec("exists", Some("ghost"))];
        let _ = Graph::new(&checks);
    }

    #[test]
    #[should_panic(expected = "unreachable")]
    fn a_dependency_cycle_is_rejected() {
        let checks = vec![spec("a", Some("b")), spec("b", Some("a"))];
        let _ = Graph::new(&checks);
    }

    #[test]
    #[should_panic(expected = "not a valid path component")]
    fn path_unsafe_check_names_are_rejected() {
        let checks = vec![spec("../escape", None)];
        let _ = Graph::new(&checks);
    }

    #[test]
    fn the_default_timeout_is_applied_when_unset() {
        let checks = vec![spec("exists", None)];
        let graph = Graph::new(&checks);
        assert_eq!(graph.timeouts["exists"], std::time::Duration::from_mins(1));
    }
}
