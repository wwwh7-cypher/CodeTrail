use crate::{
    analysis,
    graph::{Function, Graph},
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    path::Path,
    process::Command,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub key: String,
    pub status: String,
    pub reason: String,
    pub before: Option<Function>,
    pub after: Option<Function>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Claims {
    pub mentioned: Vec<String>,
    pub ambiguous: Vec<String>,
    pub modules: Vec<String>,
    pub unmentioned_upstream: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub base_commit: String,
    pub head: String,
    pub changes: Vec<Change>,
    pub claim: String,
    pub upstream_before: Vec<String>,
    pub upstream_after: Vec<String>,
    pub entries_before: Vec<String>,
    pub entries_after: Vec<String>,
    pub comparison: Claims,
    pub before_graph: Box<Graph>,
    pub diagnostics: Vec<String>,
    #[serde(default)]
    pub file_changes: Vec<serde_json::Value>,
    #[serde(default)]
    pub edge_changes: Vec<serde_json::Value>,
}

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .output()
        .context("Git is required for diff review")?;
    ensure!(
        out.status.success(),
        "Git failed: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(out.stdout)
}
fn commit(repo: &Path, reference: &str) -> Result<String> {
    ensure!(
        !reference.is_empty() && reference.len() < 256,
        "Invalid Git reference"
    );
    let reference = format!("{reference}^{{commit}}");
    let oid = String::from_utf8(git(
        repo,
        &["rev-parse", "--verify", "--end-of-options", &reference],
    )?)?
    .trim()
    .to_string();
    ensure!(
        oid.len() >= 40 && oid.chars().all(|c| c.is_ascii_hexdigit()),
        "Invalid Git object ID"
    );
    Ok(oid)
}
fn commit_graph(repo: &Path, oid: &str) -> Result<Graph> {
    let listing = git(repo, &["ls-tree", "-r", "-z", "--long", oid])?;
    let mut paths = vec![];
    let mut sources = vec![];
    let mut diagnostics = vec![];
    for record in listing.split(|b| *b == 0).filter(|v| !v.is_empty()) {
        let Some(split) = record.iter().position(|b| *b == b'\t') else {
            continue;
        };
        let header = String::from_utf8_lossy(&record[..split]);
        let fields = header.split_whitespace().collect::<Vec<_>>();
        if fields.first() != Some(&"100644") && fields.first() != Some(&"100755") {
            continue;
        }
        let file = String::from_utf8(record[split + 1..].to_vec())?;
        if !analysis::supported_file(&file) {
            continue;
        }
        if fields
            .get(3)
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(usize::MAX)
            > 2_000_000
        {
            diagnostics.push(format!("{file}: 文件超过 2 MB，跳过"));
            continue;
        }
        paths.push(file);
    }
    for chunk in paths.chunks(40) {
        let mut args = vec!["archive", "--format=tar", oid, "--"];
        args.extend(chunk.iter().map(String::as_str));
        let bytes = git(repo, &args)?;
        let mut archive = tar::Archive::new(Cursor::new(bytes));
        for entry in archive.entries()? {
            let mut entry = entry?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let file = entry.path()?.to_string_lossy().replace('\\', "/");
            if !analysis::supported_file(&file) {
                continue;
            }
            if entry.size() > 2_000_000 {
                diagnostics.push(format!("{file}: 文件超过 2 MB，跳过"));
                continue;
            }
            let mut source = String::new();
            match entry.read_to_string(&mut source) {
                Ok(_) => sources.push((file, source)),
                Err(_) => diagnostics.push(format!("{file}: 非 UTF-8 文件，跳过")),
            }
        }
    }
    analysis::scan_sources(
        &repo.file_name().unwrap_or_default().to_string_lossy(),
        &sources,
        diagnostics,
    )
}

fn symbol_groups(graph: &Graph) -> BTreeMap<String, Vec<&Function>> {
    let mut result: BTreeMap<String, Vec<&Function>> = BTreeMap::new();
    for node in &graph.nodes {
        result
            .entry(node.symbol_key.clone())
            .or_default()
            .push(node)
    }
    result
}
fn outgoing(graph: &Graph, node: &Function) -> BTreeSet<String> {
    let keys = graph
        .nodes
        .iter()
        .map(|n| (&n.id, &n.symbol_key))
        .collect::<BTreeMap<_, _>>();
    graph
        .edges
        .iter()
        .filter(|e| e.from == node.id)
        .filter_map(|e| keys.get(&e.to).map(|s| (*s).clone()))
        .collect()
}
fn mentions(text: &str, name: &str) -> bool {
    if name.starts_with('<') || name.is_empty() {
        return false;
    }
    text.match_indices(name).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let after = text[start + name.len()..].chars().next();
        let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}
pub fn compare_claims(graph: &Graph, claim: &str, upstream: &BTreeSet<String>) -> Claims {
    let mut names: BTreeMap<&str, Vec<&Function>> = BTreeMap::new();
    for n in &graph.nodes {
        if n.kind != "module" {
            names.entry(&n.name).or_default().push(n)
        }
    }
    let mut result = Claims::default();
    for (name, nodes) in names {
        if mentions(claim, name) {
            if nodes.len() == 1 {
                result.mentioned.push(nodes[0].id.clone())
            } else {
                let qualified: Vec<_> = nodes
                    .iter()
                    .filter(|n| {
                        claim.contains(&n.id) || claim.contains(&format!("{}::{}", n.file, n.name))
                    })
                    .collect();
                if qualified.len() == 1 {
                    result.mentioned.push(qualified[0].id.clone())
                } else {
                    result.ambiguous.push(name.into())
                }
            }
        }
    }
    result.modules = graph
        .nodes
        .iter()
        .map(|n| n.file.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|f| claim.replace('\\', "/").contains(f))
        .collect();
    result.unmentioned_upstream = upstream
        .iter()
        .filter(|id| {
            !graph.changed.contains(id)
                && !result.mentioned.contains(id)
                && !graph
                    .nodes
                    .iter()
                    .find(|n| &n.id == *id)
                    .is_some_and(|n| result.modules.contains(&n.file))
        })
        .cloned()
        .collect();
    result
}

/// Compare full function bodies and resolved outgoing targets in both snapshots.
pub fn compare(
    mut before: Graph,
    mut after: Graph,
    base_commit: String,
    head: String,
    claim: String,
    files_with_top_level_changes: BTreeSet<String>,
) -> Graph {
    if before.entries.is_empty() {
        before.entries = before.candidate_entries()
    }
    if after.entries.is_empty() {
        after.entries = after.candidate_entries()
    }
    let old = symbol_groups(&before);
    let new = symbol_groups(&after);
    let keys = old
        .keys()
        .chain(new.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut changes = vec![];
    let mut diagnostics = vec![];
    for key in keys {
        let left = old.get(&key).cloned().unwrap_or_default();
        let right = new.get(&key).cloned().unwrap_or_default();
        if left.len() > 1 || right.len() > 1 {
            let mut old_bodies = left
                .iter()
                .map(|n| (&n.content_hash, outgoing(&before, n)))
                .collect::<Vec<_>>();
            let mut new_bodies = right
                .iter()
                .map(|n| (&n.content_hash, outgoing(&after, n)))
                .collect::<Vec<_>>();
            old_bodies.sort();
            new_bodies.sort();
            if old_bodies == new_bodies
                && !right
                    .iter()
                    .any(|n| files_with_top_level_changes.contains(&n.file))
            {
                continue;
            }
            diagnostics.push(format!("同名符号无法跨版本唯一匹配：{key}；保守标为修改"));
            for n in left {
                changes.push(Change {
                    key: key.clone(),
                    status: "ambiguous-before".into(),
                    reason: "duplicate-symbol".into(),
                    before: Some(n.clone()),
                    after: None,
                })
            }
            for n in right {
                changes.push(Change {
                    key: key.clone(),
                    status: "ambiguous-after".into(),
                    reason: "duplicate-symbol".into(),
                    before: None,
                    after: Some(n.clone()),
                })
            }
            continue;
        }
        match (left.first(), right.first()) {
            (Some(a), Some(b)) => {
                let reason = if a.content_hash != b.content_hash {
                    "function-body"
                } else if outgoing(&before, a) != outgoing(&after, b) {
                    "resolved-call-targets"
                } else if files_with_top_level_changes.contains(&b.file) {
                    "file-level-change-conservative"
                } else {
                    continue;
                };
                changes.push(Change {
                    key,
                    status: "modified".into(),
                    reason: reason.into(),
                    before: Some((*a).clone()),
                    after: Some((*b).clone()),
                });
            }
            (Some(a), None) => changes.push(Change {
                key,
                status: "deleted".into(),
                reason: "missing-in-head".into(),
                before: Some((*a).clone()),
                after: None,
            }),
            (None, Some(b)) => changes.push(Change {
                key,
                status: "added".into(),
                reason: "new-in-head".into(),
                before: None,
                after: Some((*b).clone()),
            }),
            _ => {}
        }
    }
    before.changed = changes
        .iter()
        .filter_map(|c| c.before.as_ref().map(|n| n.id.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    after.changed = changes
        .iter()
        .filter_map(|c| c.after.as_ref().map(|n| n.id.clone()))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let upstream_before = before
        .changed
        .iter()
        .flat_map(|id| before.upstream(id))
        .collect::<BTreeSet<_>>();
    let upstream_after = after
        .changed
        .iter()
        .flat_map(|id| after.upstream(id))
        .collect::<BTreeSet<_>>();
    let comparison = compare_claims(&after, &claim, &upstream_after);
    let entries_before = before
        .entries
        .iter()
        .filter(|id| upstream_before.contains(*id))
        .cloned()
        .collect();
    let entries_after = after
        .entries
        .iter()
        .filter(|id| upstream_after.contains(*id))
        .cloned()
        .collect();
    before.claim = claim.clone();
    after.claim = claim.clone();
    let edge_changes = compare_edges(&before, &after);
    after.review = Some(Review {
        base_commit,
        head,
        changes,
        claim,
        upstream_before: upstream_before.into_iter().collect(),
        upstream_after: upstream_after.into_iter().collect(),
        entries_before,
        entries_after,
        comparison,
        before_graph: Box::new(before),
        diagnostics,
        file_changes: vec![],
        edge_changes,
    });
    after
}

pub fn git_review(
    repo: &Path,
    base: &str,
    head: Option<&str>,
    claim: String,
    entries: &[String],
) -> Result<Graph> {
    let top = String::from_utf8(git(repo, &["rev-parse", "--show-toplevel"])?)?;
    ensure!(
        Path::new(top.trim()).canonicalize()? == repo.canonicalize()?,
        "Review the Git repository root (not a subfolder)"
    );
    let base_commit = commit(repo, base)?;
    let mut before = commit_graph(repo, &base_commit)?;
    let head_oid = head.map(|reference| commit(repo, reference)).transpose()?;
    let mut after = if let Some(oid) = &head_oid {
        commit_graph(repo, oid)?
    } else {
        analysis::scan(repo)?
    };
    for name in entries {
        if let Ok(id) = before.resolve(name) {
            before.entries.push(id)
        }
        if let Ok(id) = after.resolve(name) {
            after.entries.push(id)
        }
    }
    before.explicit_entries = !before.entries.is_empty();
    after.explicit_entries = !after.entries.is_empty();
    let args = if let Some(reference) = head_oid.as_deref() {
        vec![
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--unified=0",
            &base_commit,
            reference,
            "--",
        ]
    } else {
        vec![
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--unified=0",
            &base_commit,
            "--",
        ]
    };
    let diff = String::from_utf8(git(repo, &args)?)?;
    // Import/module changes outside known function ranges conservatively affect that file.
    let mut file = None::<String>;
    let mut file_level = BTreeSet::new();
    let hunk = regex::Regex::new(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")?;
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            file = Some(path.to_string())
        } else if line == "+++ /dev/null" {
            file = None
        } else if let (Some(file), Some(c)) = (&file, hunk.captures(line)) {
            let start = c[1].parse::<usize>().unwrap_or(1);
            let length = c
                .get(2)
                .map(|v| v.as_str().parse::<usize>().unwrap_or(1))
                .unwrap_or(1);
            let end = start + length.saturating_sub(1);
            if !after.nodes.iter().any(|n| {
                n.file == *file && n.kind != "module" && n.line <= start && n.end_line >= end
            }) {
                file_level.insert(file.clone());
            }
        }
    }
    let mut graph = compare(
        before,
        after,
        base_commit,
        head.unwrap_or("working-tree (includes untracked source)")
            .into(),
        claim,
        file_level,
    );
    if let Some(review) = &mut graph.review {
        let mut args = vec![
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--name-status",
            "-z",
            &review.base_commit,
        ];
        if let Some(oid) = head_oid.as_deref() {
            args.push(oid)
        }
        args.push("--");
        let listing = git(repo, &args)?;
        let fields = listing
            .split(|b| *b == 0)
            .filter(|v| !v.is_empty())
            .map(|b| String::from_utf8_lossy(b).to_string())
            .collect::<Vec<_>>();
        let mut i = 0;
        while i + 1 < fields.len() {
            let status = fields[i].clone();
            let file = fields[i + 1].clone();
            i += 2;
            if (status.starts_with('R') || status.starts_with('C')) && i < fields.len() {
                let new = fields[i].clone();
                i += 1;
                review
                    .file_changes
                    .push(serde_json::json!({"path":new,"old_path":file,"status":status}));
            } else {
                review
                    .file_changes
                    .push(serde_json::json!({"path":file,"status":status}));
            }
        }
        if head.is_none() {
            for file in git(repo, &["ls-files", "--others", "--exclude-standard", "-z"])?
                .split(|b| *b == 0)
                .filter(|v| !v.is_empty())
            {
                review.file_changes.push(
                    serde_json::json!({"path":String::from_utf8_lossy(file),"status":"untracked"}),
                );
            }
        }
        review.diagnostics.push("基线来自 Git archive（遵循 export-ignore）；文件重命名按删除/新增呈现。影响为静态可达性，非运行时行为证明。".into());
    }
    Ok(graph)
}

fn compare_edges(before: &Graph, after: &Graph) -> Vec<serde_json::Value> {
    fn keyed(g: &Graph) -> BTreeMap<(String, String), (String, usize)> {
        let ids = g
            .nodes
            .iter()
            .map(|n| (&n.id, &n.symbol_key))
            .collect::<BTreeMap<_, _>>();
        g.edges
            .iter()
            .filter_map(|e| {
                Some((
                    (ids.get(&e.from)?.to_string(), ids.get(&e.to)?.to_string()),
                    (e.file.clone(), e.line),
                ))
            })
            .collect()
    }
    let old = keyed(before);
    let new = keyed(after);
    let mut out = vec![];
    for ((from, to), (file, line)) in &new {
        out.push(serde_json::json!({"from":from,"to":to,"file":file,"line":line,"status":if old.contains_key(&(from.clone(),to.clone())){"unchanged"}else{"added"}}))
    }
    for ((from, to), (file, line)) in &old {
        if !new.contains_key(&(from.clone(), to.clone())) {
            out.push(
                serde_json::json!({"from":from,"to":to,"file":file,"line":line,"status":"deleted"}),
            )
        }
    }
    out
}

pub fn report_markdown(graph: &Graph) -> Result<String> {
    let Some(r) = &graph.review else {
        bail!("Graph does not contain a Git review")
    };
    let mut text = format!(
        "# CodeTrail 静态影响面报告\n\n基线：`{}`\n目标：`{}`\n源码快照：`{}`\n\n## 修改函数\n\n",
        r.base_commit, r.head, graph.snapshot
    );
    for c in &r.changes {
        let n = c.after.as_ref().or(c.before.as_ref()).unwrap();
        text.push_str(&format!(
            "- **{}** `{}` — {}\n",
            c.status, n.symbol_key, c.reason
        ));
    }
    text.push_str(&format!("\n## 潜在影响\n\n- 基线节点：{}，入口：{}\n- 当前节点：{}，入口：{}\n- 声明未提及的当前上游：{}\n\n",r.upstream_before.len(),r.entries_before.len(),r.upstream_after.len(),r.entries_after.len(),r.comparison.unmentioned_upstream.len()));
    for id in &r.comparison.unmentioned_upstream {
        text.push_str(&format!("- `{id}`\n"));
    }
    text.push_str(
        "\n## 分析边界\n\n未提及不等于声明错误。动态调用、类型分派和框架隐式连接可能遗漏。\n",
    );
    for d in &r.diagnostics {
        text.push_str(&format!("- {d}\n"));
    }
    Ok(text)
}
