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
}
