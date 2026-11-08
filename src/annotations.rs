use crate::graph::Graph;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Annotation {
    pub id: String,
    pub content_hash: String,
    pub title: String,
    pub summary: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub snapshot: String,
    pub source: String,
    pub annotations: Vec<Annotation>,
}
pub fn apply(graph: &mut Graph, envelope: Envelope) -> Result<usize> {
    ensure!(
        envelope.snapshot == graph.snapshot,
        "Source snapshot changed; regenerate annotations"
    );
    ensure!(
        ["conversation-model", "byok-model", "source-reviewed"].contains(&envelope.source.as_str()),
        "Invalid annotation source"
    );
    ensure!(
        envelope.annotations.len() <= 200,
        "At most 200 annotations per batch"
    );
    let mut seen = BTreeSet::new();
    for item in &envelope.annotations {
        let n = graph
            .nodes
            .iter()
            .find(|n| n.id == item.id)
            .context("Unknown annotation symbol")?;
        ensure!(
            n.content_hash == item.content_hash,
            "Function content changed: {}",
            item.id
        );
        ensure!(seen.insert(&item.id), "Duplicate annotation symbol");
        ensure!(
            !item.title.trim().is_empty()
                && item.title.chars().count() <= 60
                && !item.summary.trim().is_empty()
                && item.summary.chars().count() <= 400,
            "Invalid title or summary"
        );
    }
    let viewer = graph.viewer.get_or_insert_with(|| json!({}));
    ensure!(viewer.is_object(), "Viewer metadata must be an object");
    if viewer.get("annotations").is_none() {
        viewer["annotations"] = json!({})
    }
    ensure!(
        viewer["annotations"].is_object(),
        "Invalid existing annotations"
    );
    for item in &envelope.annotations {
        viewer["annotations"][&item.id] = json!({"title":item.title,"summary":item.summary,"content_hash":item.content_hash,"source":envelope.source,"revision":graph.snapshot,"confirmed":false});
    }
    Ok(envelope.annotations.len())
}
pub fn prepare(graph: &Graph, ids: &[String]) -> Result<Value> {
    let ids = ids
        .iter()
        .map(|id| graph.resolve(id))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        !ids.is_empty() && ids.len() <= 12,
        "Select 1–12 functions per conversation batch"
    );
    Ok(
        json!({"snapshot":graph.snapshot,"source":"conversation-model","instructions":"Read these functions using the current conversation model. Return only {snapshot,source,annotations:[{id,content_hash,title,summary}]}. Never invent or modify edges. Summaries are unconfirmed narrative.","functions":graph.nodes.iter().filter(|n|ids.contains(&n.id)).map(|n|json!({"id":n.id,"content_hash":n.content_hash,"name":n.name,"file":n.file,"line":n.line,"source":n.source,"existing_role":n.role})).collect::<Vec<_>>()}),
    )
}

/// Reuse only validated narratives for unambiguous, unchanged lexical symbols.
/// Identity and code hash must both match; no graph structure or confirmations carry over.
pub fn reuse(previous: &Graph, graph: &mut Graph) -> Result<usize> {
    let Some(notes) = previous
        .viewer
        .as_ref()
        .and_then(|v| v["annotations"].as_object())
    else {
        return Ok(0);
    };
    let mut total = 0;
    // BYOK cache ownership includes model/prompt configuration, known only to the host.
    for source in ["conversation-model", "source-reviewed"] {
        let mut items = vec![];
        for old in &previous.nodes {
            let Some(note) = notes.get(&old.id) else {
                continue;
            };
            if note["source"] != source || note["content_hash"].as_str() != Some(&old.content_hash)
            {
                continue;
            }
            if previous
                .nodes
                .iter()
                .filter(|n| n.symbol_key == old.symbol_key)
                .count()
                != 1
            {
                continue;
            }
            let matches = graph
                .nodes
                .iter()
                .filter(|n| n.symbol_key == old.symbol_key)
                .collect::<Vec<_>>();
            if matches.len() != 1 || matches[0].content_hash != old.content_hash {
                continue;
            }
            let (Some(title), Some(summary)) = (note["title"].as_str(), note["summary"].as_str())
            else {
                continue;
            };
            if title.trim().is_empty()
                || title.chars().count() > 60
                || summary.trim().is_empty()
                || summary.chars().count() > 400
            {
                continue;
            }
            items.push(Annotation {
                id: matches[0].id.clone(),
                content_hash: old.content_hash.clone(),
                title: title.into(),
                summary: summary.into(),
            });
        }
        for batch in items.chunks(200) {
            total += apply(
                graph,
                Envelope {
                    snapshot: graph.snapshot.clone(),
                    source: source.into(),
                    annotations: batch
                        .iter()
                        .map(|n| Annotation {
                            id: n.id.clone(),
                            content_hash: n.content_hash.clone(),
                            title: n.title.clone(),
                            summary: n.summary.clone(),
                        })
                        .collect(),
                },
            )?;
        }
    }
    Ok(total)
}
