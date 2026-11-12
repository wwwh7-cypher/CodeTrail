use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp {
    root: PathBuf,
}
impl Temp {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "code-trail-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
    fn write(&self, name: &str, text: &str) {
        let path = self.root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn exe() -> Command {
    Command::new(env!("CARGO_BIN_EXE_code-trail"))
}
fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn scan_graph(t: &Temp, args: &[&str]) -> (Value, PathBuf) {
    let out = t.root.join("out/module.html");
    let mut command = exe();
    command.arg("scan").arg(t.root.join("repo")).args(args);
    let result = command
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let graph = out.with_extension("graph.json");
    let value = serde_json::from_slice(&fs::read(&graph).unwrap()).unwrap();
    (value, graph)
}

#[test]
fn scan_honors_ignores_and_emits_safe_self_contained_html() {
    let t = Temp::new();
    t.write("repo/.gitignore", "ignored.js\n");
    t.write("repo/ignored.js", "function ignored(){}\n");
    t.write("repo/node_modules/pkg/a.js", "function dependency(){}\n");
    t.write("repo/a.ts","// </script><script>globalThis.INJECTED=true</script>\nfunction sink(){}\nexport function main(){sink();}\n");
    let out = t.root.join("out/view.html");
    let result = exe()
        .arg("scan")
        .arg(t.root.join("repo"))
        .args(["--entry", "main", "--changed", "sink", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let graph: Value =
        serde_json::from_slice(&fs::read(out.with_extension("graph.json")).unwrap()).unwrap();
    assert_eq!(graph["files"], 1);
    assert_eq!(graph["edges"].as_array().unwrap().len(), 1);
    let html = fs::read_to_string(out).unwrap();
    assert!(!html.contains("</script><script>globalThis.INJECTED"));
    assert!(html.contains("\\u003c/script\\u003e"));
    let query = exe()
        .arg("query")
        .arg(t.root.join("out/view.graph.json"))
        .args(["path", "main", "sink"])
        .output()
        .unwrap();
    assert!(query.status.success());
    let query: Value = serde_json::from_slice(&query.stdout).unwrap();
    assert_eq!(query["path"].as_array().unwrap().len(), 2);
}

#[test]
fn ambiguous_change_is_an_error_without_an_artifact() {
    let t = Temp::new();
    t.write("repo/a.js", "function run(){}");
    t.write("repo/b.js", "function run(){}");
    let out = t.root.join("no.html");
    let result = exe()
        .arg("scan")
        .arg(t.root.join("repo"))
        .args(["--changed", "run", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Ambiguous"));
    assert!(!out.exists());
}

#[test]
fn fingerprints_are_content_based_and_entry_order_is_preserved() {
    let t = Temp::new();
    t.write("repo/a.js", "function z(){}\nfunction a(){z();}");
    let scan = |out: &str| -> Value {
        let out = t.root.join(out);
        let result = exe()
            .arg("scan")
            .arg(t.root.join("repo"))
            .args(["--entry", "z", "--entry", "a", "--out"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(result.status.success());
        serde_json::from_slice(&fs::read(out.with_extension("graph.json")).unwrap()).unwrap()
    };
    let first = scan("first.html");
    let second = scan("second.html");
    assert_eq!(first["snapshot"], second["snapshot"]);
    assert!(first["entries"][0].as_str().unwrap().ends_with(":z"));
    t.write("repo/a.js", "function z(){return 1;}\nfunction a(){z();}");
    let third = scan("third.html");
    assert_ne!(first["snapshot"], third["snapshot"]);
}

fn help_text(args: &[&str]) -> String {
    let output = exe().args(args).arg("--help").output().unwrap();
    assert!(
        output.status.success(),
        "help failed for {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn every_subcommand_and_nested_operation_has_chinese_help_text() {
    let top = help_text(&[]);
    for needle in [
        "扫描仓库",
        "Git 变更",
        "查询已生成的代码图",
        "重新渲染",
        "稳定记忆",
        "MCP stdio",
        "token 数",
        "查看器模板",
        "函数标注模板",
        "应用标注",
    ] {
        assert!(
            top.contains(needle),
            "top-level help is missing {needle:?}: {top}"
        );
    }
    for (command, needle) in [
        ("scan", "扫描仓库中的 TypeScript"),
        ("review", "比较 Git 基线"),
        ("query", "在 JSON 代码图上执行路径"),
        ("render", "重新渲染为自包含"),
        ("memory", "管理经确认的稳定记忆"),
        ("mcp", "以 stdio 方式提供只读 MCP"),
        ("tokens", "cl100k_base"),
        ("template", "内置的 HTML 查看器模板"),
        ("prepare-annotations", "待填写的标注模板"),
        ("annotate", "校验并应用人工标注"),
    ] {
        assert!(
            help_text(&[command]).contains(needle),
            "`{command} --help` is missing {needle:?}"
        );
    }
    let query = help_text(&["query"]);
    for needle in [
        "查找两个符号间的最短静态调用路径",
        "上游入口",
        "按名称查找符号",
        "能力声明",
        "模糊搜索符号",
        "直接邻居",
        "模块级节点",
    ] {
        assert!(
            query.contains(needle),
            "`query --help` is missing {needle:?}: {query}"
        );
    }
    let memory = help_text(&["memory"]);
    for needle in [
        "查看记忆库状态",
        "导出稳定记忆上下文",
        "保存稳定记忆",
        "确认审查结论",
        "标记记忆已过期",
        "清理过期记忆",
        "检索记忆",
    ] {
        assert!(
            memory.contains(needle),
            "`memory --help` is missing {needle:?}: {memory}"
        );
    }
    // 新增的模块选项必须出现在中文帮助里。
    let scan = help_text(&["scan"]);
    assert!(scan.contains("--module-depth") && scan.contains("--module-group"));
    assert!(scan.contains("模块分段深度"));
    let review = help_text(&["review"]);
    assert!(review.contains("--module-depth") && review.contains("--module-group"));
    let modules = help_text(&["query", "modules"]);
    assert!(modules.contains("--full"));
    assert!(modules.contains("instability"));
}

#[test]
fn scan_writes_module_configuration_and_module_keys() {
    let t = Temp::new();
    t.write(
        "repo/src/core/config.ts",
        "export function parseConfig(){return 1}\n",
    );
    t.write(
        "repo/src/core/main.ts",
        "import {parseConfig} from './config'; export function main(){return parseConfig()}\n",
    );
    t.write("repo/lib/util.ts", "export function util(){return 2}\n");
    let (graph, _) = scan_graph(
        &t,
        &[
            "--module-depth",
            "1",
            "--module-group",
            "核心=src/core/",
            "--entry",
            "main",
        ],
    );
    assert_eq!(graph["module_depth"], 1);
    assert_eq!(graph["module_groups"][0]["name"], "核心");
    assert_eq!(graph["module_groups"][0]["prefix"], "src/core/");
    for node in graph["nodes"].as_array().unwrap() {
        let module = node["module"].as_str().unwrap();
        assert!(!module.is_empty(), "node without module: {node}");
        if node["file"].as_str().unwrap().starts_with("src/core/") {
            assert_eq!(module, "核心");
        } else {
            assert_eq!(module, "lib", "{node}");
        }
    }
}

#[test]
fn flat_repository_groups_single_segment_files_as_their_own_modules() {
    let t = Temp::new();
    t.write(
        "repo/main.ts",
        "import {parse} from './config'; export function main(){return parse()}\n",
    );
    t.write("repo/config.ts", "export function parse(){return 1}\n");
    t.write("repo/jobs.py", "def run():\n    return 1\n");
    let (graph, path) = scan_graph(&t, &[]);
    let modules = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["module"].as_str().unwrap().to_string())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        modules,
        ["config.ts", "jobs.py", "main.ts"]
            .map(String::from)
            .into_iter()
            .collect()
    );
    let result = exe()
        .arg("query")
        .arg(&path)
        .arg("modules")
        .output()
        .unwrap();
    assert!(result.status.success());
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["summary"]["modules"], 3);
    assert_eq!(summary["summary"]["cross_module_pairs"], 1);
    assert_eq!(summary["summary"]["cross_module_call_sites"], 1);
    let edges = summary["edges"].as_array().unwrap();
    assert_eq!(edges[0]["from"], "main.ts");
    assert_eq!(edges[0]["to"], "config.ts");
}

#[test]
fn invalid_module_group_fails_with_a_chinese_error() {
    let t = Temp::new();
    t.write("repo/a.ts", "export function a(){}\n");
    let out = t.root.join("bad.html");
    for value in ["core", "=src/", "core="] {
        let result = exe()
            .arg("scan")
            .arg(t.root.join("repo"))
            .args(["--module-group", value, "--out"])
            .arg(&out)
            .output()
            .unwrap();
        assert!(!result.status.success(), "{value:?} should be rejected");
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(stderr.contains("--module-group"), "{stderr}");
        assert!(!out.exists());
    }
}

#[test]
fn query_modules_returns_summary_edges_cycles_and_full_details() {
    let t = Temp::new();
    t.write(
        "repo/src/a/one.ts",
        "import {two} from '../b/two'; export function one(){return two()}\n",
    );
    t.write(
        "repo/src/b/two.ts",
        "import {one} from '../a/one'; export function two(){return one()}\n",
    );
    let (_, graph) = scan_graph(&t, &[]);
    let run = |extra: &[&str]| -> Value {
        let result = exe()
            .arg("query")
            .arg(&graph)
            .arg("modules")
            .args(extra)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&result.stdout).unwrap()
    };
    let summary = run(&[]);
    assert_eq!(summary["module_config"]["depth"], 0);
    assert_eq!(summary["summary"]["modules"], 2);
    assert_eq!(summary["summary"]["files"], 2);
    assert_eq!(summary["summary"]["functions"], 2);
    assert_eq!(summary["summary"]["cross_module_pairs"], 2);
    assert_eq!(summary["summary"]["cross_module_call_sites"], 2);
    assert_eq!(summary["summary"]["cycles"], 1);
    assert_eq!(summary["cycles"], serde_json::json!([["src/a", "src/b"]]));
    let modules = summary["modules"].as_array().unwrap();
    assert_eq!(modules[0]["id"], "src/a");
    assert_eq!(modules[1]["id"], "src/b");
    for module in modules {
        for key in [
            "id",
            "name",
            "files_count",
            "functions_count",
            "incoming_modules",
            "outgoing_modules",
            "incoming_call_sites",
            "outgoing_call_sites",
            "instability",
        ] {
            assert!(module.get(key).is_some(), "missing {key}: {module}");
        }
        assert_eq!(module["files_count"], 1);
        assert_eq!(module["functions_count"], 1);
        // 摘要形态不展开文件、函数与证据。
        assert!(module.get("files").is_none());
        assert!(module.get("functions").is_none());
    }
    assert_eq!(modules[0]["instability"], 0.5);
    assert_eq!(modules[1]["instability"], 0.5);
    let edges = summary["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 2);
    for edge in edges {
        assert_eq!(edge["call_sites"], 1);
        assert!(edge.get("evidence").is_none());
    }
    let full = run(&["--full"]);
    let modules = full["modules"].as_array().unwrap();
    assert!(modules[0]["files"].as_array().unwrap().len() == 1);
    assert!(modules[0]["functions"].as_array().unwrap().len() == 1);
    let edges = full["edges"].as_array().unwrap();
    let evidence = edges[0]["evidence"].as_array().unwrap();
    assert_eq!(evidence.len(), 1);
    assert!(evidence[0]["file"].is_string() && evidence[0]["line"].is_number());
}

#[test]
fn legacy_graph_without_module_fields_still_queries_and_renders() {
    let t = Temp::new();
    t.write(
        "repo/src/core/main.ts",
        "export function main(){return 1}\n",
    );
    t.write("repo/lib/util.ts", "export function util(){return 2}\n");
    let (mut graph, path) = scan_graph(&t, &[]);
    let object = graph.as_object_mut().unwrap();
    object.remove("module_depth");
    object.remove("module_groups");
    for node in object["nodes"].as_array_mut().unwrap() {
        node.as_object_mut().unwrap().remove("module");
    }
    let legacy = t.root.join("legacy.graph.json");
    fs::write(&legacy, serde_json::to_vec(&graph).unwrap()).unwrap();
    assert!(path.exists());
    let result = exe()
        .arg("query")
        .arg(&legacy)
        .arg("modules")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    let ids = summary["modules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["lib".to_string(), "src/core".to_string()]);
    let render = exe()
        .arg("render")
        .arg(&legacy)
        .arg("--out")
        .arg(t.root.join("legacy.html"))
        .output()
        .unwrap();
    assert!(
        render.status.success(),
        "{}",
        String::from_utf8_lossy(&render.stderr)
    );
}

#[test]
fn review_writes_module_configuration_into_its_embedded_baseline() {
    let t = Temp::new();
    let repo = t.root.join("repo");
    fs::create_dir_all(repo.join("src/core")).unwrap();
    fs::create_dir_all(repo.join("src/api")).unwrap();
    fs::write(
        repo.join("src/core/main.ts"),
        "export function main(){return 1}\n",
    )
    .unwrap();
    fs::write(
        repo.join("src/api/handler.ts"),
        "export function handler(){return 2}\n",
    )
    .unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.name", "CodeTrail test"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "fixture"]);
    fs::write(
        repo.join("src/core/main.ts"),
        "export function main(){return 3}\n",
    )
    .unwrap();
    let out = t.root.join("review/review.html");
    let result = exe()
        .arg("review")
        .arg(&repo)
        .args(["--module-group", "核心=src/core/", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let graph: Value =
        serde_json::from_slice(&fs::read(out.with_extension("graph.json")).unwrap()).unwrap();
    assert_eq!(graph["module_groups"][0]["name"], "核心");
    let baseline = &graph["review"]["before_graph"];
    assert_eq!(baseline["module_groups"][0]["name"], "核心");
    for node in graph["nodes"].as_array().unwrap() {
        let module = node["module"].as_str().unwrap();
        assert!(!module.is_empty());
        if node["file"].as_str().unwrap().starts_with("src/core/") {
            assert_eq!(module, "核心");
        }
    }
    for node in baseline["nodes"].as_array().unwrap() {
        assert!(!node["module"].as_str().unwrap().is_empty());
    }
}
