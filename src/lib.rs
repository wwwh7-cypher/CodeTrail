pub mod analysis;
pub mod annotations;
pub mod graph;
pub mod index;
pub mod mcp;
pub mod memory;
pub mod review;

use anyhow::{Context, Result};
use std::{fs, path::Path};

pub fn write_atomic(path: &Path, bytes: impl AsRef<[u8]>) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, bytes).with_context(|| format!("Cannot write {}", temp.display()))?;
    // File persistence is guarded by the memory-store lock where multiple writes must agree.
    if let Err(e) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(e.into());
    }
    Ok(())
}

pub fn render(graph: &graph::Graph) -> Result<String> {
    let data = serde_json::to_string(graph)?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    Ok(viewer_template().replace("/*__GRAPH_DATA__*/", &data))
}

pub fn viewer_template() -> String {
    include_str!("../assets/viewer.html")
        .replace(
            "/*__VIEWER_STYLE__*/",
            &format!(
                "{}\n{}\n{}",
                include_str!("../assets/viewer.css"),
                include_str!("../assets/workbench.css"),
                include_str!("../assets/explorer.css")
            ),
        )
        .replace(
            "/*__VIEWER_SCRIPT__*/",
            &format!(
                "{}\n{}\n{}",
                include_str!("../assets/viewer.js"),
                include_str!("../assets/workbench.js"),
                include_str!("../assets/explorer.js")
            ),
        )
        .replace(
            "<!--__WORKBENCH_HTML__-->",
            include_str!("../assets/workbench.html"),
        )
}
