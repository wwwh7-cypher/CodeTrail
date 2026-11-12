use crate::graph::{Function, Graph, ModuleGroup};
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
        let module = node_module(g, n);
        let fields = [
            ("name", n.name.as_str(), 120i64),
            ("title", title.as_str(), 100),
            ("file", n.file.as_str(), 20),
            ("summary", summary.as_str(), 0),
            ("module", module.as_str(), 30),
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
/// 按模块配置计算单个文件的模块键。
///
/// 先按 `groups` 的声明顺序匹配路径前缀；没有命中时按 `/` 分段：单段文件直接以文件路径
/// 本身作为模块键（扁平仓库按文件分组），多段文件的深度 = `depth`（0 表示自动：
/// `src`/`backend` 取 3 段，其余取 2 段），再收缩到 `parts.len() - 1`，取前 depth 段用
/// `/` 连接。
pub fn module_key_with(file: &str, depth: u32, groups: &[ModuleGroup]) -> String {
    for group in groups {
        if file.starts_with(&group.prefix) {
            return group.name.clone();
        }
    }
    let parts = file.split('/').collect::<Vec<_>>();
    if parts.len() <= 1 {
        return parts[0].to_string();
    }
    let automatic = if parts.first() == Some(&"src") || parts.first() == Some(&"backend") {
        3
    } else {
        2
    };
    let depth = if depth > 0 { depth as usize } else { automatic };
    let depth = depth.min(parts.len() - 1);
    parts[..depth].join("/")
}

/// 旧调用点的默认模块键：自动深度、无自定义分组。
pub fn module_key(file: &str) -> String {
    module_key_with(file, 0, &[])
}

/// 节点的模块键：新图直接使用已写入的 `module` 字段，旧图（字段为空）按同一算法回退。
pub fn node_module(graph: &Graph, node: &Function) -> String {
    if !node.module.is_empty() {
        node.module.clone()
    } else {
        module_key_with(&node.file, graph.module_depth, &graph.module_groups)
    }
}

/// 解析可重复的 `--module-group NAME=PREFIX`；名称与前缀都必须非空。
pub fn parse_module_groups(values: &[String]) -> anyhow::Result<Vec<ModuleGroup>> {
    let mut groups = Vec::new();
    for value in values {
        let Some((name, prefix)) = value.split_once('=') else {
            anyhow::bail!("--module-group 需要 NAME=PREFIX 形式：{value}");
        };
        let (name, prefix) = (name.trim(), prefix.trim());
        anyhow::ensure!(
            !name.is_empty() && !prefix.is_empty(),
            "--module-group 的名称与前缀都不能为空：{value}"
        );
        groups.push(ModuleGroup {
            name: name.into(),
            prefix: prefix.into(),
        });
    }
    Ok(groups)
}

/// 构图完成后把模块配置写回代码图，并填充每个节点的模块键（含审查基线内嵌图与变更对）。
pub fn apply_module_config(graph: &mut Graph, depth: u32, groups: Vec<ModuleGroup>) {
    graph.module_depth = depth;
    graph.module_groups = groups;
    let depth = graph.module_depth;
    let groups = graph.module_groups.clone();
    for node in &mut graph.nodes {
        node.module = module_key_with(&node.file, depth, &groups);
    }
    if let Some(review) = graph.review.as_mut() {
        review.before_graph.module_depth = depth;
        review.before_graph.module_groups = groups.clone();
        for node in &mut review.before_graph.nodes {
            node.module = module_key_with(&node.file, depth, &groups);
        }
        for change in &mut review.changes {
            if let Some(node) = change.before.as_mut() {
                node.module = module_key_with(&node.file, depth, &groups);
            }
            if let Some(node) = change.after.as_mut() {
                node.module = module_key_with(&node.file, depth, &groups);
            }
        }
    }
}

/// 模块级 SCC（Kosaraju）：只返回节点数大于 1 的强连通分量，按字典序稳定输出。
fn module_cycles(adjacency: &BTreeMap<String, BTreeSet<String>>) -> Vec<Vec<String>> {
    fn finish(
        node: &str,
        adjacency: &BTreeMap<String, BTreeSet<String>>,
        seen: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) {
        if !seen.insert(node.to_string()) {
            return;
        }
        if let Some(targets) = adjacency.get(node) {
            for target in targets {
                finish(target, adjacency, seen, order);
            }
        }
        order.push(node.to_string());
    }
    fn collect(
        node: &str,
        reverse: &BTreeMap<String, BTreeSet<String>>,
        seen: &mut BTreeSet<String>,
        component: &mut Vec<String>,
    ) {
        if !seen.insert(node.to_string()) {
            return;
        }
        component.push(node.to_string());
        if let Some(sources) = reverse.get(node) {
            for source in sources {
                collect(source, reverse, seen, component);
            }
        }
    }
    let mut order = vec![];
    let mut seen = BTreeSet::new();
    for node in adjacency.keys() {
        finish(node, adjacency, &mut seen, &mut order);
    }
    let mut reverse = adjacency
        .keys()
        .map(|node| (node.clone(), BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for (from, targets) in adjacency {
        for to in targets {
            reverse.entry(to.clone()).or_default().insert(from.clone());
        }
    }
    let mut cycles = vec![];
    let mut seen = BTreeSet::new();
    for node in order.iter().rev() {
        if seen.contains(node.as_str()) {
            continue;
        }
        let mut component = vec![];
        collect(node, &reverse, &mut seen, &mut component);
        if component.len() > 1 {
            component.sort();
            cycles.push(component);
        }
    }
    cycles.sort();
    cycles
}

/// 模块摘要查询：默认只输出统计与跨模块边；`full` 时展开文件、函数与调用证据。
///
/// `instability` = 出向调用点 / (入向 + 出向调用点)，分母为 0 时为 null。
pub fn modules(graph: &Graph, full: bool) -> Value {
    let mut files: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut functions: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut id_module: HashMap<&str, String> = HashMap::new();
    for node in &graph.nodes {
        let module = node_module(graph, node);
        id_module.insert(node.id.as_str(), module.clone());
        files.entry(module.clone()).or_default().insert(node.file.clone());
        if node.kind != "module" {
            functions
                .entry(module)
                .or_default()
                .insert(node.id.clone());
        }
    }
    for module in files.keys() {
        functions.entry(module.clone()).or_default();
    }
    let mut cross = BTreeMap::<(String, String), Vec<Value>>::new();
    for edge in &graph.edges {
        let (Some(from), Some(to)) = (
            id_module.get(edge.from.as_str()),
            id_module.get(edge.to.as_str()),
        ) else {
            continue;
        };
        if from == to {
            continue;
        }
        cross
            .entry((from.clone(), to.clone()))
            .or_default()
            .push(json!({"from":edge.from,"to":edge.to,"file":edge.file,"line":edge.line,"resolution":edge.resolution}));
    }
    let mut adjacency = files
        .keys()
        .map(|module| (module.clone(), BTreeSet::new()))
        .collect::<BTreeMap<String, BTreeSet<String>>>();
    let mut incoming_modules = BTreeMap::<String, BTreeSet<String>>::new();
    let mut outgoing_modules = BTreeMap::<String, BTreeSet<String>>::new();
    let mut incoming_sites = BTreeMap::<String, usize>::new();
    let mut outgoing_sites = BTreeMap::<String, usize>::new();
    for ((from, to), evidence) in &cross {
        adjacency.get_mut(from).unwrap().insert(to.clone());
        outgoing_modules
            .entry(from.clone())
            .or_default()
            .insert(to.clone());
        incoming_modules
            .entry(to.clone())
            .or_default()
            .insert(from.clone());
        *outgoing_sites.entry(from.clone()).or_default() += evidence.len();
        *incoming_sites.entry(to.clone()).or_default() += evidence.len();
    }
    let cycles = module_cycles(&adjacency);
    let mut items = vec![];
    for (id, module_files) in &files {
        let module_functions = &functions[id];
        let incoming_sites = incoming_sites.get(id).copied().unwrap_or(0);
        let outgoing_sites = outgoing_sites.get(id).copied().unwrap_or(0);
        let total = incoming_sites + outgoing_sites;
        let instability = if total == 0 {
            Value::Null
        } else {
            json!(outgoing_sites as f64 / total as f64)
        };
        let mut item = json!({
            "id": id,
            "name": id,
            "files_count": module_files.len(),
            "functions_count": module_functions.len(),
            "incoming_modules": incoming_modules.get(id).map_or(0, BTreeSet::len),
            "outgoing_modules": outgoing_modules.get(id).map_or(0, BTreeSet::len),
            "incoming_call_sites": incoming_sites,
            "outgoing_call_sites": outgoing_sites,
            "instability": instability,
        });
        if full {
            item["files"] = json!(module_files.iter().collect::<Vec<_>>());
            item["functions"] = json!(module_functions.iter().collect::<Vec<_>>());
        }
        items.push(item);
    }
    let mut edges = vec![];
    for ((from, to), evidence) in &cross {
        let mut item = json!({"from":from,"to":to,"call_sites":evidence.len()});
        if full {
            item["evidence"] = json!(evidence);
        }
        edges.push(item);
    }
    json!({
        "snapshot": graph.snapshot,
        "module_config": {"depth": graph.module_depth, "groups": graph.module_groups},
        "summary": {
            "modules": files.len(),
            "files": files.values().map(BTreeSet::len).sum::<usize>(),
            "functions": functions.values().map(BTreeSet::len).sum::<usize>(),
            "cross_module_pairs": cross.len(),
            "cross_module_call_sites": cross.values().map(Vec::len).sum::<usize>(),
            "cycles": cycles.len(),
        },
        "modules": items,
        "edges": edges,
        "cycles": cycles,
    })
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
    #[test]
    fn module_keys_follow_groups_and_path_depth() {
        // 单段路径按文件本身分组，扁平仓库不会退化成一个 (root) 模块。
        assert_eq!(module_key("main.ts"), "main.ts");
        assert_eq!(module_key("cli.js"), "cli.js");
        assert_eq!(module_key("examples/config-service/config.ts"), "examples/config-service");
        assert_eq!(module_key("tools/generate.js"), "tools");
        assert_eq!(module_key("lib/util/index.ts"), "lib/util");
        assert_eq!(module_key("src/core/config.ts"), "src/core");
        assert_eq!(module_key("src/core/lib/parse.ts"), "src/core/lib");
        assert_eq!(module_key("backend/jobs/worker.py"), "backend/jobs");
        assert_eq!(
            module_key("backend/jobs/queue/worker.py"),
            "backend/jobs/queue"
        );
        // 自定义分组优先于路径自动分段，并按声明顺序取胜。
        let groups = vec![
            ModuleGroup {
                name: "业务".into(),
                prefix: "src/core/".into(),
            },
            ModuleGroup {
                name: "核心".into(),
                prefix: "src/".into(),
            },
        ];
        assert_eq!(module_key_with("src/core/config.ts", 0, &groups), "业务");
        assert_eq!(module_key_with("src/other/x.ts", 0, &groups), "核心");
        let ordered = vec![
            ModuleGroup {
                name: "宽".into(),
                prefix: "src/".into(),
            },
            ModuleGroup {
                name: "窄".into(),
                prefix: "src/core/".into(),
            },
        ];
        assert_eq!(module_key_with("src/core/config.ts", 0, &ordered), "宽");
        // 显式深度优先，并 clamp 到 parts.len()-1。
        assert_eq!(module_key_with("src/core/lib/parse.ts", 1, &[]), "src");
        assert_eq!(module_key_with("a/b/c/d.ts", 2, &[]), "a/b");
        assert_eq!(module_key_with("a/b/c/d.ts", 10, &[]), "a/b/c");
    }
    #[test]
    fn module_cycles_use_strongly_connected_components() {
        let mut adjacency = BTreeMap::new();
        for (from, to) in [
            ("api", "core"),
            ("core", "api"),
            ("core", "db"),
            ("db", "core"),
            ("api", "db"),
            ("ui", "api"),
        ] {
            adjacency
                .entry(from.to_string())
                .or_insert_with(BTreeSet::new)
                .insert(to.to_string());
        }
        for node in ["ui", "api", "core", "db", "solo"] {
            adjacency.entry(node.to_string()).or_insert_with(BTreeSet::new);
        }
        let cycles = module_cycles(&adjacency);
        assert_eq!(
            cycles,
            vec![vec![
                "api".to_string(),
                "core".to_string(),
                "db".to_string()
            ]]
        );
        // 单节点自环不是跨模块循环。
        adjacency.get_mut("solo").unwrap().insert("solo".into());
        assert_eq!(module_cycles(&adjacency), cycles);
    }
    #[test]
    fn modules_fall_back_to_the_shared_algorithm_for_old_graphs() {
        let mut graph = crate::analysis::scan_sources(
            "test",
            &[
                (
                    "src/a/main.ts".into(),
                    "export function main(){return run()}".into(),
                ),
                ("src/a/run.ts".into(), "export function run(){return 1}".into()),
            ],
            vec![],
        )
        .unwrap();
        // 模拟旧图：没有 module 字段与模块配置。
        graph.module_depth = 0;
        graph.module_groups.clear();
        for node in &mut graph.nodes {
            node.module.clear();
        }
        let summary = modules(&graph, false);
        assert_eq!(summary["module_config"]["depth"], 0);
        assert_eq!(summary["summary"]["modules"], 1);
        assert_eq!(summary["modules"][0]["id"], "src/a");
        assert!(summary["modules"][0].get("files").is_none());
        assert!(summary["modules"][0].get("functions").is_none());
        assert_eq!(summary["modules"][0]["files_count"], 2);
        assert_eq!(summary["modules"][0]["instability"], Value::Null);
        let full = modules(&graph, true);
        assert!(full["modules"][0]["files"].is_array());
        assert!(full["modules"][0]["functions"].is_array());
        assert!(
            full["edges"]
                .as_array()
                .unwrap()
                .iter()
                .all(|edge| edge.get("evidence").is_some())
        );
    }
    #[test]
    fn modules_configures_groups_and_reports_cross_module_edges() {
        let mut graph = crate::analysis::scan_sources(
            "test",
            &[
                (
                    "src/core/main.ts".into(),
                    "import { run } from './run'; import { util } from '../../lib/util'; export function main(){run();util()}".into(),
                ),
                (
                    "src/core/run.ts".into(),
                    "export function run(){return 1}".into(),
                ),
                ("lib/util.ts".into(), "export function util(){return 2}".into()),
            ],
            vec![],
        )
        .unwrap();
        apply_module_config(
            &mut graph,
            0,
            vec![ModuleGroup {
                name: "核心".into(),
                prefix: "src/core/".into(),
            }],
        );
        let summary = modules(&graph, false);
        assert_eq!(summary["module_config"]["groups"][0]["name"], "核心");
        assert_eq!(summary["module_config"]["groups"][0]["prefix"], "src/core/");
        assert_eq!(summary["summary"]["modules"], 2);
        assert_eq!(summary["summary"]["cross_module_pairs"], 1);
        assert_eq!(summary["summary"]["cross_module_call_sites"], 1);
        assert_eq!(summary["modules"][0]["id"], "lib");
        assert_eq!(summary["modules"][0]["incoming_call_sites"], 1);
        assert_eq!(summary["modules"][0]["instability"], 0.0);
        assert_eq!(summary["modules"][1]["id"], "核心");
        assert_eq!(summary["modules"][1]["outgoing_call_sites"], 1);
        assert_eq!(summary["modules"][1]["instability"], 1.0);
        assert_eq!(summary["edges"][0]["from"], "核心");
        assert_eq!(summary["edges"][0]["to"], "lib");
        assert_eq!(summary["edges"][0]["call_sites"], 1);
        assert!(graph.nodes.iter().all(|node| !node.module.is_empty()));
    }
}
