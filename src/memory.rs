use crate::{graph::Graph, review, write_atomic};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const TOKEN_LIMIT: usize = 1500;
pub const RECORD_LIMIT: usize = 200;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub created_unix: u64,
    pub snapshot: String,
    pub functions: Vec<String>,
    pub signatures: Vec<String>,
    pub conclusion: String,
    pub evidence: Value,
    pub status: String,
    pub confirmed: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub stable_snapshot: String,
    pub stable_tokens: usize,
    pub stable_sha256: String,
    pub records: Vec<Record>,
}

pub fn tokens(text: &str) -> Result<usize> {
    ensure!(
        text.len() <= 65_536,
        "Stable memory exceeds the 64 KiB input limit"
    );
    Ok(tiktoken_rs::cl100k_base()?.encode_ordinary(text).len())
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub struct Store {
    root: PathBuf,
}
impl Store {
    pub fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let root = root.canonicalize()?;
        for child in ["manifest.json", "ARCHITECTURE.md", ".lock", "records"] {
            let path = root.join(child);
            if path.exists() {
                ensure!(
                    path.canonicalize()?.starts_with(&root),
                    "Memory store path escapes its root: {child}"
                );
            }
            if let Ok(meta) = fs::symlink_metadata(&path) {
                ensure!(
                    !meta.file_type().is_symlink(),
                    "Memory store refuses symbolic links: {child}"
                );
            }
        }
        Ok(Self { root })
    }
    fn lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.root.join(".lock"))?;
        file.lock_exclusive()?;
        Ok(file)
    }
    fn load(&self) -> Result<Manifest> {
        let path = self.root.join("manifest.json");
        if !path.exists() {
            return Ok(Manifest {
                version: 1,
                ..Manifest::default()
            });
        }
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }
    fn save(&self, manifest: &Manifest) -> Result<()> {
        write_atomic(
            &self.root.join("manifest.json"),
            serde_json::to_vec_pretty(manifest)?,
        )
    }
    pub fn status(&self, graph: &Graph) -> Result<Value> {
        let _guard = self.lock()?;
        let manifest = self.load()?;
        let stable = fs::read_to_string(self.root.join("ARCHITECTURE.md")).unwrap_or_default();
        let edited = !stable.is_empty()
            && format!("{:x}", Sha256::digest(stable.as_bytes())) != manifest.stable_sha256;
        let stale =
            !manifest.stable_snapshot.is_empty() && manifest.stable_snapshot != graph.snapshot;
        let records=manifest.records.iter().map(|r|json!({"id":r.id,"created_unix":r.created_unix,"snapshot":r.snapshot,"functions":r.functions,"conclusion":r.conclusion,"status":if r.status=="active"&&r.snapshot!=graph.snapshot{"stale"}else{r.status.as_str()},"confirmed":r.confirmed})).collect::<Vec<_>>();
        Ok(
            json!({"stable":stable,"stable_tokens":if edited{tokens(&stable)?}else{manifest.stable_tokens},"token_limit":TOKEN_LIMIT,"tokenizer":"cl100k_base","stable_snapshot":manifest.stable_snapshot,"stale":stale,"externally_edited":edited,"records":records,"path":self.root}),
        )
    }
    pub fn save_stable(&self, graph: &Graph, text: &str, confirmed: bool) -> Result<Value> {
        ensure!(
            confirmed,
            "Human confirmation is required before persisting stable memory"
        );
        ensure!(!text.trim().is_empty(), "Stable memory cannot be empty");
        let count = tokens(text)?;
        ensure!(
            count <= TOKEN_LIMIT,
            "Stable memory exceeds {TOKEN_LIMIT} tokens ({count}, cl100k_base)"
        );
        let _guard = self.lock()?;
        let mut manifest = self.load()?;
        write_atomic(&self.root.join("ARCHITECTURE.md"), text.as_bytes())?;
        manifest.stable_snapshot = graph.snapshot.clone();
        manifest.stable_tokens = count;
        manifest.stable_sha256 = format!("{:x}", Sha256::digest(text.as_bytes()));
        self.save(&manifest)?;
        Ok(
            json!({"saved":true,"tokens":count,"snapshot":graph.snapshot,"path":self.root.join("ARCHITECTURE.md")}),
        )
    }
    pub fn confirm_review(
        &self,
        graph: &Graph,
        ids: &[String],
        conclusion: &str,
        confirmed: bool,
    ) -> Result<Record> {
        ensure!(
            confirmed,
            "Human confirmation is required before persisting a review"
        );
        ensure!(
            !conclusion.trim().is_empty() && conclusion.len() <= 16_000,
            "Conclusion must contain 1–16000 bytes"
        );
        let ids = if ids.is_empty() {
            if !graph.changed.is_empty() {
                graph.changed.clone()
            } else {
                graph
                    .review
                    .as_ref()
                    .map(|r| {
                        r.before_graph
                            .changed
                            .iter()
                            .map(|id| format!("before::{id}"))
                            .collect()
                    })
                    .unwrap_or_default()
            }
        } else {
            ids.to_vec()
        };
        ensure!(!ids.is_empty(), "Select at least one reviewed function");
        let mut selected = vec![];
        let mut upstream = BTreeSet::new();
        let mut upstream_before = BTreeSet::new();
        for value in &ids {
            let (scope, value, before): (&Graph, &str, bool) =
                if let Some(id) = value.strip_prefix("before::") {
                    (
                        &graph
                            .review
                            .as_ref()
                            .context("Baseline selection requires a Git review")?
                            .before_graph,
                        id,
                        true,
                    )
                } else {
                    (graph, value.as_str(), false)
                };
            let id = scope.resolve(value)?;
            let node = scope
                .nodes
                .iter()
                .find(|n| n.id == id)
                .context("Unknown reviewed function")?;
            selected.push((node, before));
            if before {
                upstream_before.extend(scope.upstream(&id));
            } else {
                upstream.extend(scope.upstream(&id));
            }
        }
        let evidence = json!({"upstream":upstream,"upstream_selected_before":upstream_before,"entries":graph.entries.iter().filter(|id|upstream.contains(*id)).collect::<Vec<_>>(),"edges":graph.edges.iter().filter(|e|upstream.contains(&e.from)&&upstream.contains(&e.to)).collect::<Vec<_>>(),"selected_before_edges":graph.review.as_ref().map(|r|r.before_graph.edges.iter().filter(|e|upstream_before.contains(&e.from)&&upstream_before.contains(&e.to)).collect::<Vec<_>>()),"git_review":graph.review.as_ref().map(|r|json!({"base_commit":r.base_commit,"head":r.head,"changes":r.changes,"upstream_before":r.upstream_before,"entries_before":r.entries_before})),"basis":"potential-static-reach-not-runtime-proof"});
        let stamp = now();
        let hash = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&json!([
                graph.snapshot,
                ids,
                conclusion,
                evidence
            ]))?)
        );
        let id = format!("{stamp}-{}", &hash[..16]);
        let record = Record {
            id: id.clone(),
            created_unix: stamp,
            snapshot: graph.snapshot.clone(),
            functions: selected
                .iter()
                .map(|(n, before)| {
                    if *before {
                        format!("before::{}", n.symbol_key)
                    } else {
                        n.symbol_key.clone()
                    }
                })
                .collect(),
            signatures: selected
                .iter()
                .map(|(n, _)| {
                    n.source
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(300)
                        .collect()
                })
                .collect(),
            conclusion: conclusion.into(),
            evidence,
            status: "active".into(),
            confirmed: true,
        };
        let _guard = self.lock()?;
        let mut manifest = self.load()?;
        if !manifest.records.iter().any(|r| r.id == id) {
            ensure!(
                manifest.records.len() < RECORD_LIMIT,
                "Review history reached {RECORD_LIMIT} records. Prune confirmed stale records before adding more."
            );
            manifest.records.push(record.clone());
        }
        let records = self.root.join("records");
        fs::create_dir_all(&records)?;
        write_atomic(
            &records.join(format!("{id}.md")),
            format!(
                "# 已人工确认的静态影响面结论\n\n- 时间（Unix）：{stamp}\n- 快照：{}\n- 触发函数：{}\n- 函数签名：{}\n\n{}\n\n## 调用证据\n\n```json\n{}\n```\n\n静态可达性不证明业务行为变化；源码变化后应重新确认。\n",
                record.snapshot,
                record.functions.join(", "),
                record.signatures.join("; "),
                record.conclusion,
                serde_json::to_string_pretty(&record.evidence)?
            ),
        )?;
        self.save(&manifest)?;
        Ok(record)
    }
    pub fn mark_stale(&self, id: &str, confirmed: bool) -> Result<Value> {
        ensure!(confirmed, "Human confirmation required");
        let _guard = self.lock()?;
        let mut manifest = self.load()?;
        let record = manifest
            .records
            .iter_mut()
            .find(|r| r.id == id)
            .context("Review record not found")?;
        record.status = "stale".into();
        self.save(&manifest)?;
        Ok(json!({"id":id,"status":"stale"}))
    }
    pub fn prune(&self, graph: &Graph, confirmed: bool) -> Result<Value> {
        ensure!(confirmed, "Human confirmation required before pruning");
        let _guard = self.lock()?;
        let mut manifest = self.load()?;
        let removed = manifest
            .records
            .iter()
            .filter(|r| r.status == "stale" || r.snapshot != graph.snapshot)
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        for id in &removed {
            ensure!(
                id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'),
                "Invalid stored record ID"
            );
            let path = self.root.join("records").join(format!("{id}.md"));
            if path.exists() {
                ensure!(
                    path.canonicalize()?.starts_with(&self.root),
                    "Record path escapes memory store"
                );
                fs::remove_file(path)?;
            }
        }
        manifest.records.retain(|r| !removed.contains(&r.id));
        self.save(&manifest)?;
        Ok(json!({"pruned":removed.len(),"ids":removed}))
    }
    pub fn query(&self, graph: &Graph, text: &str, include_stale: bool) -> Result<Value> {
        let _guard = self.lock()?;
        let manifest = self.load()?;
        let text = text.to_lowercase();
        let records = manifest
            .records
            .iter()
            .filter(|r| {
                (include_stale || (r.status == "active" && r.snapshot == graph.snapshot))
                    && format!("{} {}", r.conclusion, r.functions.join(" "))
                        .to_lowercase()
                        .contains(&text)
            })
            .take(20)
            .collect::<Vec<_>>();
        Ok(
            json!({"records":records,"include_stale":include_stale,"limit":20,"snapshot":graph.snapshot}),
        )
    }
    pub fn stable_context(&self, graph: &Graph) -> Result<Value> {
        let status = self.status(graph)?;
        if status["stale"] == true || status["externally_edited"] == true {
            return Ok(
                json!({"usable":false,"reason":"stale-or-edited-requires-human-reconfirmation","stable_snapshot":status["stable_snapshot"],"current_snapshot":graph.snapshot}),
            );
        }
        if status["stable"].as_str().unwrap_or("").is_empty() {
            return Ok(json!({"usable":false,"reason":"no-confirmed-stable-memory"}));
        }
        Ok(
            json!({"usable":true,"markdown":status["stable"],"tokens":status["stable_tokens"],"tokenizer":"cl100k_base","snapshot":graph.snapshot}),
        )
    }
}

