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
