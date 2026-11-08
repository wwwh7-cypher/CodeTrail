use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use code_trail::{
    analysis,
    graph::Graph,
    mcp,
    memory::{self, Store},
    render, review, write_atomic,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Parser)]
#[command(version, about = "CodeTrail · Rust 静态架构导览、Git 审查与有界记忆")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Scan {
        repo: PathBuf,
        #[arg(short, long, default_value = "code-trail.html")]
        out: PathBuf,
        #[arg(long)]
        json: Option<PathBuf>,
        #[arg(long)]
        entry: Vec<String>,
        #[arg(long)]
        changed: Vec<String>,
        #[arg(long)]
        claims: Option<PathBuf>,
        #[arg(long)]
        cache: Option<PathBuf>,
    },
    Review {
        repo: PathBuf,
        #[arg(long, default_value = "HEAD")]
        base: String,
        #[arg(long)]
        head: Option<String>,
        #[arg(long)]
        claims: Option<PathBuf>,
        #[arg(long)]
        entry: Vec<String>,
        #[arg(short, long, default_value = "code-trail-review.html")]
        out: PathBuf,
        #[arg(long)]
        json: Option<PathBuf>,
        #[arg(long)]
        report: Option<PathBuf>,
    },
    Query {
        graph: PathBuf,
        #[command(subcommand)]
        operation: Query,
    },
    Render {
        graph: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    Memory {
        #[arg(long)]
        graph: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[command(subcommand)]
        operation: Memory,
    },
    Mcp {
        #[arg(long)]
        graph: PathBuf,
        #[arg(long)]
        store: PathBuf,
    },
    Tokens {
        input: PathBuf,
    },
    Template {
        #[arg(long)]
        out: PathBuf,
    },
    PrepareAnnotations {
        graph: PathBuf,
        #[arg(long)]
        function: Vec<String>,
    },
    Annotate {
        graph: PathBuf,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}
#[derive(Subcommand)]
enum Query {
    Path {
        from: String,
        to: String,
    },
    Impact {
        function: String,
    },
    Find {
        text: String,
    },
    Claims {
        text: String,
    },
    Search {
        text: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    Neighbors {
        function: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Modules,
}
#[derive(Subcommand)]
enum Memory {
    Status,
    Context,
    SaveStable {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        confirmed: bool,
    },
    ConfirmReview {
        #[arg(long)]
        function: Vec<String>,
        #[arg(long)]
        conclusion: PathBuf,
        #[arg(long)]
        confirmed: bool,
    },
    MarkStale {
        id: String,
        #[arg(long)]
        confirmed: bool,
    },
    Prune {
        #[arg(long)]
        confirmed: bool,
    },
    Query {
        text: String,
        #[arg(long)]
        include_stale: bool,
    },
}
fn print(value: impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}
fn graph_out(graph: &mut Graph, out: &Path, json: Option<PathBuf>) -> Result<()> {
    let json = json.unwrap_or_else(|| out.with_extension("graph.json"));
    ensure!(json != out, "HTML and JSON output paths must differ");
    if let Ok(previous) = memory::read_graph(&json) {
        code_trail::annotations::reuse(&previous, graph)?;
    }
    write_atomic(&json, serde_json::to_vec_pretty(graph)?)?;
    write_atomic(out, render(graph)?)?;
    print(
        serde_json::json!({"status":"ok","html":out.canonicalize()?.display().to_string(),"graph":json.canonicalize()?.display().to_string(),"snapshot":graph.snapshot,"files":graph.files,"functions":graph.nodes.iter().filter(|n|n.kind!="module").count(),"edges":graph.edges.len(),"unresolved_calls":graph.unresolved.len(),"diagnostics":graph.diagnostics.len(),"analysis_ms":graph.analysis_ms,"changed_functions":graph.changed.len(),"git_changes":graph.review.as_ref().map(|r|r.changes.len())}),
    )
}
fn root(path: &Path) -> Result<PathBuf> {
    let p = path
        .canonicalize()
        .context("Repository directory does not exist")?;
    ensure!(p.is_dir(), "Repository must be a directory");
    Ok(p)
}
fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Scan {
            repo,
            out,
            json,
            entry,
            changed,
            claims,
            cache,
        } => {
            let start = Instant::now();
            let mut graph = analysis::scan_cached(&root(&repo)?, cache.as_deref())?;
            for name in entry {
                let id = graph.resolve(&name)?;
                if !graph.entries.contains(&id) {
                    graph.entries.push(id)
                }
            }
            graph.explicit_entries = !graph.entries.is_empty();
            if graph.entries.is_empty() {
                graph.entries = graph.candidate_entries()
            }
            for name in changed {
                let id = graph.resolve(&name)?;
                if !graph.changed.contains(&id) {
                    graph.changed.push(id)
                }
            }
            graph.claim = claims
                .map(fs::read_to_string)
                .transpose()?
                .unwrap_or_default();
            graph.generated_unix = memory::now();
            graph.analysis_ms = start.elapsed().as_millis() as u64;
            graph_out(&mut graph, &out, json)
        }
        Command::Review {
            repo,
            base,
            head,
            claims,
            entry,
            out,
            json,
            report,
        } => {
            let start = Instant::now();
            let mut graph = review::git_review(
                &root(&repo)?,
                &base,
                head.as_deref(),
                claims
                    .map(fs::read_to_string)
                    .transpose()?
                    .unwrap_or_default(),
                &entry,
            )?;
            graph.generated_unix = memory::now();
            graph.analysis_ms = start.elapsed().as_millis() as u64;
            if let Some(path) = report {
                write_atomic(&path, review::report_markdown(&graph)?)?
            }
            graph_out(&mut graph, &out, json)
        }
        Command::Render { graph, out } => {
            write_atomic(&out, render(&memory::read_graph(&graph)?)?)?;
            print(serde_json::json!({"status":"ok","html":out}))
        }
        Command::Query { graph, operation } => {
            let graph = memory::read_graph(&graph)?;
            match operation {
                Query::Path { from, to } => {
                    let (a, b) = (graph.resolve(&from)?, graph.resolve(&to)?);
                    print(
                        serde_json::json!({"from":a,"to":b,"path":graph.path(&a,&b),"basis":"resolved-static-calls"}),
                    )
                }
                Query::Impact { function } => {
                    let id = graph.resolve(&function)?;
                    let affected = graph.upstream(&id);
                    print(
                        serde_json::json!({"function":id,"upstream_including_self":affected,"reached_entries":graph.entries.iter().filter(|e|affected.contains(e.as_str())).collect::<Vec<_>>(),"basis":"potential-static-reach-not-runtime-proof"}),
                    )
                }
                Query::Find { text } => print(memory::query_graph(
                    &graph,
                    "find",
                    &serde_json::json!({"text":text}),
                )?),
                Query::Claims { text } => print(memory::query_graph(
                    &graph,
                    "claims",
                    &serde_json::json!({"text":text}),
                )?),
                Query::Search { text, limit } => {
                    print(code_trail::index::search(&graph, &text, limit))
                }
                Query::Neighbors {
                    function,
                    offset,
                    limit,
                } => print(code_trail::index::neighbors(
                    &graph, &function, offset, limit,
                )?),
                Query::Modules => print(code_trail::index::modules(&graph)),
            }
        }
        Command::Memory {
            graph,
            store,
            operation,
        } => {
            let graph = memory::read_graph(&graph)?;
            let store = Store::open(&store)?;
            match operation {
                Memory::Status => print(store.status(&graph)?),
                Memory::Context => print(store.stable_context(&graph)?),
                Memory::SaveStable { input, confirmed } => {
                    print(store.save_stable(&graph, &fs::read_to_string(input)?, confirmed)?)
                }
                Memory::ConfirmReview {
                    function,
                    conclusion,
                    confirmed,
                } => print(store.confirm_review(
                    &graph,
                    &function,
                    &fs::read_to_string(conclusion)?,
                    confirmed,
                )?),
                Memory::MarkStale { id, confirmed } => print(store.mark_stale(&id, confirmed)?),
                Memory::Prune { confirmed } => print(store.prune(&graph, confirmed)?),
                Memory::Query {
                    text,
                    include_stale,
                } => print(store.query(&graph, &text, include_stale)?),
            }
        }
        Command::Mcp { graph, store } => mcp::serve(&graph, &store),
        Command::Template { out } => {
            write_atomic(&out, code_trail::viewer_template())?;
            print(serde_json::json!({"template":out}))
        }
        Command::PrepareAnnotations { graph, function } => print(code_trail::annotations::prepare(
            &memory::read_graph(&graph)?,
            &function,
        )?),
        Command::Annotate { graph, input, out } => {
            let mut graph = memory::read_graph(&graph)?;
            let count = code_trail::annotations::apply(
                &mut graph,
                serde_json::from_slice(&fs::read(input)?)?,
            )?;
            write_atomic(&out, serde_json::to_vec_pretty(&graph)?)?;
            print(serde_json::json!({"annotations":count,"snapshot":graph.snapshot,"graph":out}))
        }
        Command::Tokens { input } => {
            let text = fs::read_to_string(input)?;
            print(
                serde_json::json!({"tokens":memory::tokens(&text)?,"limit":memory::TOKEN_LIMIT,"tokenizer":"cl100k_base","bytes":text.len()}),
            )
        }
    }
}
fn main() {
    if let Err(error) = run() {
        eprintln!("code-trail: {error:#}");
        std::process::exit(1)
    }
}