pub fn read_graph(path: &Path) -> Result<Graph> {
    let graph: Graph = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(!graph.snapshot.is_empty(), "Graph has no snapshot");
    Ok(graph)
}
pub fn query_graph(graph: &Graph, operation: &str, args: &Value) -> Result<Value> {
    let string = |key: &str| -> Result<&str> {
        args.get(key)
            .and_then(Value::as_str)
            .with_context(|| format!("Missing string argument: {key}"))
    };
    match operation {
        "find" => {
            let text = string("text")?.to_lowercase();
            Ok(json!(
                graph
                    .nodes
                    .iter()
                    .filter(|n| format!("{} {}", n.name, n.file)
                        .to_lowercase()
                        .contains(&text))
                    .take(100)
                    .collect::<Vec<_>>()
            ))
        }
        "path" => {
            let a = graph.resolve(string("from")?)?;
            let b = graph.resolve(string("to")?)?;
            Ok(
                json!({"path":graph.path(&a,&b),"basis":"resolved-static-calls","snapshot":graph.snapshot}),
            )
        }
        "impact" => {
            let id = graph.resolve(string("function")?)?;
            let upstream = graph.upstream(&id);
            Ok(
                json!({"function":id,"upstream":upstream,"entries":graph.entries.iter().filter(|id|upstream.contains(*id)).collect::<Vec<_>>(),"basis":"potential-static-reach-not-runtime-proof","snapshot":graph.snapshot}),
            )
        }
        "claims" => {
            let targets = graph
                .changed
                .iter()
                .flat_map(|id| graph.upstream(id))
                .collect();
            Ok(serde_json::to_value(review::compare_claims(
                graph,
                string("text")?,
                &targets,
            ))?)
        }
        _ => bail!("Unknown graph query"),
    }
}
