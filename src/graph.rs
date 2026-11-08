use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Function {
    pub id: String,
    pub name: String,
    pub file: String,
    pub line: usize,
    pub end_line: usize,
    pub kind: String,
    pub role: String,
    pub role_source: String,
    pub source: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub parameters: String,
    #[serde(default)]
    pub return_type: String,
    pub language: String,
    #[serde(default)]
    pub symbol_key: String,
    #[serde(default)]
    pub content_hash: String,
    #[serde(skip)]
    pub start: usize,
    #[serde(skip)]
    pub end: usize,
    #[serde(skip)]
    pub scope: Option<String>,
    #[serde(skip)]
    pub class: Option<String>,
    #[serde(skip)]
    pub bindings: Vec<String>,
    #[serde(skip)]
    pub exported: bool,
    #[serde(skip)]
    pub default_export: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub file: String,
    pub line: usize,
    pub resolution: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unresolved {
    pub from: String,
    pub expression: String,
    pub file: String,
    pub line: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Graph {
    pub schema_version: u32,
    pub project: String,
    pub files: usize,
    pub snapshot: String,
    pub generated_unix: u64,
    pub analysis_ms: u64,
    #[serde(default)]
    pub cache_hits: usize,
    #[serde(default)]
    pub parsed_files: usize,
    pub nodes: Vec<Function>,
    pub edges: Vec<Edge>,
    pub unresolved: Vec<Unresolved>,
    pub diagnostics: Vec<String>,
    pub entries: Vec<String>,
    pub explicit_entries: bool,
    pub changed: Vec<String>,
    pub claim: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<crate::review::Review>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewer: Option<serde_json::Value>,
}

impl Graph {
    pub fn resolve(&self, value: &str) -> Result<String> {
        let found: Vec<_> = self
            .nodes
            .iter()
            .filter(|n| n.id == value || n.name == value || n.symbol_key == value)
            .collect();
        match found.as_slice() {
            [node] => Ok(node.id.clone()),
            [] => bail!("No function matches {value:?}. Use query <graph> find <text>."),
            _ => bail!(
                "Ambiguous function {value:?}; use a stable ID: {}",
                found
                    .iter()
                    .map(|n| n.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    pub fn candidate_entries(&self) -> Vec<String> {
        let called: BTreeSet<_> = self.edges.iter().map(|e| &e.to).collect();
        let callers: BTreeSet<_> = self.edges.iter().map(|e| &e.from).collect();
        self.nodes
            .iter()
            .filter(|n| !called.contains(&n.id) && (n.kind != "module" || callers.contains(&n.id)))
            .map(|n| n.id.clone())
            .collect()
    }

    pub fn path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        let mut queue = VecDeque::from([from.to_string()]);
        let mut prev = HashMap::from([(from.to_string(), None::<String>)]);
        while let Some(id) = queue.pop_front() {
            if id == to {
                let mut path = vec![id.clone()];
                let mut current = id;
                while let Some(Some(p)) = prev.get(&current) {
                    path.push(p.clone());
                    current = p.clone();
                }
                path.reverse();
                return Some(path);
            }
            for edge in self.edges.iter().filter(|e| e.from == id) {
                if !prev.contains_key(&edge.to) {
                    prev.insert(edge.to.clone(), Some(id.clone()));
                    queue.push_back(edge.to.clone());
                }
            }
        }
        None
    }

    pub fn upstream(&self, target: &str) -> BTreeSet<String> {
        let mut visited = BTreeSet::from([target.to_string()]);
        let mut queue = VecDeque::from([target.to_string()]);
        while let Some(id) = queue.pop_front() {
            for edge in self.edges.iter().filter(|e| e.to == id) {
                if visited.insert(edge.from.clone()) {
                    queue.push_back(edge.from.clone());
                }
            }
        }
        visited
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycles_and_shortest_path_are_bounded() {
        let mut g = Graph::default();
        for (a, b) in [("a", "b"), ("b", "c"), ("c", "b"), ("a", "c"), ("x", "c")] {
            g.edges.push(Edge {
                from: a.into(),
                to: b.into(),
                file: "test".into(),
                line: 1,
                resolution: "test".into(),
            });
        }
        assert_eq!(g.path("a", "c"), Some(vec!["a".into(), "c".into()]));
        assert_eq!(g.path("c", "a"), None);
        assert_eq!(g.path("a", "a"), Some(vec!["a".into()]));
        assert_eq!(
            g.upstream("c"),
            BTreeSet::from(["a".into(), "b".into(), "c".into(), "x".into()])
        );
    }
}
