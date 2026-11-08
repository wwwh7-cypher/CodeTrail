use crate::graph::{Function, Graph};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub id: String,
    pub name: String,
    pub file: String,
    pub symbol_key: String,
    pub score: i64,
    pub matched_field: String,
    pub title: String,
    pub summary: String,
    pub changed: bool,
}

/// Bounded subsequence matcher with word-boundary and consecutive bonuses.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let q = query.to_lowercase().chars().collect::<Vec<_>>();
    let source = text.chars().collect::<Vec<_>>();
    let t = text.to_lowercase().chars().collect::<Vec<_>>();
    if q.len() > t.len() {
        return None;
    }
    if q == t {
        return Some(2000);
    }
    if let Some(pos) = text.to_lowercase().find(&query.to_lowercase()) {
        return Some(
            1200 - (pos.min(200) as i64) - (t.len().saturating_sub(q.len()).min(100) as i64),
        );
    }
    let (mut at, mut previous, mut score) = (0, None::<usize>, 0i64);
    for (i, c) in t.iter().enumerate() {
        if at < q.len() && *c == q[at] {
            score += 24;
            if i == 0
                || source
                    .get(i.wrapping_sub(1))
                    .is_some_and(|v| !v.is_alphanumeric())
                || source.get(i).is_some_and(|v| v.is_uppercase())
            {
                score += 18
            }
            if previous == i.checked_sub(1) {
                score += 20
            } else if let Some(p) = previous {
                score -= (i - p - 1).min(12) as i64
            }
            previous = Some(i);
            at += 1;
        }
    }
    if at == q.len() {
        return Some(score - (t.len().min(120) as i64) / 4);
    }
    if q.iter().all(|c| ('\u{3400}'..='\u{9fff}').contains(c)) {
        let mut available = t.clone();
        if q.iter().all(|c| {
            available
                .iter()
                .position(|v| v == c)
                .map(|i| {
                    available.remove(i);
                    true
                })
                .unwrap_or(false)
        }) {
            return Some(30 + q.len() as i64 * 6 - (t.len().min(120) as i64) / 4);
        }
    }
    None
}
fn note<'a>(g: &'a Graph, n: &Function) -> (String, String) {
    let a = g
        .viewer
        .as_ref()
        .and_then(|v| v.get("annotations"))
        .and_then(|a| a.get(&n.id));
    if let Some(a) =
        a.filter(|a| a.get("content_hash").and_then(Value::as_str) == Some(n.content_hash.as_str()))
    {
        return (
            a["title"].as_str().unwrap_or(&n.name).into(),
            a["summary"].as_str().unwrap_or(&n.role).into(),
        );
    }
    (
        n.name.clone(),
        if n.role_source == "no-description" {
            String::new()
        } else {
            n.role.clone()
        },
    )
}
pub fn search(g: &Graph, query: &str, limit: usize) -> Vec<SearchHit> {
    let terms = query.split_whitespace().collect::<Vec<_>>();
    let mut result = vec![];
    for n in &g.nodes {
        if n.kind == "module" || n.name.starts_with('<') {
            continue;
        }
        let (title, summary) = note(g, n);
        let module = g
            .viewer
            .as_ref()
            .and_then(|v| v["module_groups"].as_array())
            .and_then(|groups| {
                groups
                    .iter()
                    .find(|v| v["prefix"].as_str().is_some_and(|p| n.file.starts_with(p)))
            })
            .and_then(|v| v["name"].as_str())
            .unwrap_or("");
        let fields = [
            ("name", n.name.as_str(), 120i64),
            ("title", title.as_str(), 100),
            ("file", n.file.as_str(), 20),
            ("summary", summary.as_str(), 0),
            ("module", module, 30),
        ];
        let mut score = 0;
        let mut matched = "name";
        let mut accepted = true;
        for term in &terms {
            let best = fields
                .iter()
                .filter_map(|(key, text, weight)| {
                    fuzzy_score(term, text).map(|s| (s + weight, *key))
                })
                .max_by_key(|p| p.0);
            if let Some((s, key)) = best {
                score += s;
                matched = key
            } else {
                accepted = false;
                break;
            }
        }
        if accepted {
            result.push(SearchHit {
                id: n.id.clone(),
                name: n.name.clone(),
                file: n.file.clone(),
                symbol_key: n.symbol_key.clone(),
                score,
                matched_field: matched.into(),
                title,
                summary,
                changed: g.changed.contains(&n.id),
            })
        }
    }
    result.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(b.changed.cmp(&a.changed))
            .then(a.name.cmp(&b.name))
            .then(a.file.cmp(&b.file))
    });
    result.truncate(limit.clamp(1, 200));
    result
}
pub fn neighbors(g: &Graph, id: &str, offset: usize, limit: usize) -> anyhow::Result<Value> {
    let id = g.resolve(id)?;
    let incoming = g.edges.iter().filter(|e| e.to == id).collect::<Vec<_>>();
    let outgoing = g.edges.iter().filter(|e| e.from == id).collect::<Vec<_>>();
    let ids = incoming
        .iter()
        .map(|e| e.from.clone())
        .chain(outgoing.iter().map(|e| e.to.clone()))
        .collect::<BTreeSet<_>>();
    let page = ids
        .iter()
        .skip(offset)
        .take(limit.clamp(1, 200))
        .cloned()
        .collect::<BTreeSet<_>>();
    Ok(
        json!({"function":id,"snapshot":g.snapshot,"total":ids.len(),"offset":offset,"nodes":g.nodes.iter().filter(|n|page.contains(&n.id)).collect::<Vec<_>>(),"incoming":incoming.into_iter().filter(|e|page.contains(&e.from)).collect::<Vec<_>>(),"outgoing":outgoing.into_iter().filter(|e|page.contains(&e.to)).collect::<Vec<_>>(),"unresolved":g.unresolved.iter().filter(|u|u.from==id).collect::<Vec<_>>(),"complete_static_page":offset+page.len()>=ids.len()}),
    )
}
pub fn module_key(file: &str) -> String {
    let parts = file.split('/').collect::<Vec<_>>();
    if parts.len() <= 1 {
        return "(root)".into();
    }
    let mut depth = if parts.first() == Some(&"src") {
        3
    } else if parts.first() == Some(&"backend") {
        3
    } else {
        2
    };
    depth = depth.min(parts.len() - 1);
    parts[..depth].join("/")
}
pub fn modules(g: &Graph) -> Value {
    let mut nodes: BTreeMap<String, Value> = BTreeMap::new();
    let id_module = g
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), module_key(&n.file)))
        .collect::<HashMap<_, _>>();
    for n in &g.nodes {
        let key = module_key(&n.file);
        let entry = nodes
            .entry(key.clone())
            .or_insert_with(|| json!({"id":key,"functions":[],"files":[]}));
        if n.kind != "module" {
            entry["functions"].as_array_mut().unwrap().push(json!(n.id));
        }
        if !entry["files"].as_array().unwrap().contains(&json!(n.file)) {
            entry["files"].as_array_mut().unwrap().push(json!(n.file));
        }
    }
    let mut edges: BTreeMap<(String, String), Vec<Value>> = BTreeMap::new();
    for e in &g.edges {
        if let (Some(a), Some(b)) = (id_module.get(e.from.as_str()), id_module.get(e.to.as_str())) {
            if a != b {
                edges.entry((a.clone(),b.clone())).or_default().push(json!({"from":e.from,"to":e.to,"file":e.file,"line":e.line,"resolution":e.resolution}));
            }
        }
    }
    json!({"snapshot":g.snapshot,"modules":nodes.into_values().collect::<Vec<_>>(),"edges":edges.into_iter().map(|((from,to),evidence)|json!({"from":from,"to":to,"kind":"aggregated-static-calls","call_sites":evidence.len(),"evidence":evidence})).collect::<Vec<_>>()})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camel_case_and_subsequence() {
        assert!(fuzzy_score("pBtx", "parseBibtex").is_some());
        assert!(fuzzy_score("pbib", "parseBibliography").is_some());
        assert!(fuzzy_score("xyz", "parseConfig").is_none());
        assert!(
            fuzzy_score("parse", "parse").unwrap() > fuzzy_score("parse", "parseConfig").unwrap()
        );
    }
    #[test]
    fn chinese_subsequence() {
        assert!(fuzzy_score("作者拆分", "作者姓名拆分").is_some());
        assert!(fuzzy_score("拆作者", "拆分作者姓名").is_some());
    }
}
