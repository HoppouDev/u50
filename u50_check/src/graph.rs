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

impl Graph {
    /// Builds the graph from the plugin's checks (in declaration order).
    /// Panics on duplicate check names (a plugin-authoring bug, caught
    /// by the registry tests).
    #[must_use]
    pub fn new(checks: &[CheckSpec]) -> Self {
        let mut order = Vec::new();
        let mut dependents: HashMap<Option<String>, Vec<String>> = HashMap::new();
        let mut dependency_of = HashMap::new();
        let mut descriptions = HashMap::new();
        let mut timeouts = HashMap::new();
        let mut hidden = HashMap::new();
        let mut specs = HashMap::new();
        for (index, check) in checks.iter().enumerate() {
            order.push(check.name.clone());
            dependents
                .entry(check.dependency.clone())
                .or_default()
                .push(check.name.clone());
            dependency_of.insert(check.name.clone(), check.dependency.clone());
            descriptions.insert(check.name.clone(), check.description.clone());
            timeouts.insert(
                check.name.clone(),
                check.timeout.unwrap_or(std::time::Duration::from_mins(1)),
            );
            if let Some(rationale) = &check.hidden_rationale {
                hidden.insert(check.name.clone(), rationale.clone());
            }
            specs.insert(check.name.clone(), index);
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
            let dependency = self.dependency_of[name].clone();
            if let Some(children) = self.dependents.get(&dependency)
                && children.contains(name)
            {
                let name = name.clone();
                if keep.contains(&name) {
                    subgraph
                        .entry(self.dependency_of[&name].clone())
                        .or_default()
                        .push(name);
                }
            }
        }
        Some(subgraph)
    }
}
