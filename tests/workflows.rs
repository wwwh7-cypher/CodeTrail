use code_trail::{
    analysis,
    annotations::{self, Annotation, Envelope},
    index,
    mcp::Server,
    memory::Store,
    review,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static COUNT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "codetrail-workflow-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn write(&self, file: &str, text: &str) {
        let p = self.0.join(file);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
    fn git(&self, args: &[&str]) {
        let o = Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn graph(source: &str) -> code_trail::graph::Graph {
    let mut g =
        analysis::scan_sources("test", &[("main.ts".into(), source.into())], vec![]).unwrap();
    g.entries = g.candidate_entries();
    g
}

#[test]
fn neighbors_are_direct_deduplicated_paginated_and_evidenced() {
    let g = graph("function leaf(){} function middle(){leaf();leaf();} function main(){middle();}");
    let result = index::neighbors(&g, "middle", 0, 1).unwrap();
    assert_eq!(result["total"], 2);
    assert_eq!(result["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(result["complete_static_page"], false);
    assert!(
        result["incoming"]
            .as_array()
            .unwrap()
            .iter()
            .chain(result["outgoing"].as_array().unwrap())
            .all(|e| e["line"].as_u64().unwrap() > 0)
    );
    let second = index::neighbors(&g, "middle", 1, 1).unwrap();
    assert_eq!(second["complete_static_page"], true);
}

#[test]
fn go_receivers_and_same_package_cross_file_calls() {
    let g=analysis::scan_sources("go",&[("pkg/a.go".into(),"package app\ntype Service struct {}\nfunc (s *Service) Answer(){s.retrieve(); utility()}\n".into()),("pkg/b.go".into(),"package app\nfunc(s *Service) retrieve(){}\nfunc utility(){}\n".into()),("other/c.go".into(),"package app\nfunc unrelated(){utility()}\n".into())],vec![]).unwrap();
    let answer = g.resolve("Answer").unwrap();
    assert_eq!(g.edges.iter().filter(|e| e.from == answer).count(), 2);
    assert!(
        g.unresolved
            .iter()
            .any(|u| u.file == "other/c.go" && u.expression == "utility")
    );
}

#[test]
fn go_parameter_shadow_does_not_connect_a_package_function() {
    let g = analysis::scan_sources(
        "go",
        &[(
            "a.go".into(),
            "package app\nfunc run(){}\nfunc caller(run func()){run()}\n".into(),
        )],
        vec![],
    )
    .unwrap();
    assert!(g.edges.is_empty());
    assert!(
        g.unresolved
            .iter()
            .any(|c| c.reason == "shadowed-or-assigned-binding")
    );
}

#[test]
fn go_typed_struct_fields_resolve_inside_nested_handler_but_not_interfaces() {
    let g = analysis::scan_sources(
        "go",
        &[(
            "pkg/service.go".into(),
            r#"package app
type Generator interface { Generate() }
type Service struct { Generator Generator }
type API struct { Service *Service }
func (s *Service) Answer() { s.Generator.Generate() }
func register(handler func()) {}
func (a *API) Routes() { register(func() { a.Service.Answer() }) }
"#
            .into(),
        )],
        vec![],
    )
    .unwrap();
    let answer = g.resolve("Answer").unwrap();
    assert!(
        g.edges
            .iter()
            .any(|e| e.to == answer && e.resolution == "go-typed-receiver-static-candidate")
    );
    assert!(
        g.unresolved
            .iter()
            .any(|u| u.expression.contains("Generator.Generate"))
    );
}

#[test]
fn unchanged_annotation_survives_line_shift_and_changed_body_expires() {
    let mut before = graph("function leaf(){return 1}");
    let node = before
        .nodes
        .iter()
        .find(|n| n.name == "leaf")
        .unwrap()
        .clone();
    let snapshot = before.snapshot.clone();
    annotations::apply(
        &mut before,
        Envelope {
            snapshot,
            source: "conversation-model".into(),
            annotations: vec![Annotation {
                id: node.id,
                content_hash: node.content_hash,
                title: "返回常量".into(),
                summary: "返回数值一。".into(),
            }],
        },
    )
    .unwrap();
    let mut after = graph("\n\nfunction leaf(){return 1}");
    assert_eq!(annotations::reuse(&before, &mut after).unwrap(), 1);
    assert_eq!(index::search(&after, "常量", 5)[0].name, "leaf");
    let mut changed = graph("function leaf(){return 2}");
    assert_eq!(annotations::reuse(&before, &mut changed).unwrap(), 0);
}

#[test]
fn cached_scan_invalidates_only_changed_files_and_resolves_imports() {
    let t = Temp::new();
    t.write(
        "src/a.ts",
        "import { run } from './b'; export function main(){return run()}",
    );
    t.write("src/b.ts", "export function run(){return 1}");
    let cache = t.0.join(".cache/parse.json");
    let a = analysis::scan_cached(&t.0, Some(&cache)).unwrap();
    assert_eq!(a.parsed_files, 2);
    let b = analysis::scan_cached(&t.0, Some(&cache)).unwrap();
    assert_eq!(b.cache_hits, 2);
    assert_eq!(a.snapshot, b.snapshot);
    assert_eq!(a.edges.len(), b.edges.len());
    t.write("src/b.ts", "export function run(){return 2}");
    let c = analysis::scan_cached(&t.0, Some(&cache)).unwrap();
    assert_eq!(c.parsed_files, 1);
    assert_eq!(c.cache_hits, 1);
    assert_ne!(b.snapshot, c.snapshot);
    assert_eq!(c.edges.len(), 1);
}

#[test]
fn annotation_validation_cannot_modify_edges_or_accept_stale_source() {
    let mut g = graph("function leaf(){} function main(){leaf();}");
    let n = g.nodes.iter().find(|n| n.name == "main").unwrap().clone();
    let edges = serde_json::to_value(&g.edges).unwrap();
    let valid = Envelope {
        snapshot: g.snapshot.clone(),
        source: "conversation-model".into(),
        annotations: vec![Annotation {
            id: n.id.clone(),
            content_hash: n.content_hash.clone(),
            title: "启动流程".into(),
            summary: "调用 leaf 完成后续处理。".into(),
        }],
    };
    assert_eq!(annotations::apply(&mut g, valid).unwrap(), 1);
    assert_eq!(serde_json::to_value(&g.edges).unwrap(), edges);
    let bad = Envelope {
        snapshot: "outdated".into(),
        source: "conversation-model".into(),
        annotations: vec![],
    };
    assert!(annotations::apply(&mut g, bad).is_err());
    assert!(
        serde_json::from_value::<Envelope>(
            json!({"snapshot":g.snapshot,"source":"conversation-model","annotations":[],"edges":[]})
        )
        .is_err()
    );
    let found = index::search(&g, "启动", 5);
    assert_eq!(found[0].name, "main");
}

#[test]
fn stable_symbol_keys_survive_line_insertions() {
    let a = graph("function main(){return 1}");
    let b = graph("\n\nfunction main(){return 1}");
    let (a, b) = (&a.nodes[0], &b.nodes[0]);
    assert_ne!(a.id, b.id);
    assert_eq!(a.symbol_key, b.symbol_key);
    assert_eq!(a.content_hash, b.content_hash);
}

#[test]
fn review_lf_crlf_and_unchanged_duplicate_methods_are_not_changes() {
    let mut a = graph("const x={run(){}};const y={run(){}};function main(){return 1;}\n");
    let mut b = graph("const x={run(){}};const y={run(){}};function main(){return 1;}\r\n");
    a.entries = a.candidate_entries();
    b.entries = b.candidate_entries();
    let result = review::compare(
        a,
        b,
        "base".into(),
        "work".into(),
        String::new(),
        Default::default(),
    );
    assert!(result.review.unwrap().changes.is_empty());
}

#[test]
fn review_tracks_deleted_nodes_and_edge_deltas() {
    let before = graph("function leaf(){} function main(){leaf();}");
    let after = graph("function main(){}");
    let result = review::compare(
        before,
        after,
        "base".into(),
        "work".into(),
        String::new(),
        Default::default(),
    );
    let r = result.review.unwrap();
    assert!(r.changes.iter().any(|c| c.status == "deleted"));
    assert!(!r.upstream_before.is_empty());
    assert!(r.edge_changes.iter().any(|c| c["status"] == "deleted"));
}

#[test]
fn git_review_has_non_function_changes_and_clean_code_is_zero() {
    let t = Temp::new();
    t.git(&["init", "-q"]);
    t.git(&["config", "user.name", "CodeTrail test"]);
    t.git(&["config", "user.email", "test@example.invalid"]);
    t.write("main.ts", "function main(){return 1}\n");
    t.write("config.json", "{\"port\":1}\n");
    t.git(&["add", "."]);
    t.git(&["commit", "-qm", "fixture"]);
    t.write("config.json", "{\"port\":2}\n");
    let g = review::git_review(&t.0, "HEAD", None, String::new(), &[]).unwrap();
    let r = g.review.unwrap();
    assert!(r.changes.is_empty());
    assert!(r.file_changes.iter().any(|c| c["path"] == "config.json"));
}

#[test]
fn memory_confirmation_budget_staleness_and_prune() {
    let t = Temp::new();
    let mut g = graph("function main(){return 1}");
    g.changed = vec![g.resolve("main").unwrap()];
    let store = Store::open(&t.0).unwrap();
    assert!(store.save_stable(&g, "# 入口\nmain", false).is_err());
    assert!(
        store
            .save_stable(&g, &"不同的词 ".repeat(3000), true)
            .is_err()
    );
    for _ in 0..12 {
        store
            .save_stable(&g, "# 架构共识\n\n入口：main", true)
            .unwrap();
    }
    let status = store.status(&g).unwrap();
    assert!(status["stable_tokens"].as_u64().unwrap() <= 1500);
    assert_eq!(status["records"].as_array().unwrap().len(), 0);
    let record = store
        .confirm_review(&g, &[], "已复核主流程的静态关系。", true)
        .unwrap();
    assert_eq!(
        store.query(&g, "主流程", false).unwrap()["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let other = graph("function main(){return 2}");
    assert_eq!(store.stable_context(&other).unwrap()["usable"], false);
    assert!(
        store.query(&other, "", false).unwrap()["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.query(&other, "", true).unwrap()["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    store.mark_stale(&record.id, true).unwrap();
    assert_eq!(store.prune(&g, true).unwrap()["pruned"], 1);
}

#[test]
fn mcp_lifecycle_read_only_tools_and_errors() {
    let t = Temp::new();
    let g = graph("function main(){}");
    let file = t.0.join("graph.json");
    fs::write(&file, serde_json::to_vec(&g).unwrap()).unwrap();
    let mut s = Server::new(&file, &t.0.join("memory")).unwrap();
    let call = |method: &str, id: i64, params: Value| json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    assert!(s.dispatch(call("tools/list", 0, json!({}))).unwrap()["error"].is_object());
    assert_eq!(
        s.dispatch(call(
            "initialize",
            1,
            json!({"protocolVersion":"2025-11-25"})
        ))
        .unwrap()["result"]["protocolVersion"],
        "2025-11-25"
    );
    assert!(
        s.dispatch(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .is_none()
    );
    let tools = s.dispatch(call("tools/list", 2, json!({}))).unwrap();
    assert!(
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["annotations"]["readOnlyHint"] == true)
    );
    let found = s
        .dispatch(call(
            "tools/call",
            3,
            json!({"name":"code_trail_find","arguments":{"text":"main"}}),
        ))
        .unwrap();
    assert_eq!(found["result"]["isError"], false);
    let write = s
        .dispatch(call(
            "tools/call",
            4,
            json!({"name":"write_memory","arguments":{}}),
        ))
        .unwrap();
    assert_eq!(write["result"]["isError"], true);
}

#[test]
fn deleted_function_memory_retains_baseline_identity_signature_and_reach() {
    let t = Temp::new();
    let before = graph("function removed(value){return value} function main(){removed(1)}");
    let id = before.resolve("removed").unwrap();
    let after = graph("function main(){}");
    let reviewed = review::compare(
        before,
        after,
        "base".into(),
        "work".into(),
        String::new(),
        Default::default(),
    );
    let record = Store::open(&t.0)
        .unwrap()
        .confirm_review(
            &reviewed,
            &[format!("before::{id}")],
            "已核对删除函数及其基线调用方。",
            true,
        )
        .unwrap();
    assert!(record.functions[0].starts_with("before::"));
    assert!(record.signatures[0].contains("removed(value)"));
    assert!(
        !record.evidence["selected_before_edges"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
