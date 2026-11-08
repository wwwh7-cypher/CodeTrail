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
    #[command(
        about = "扫描仓库，生成代码图与静态导览",
        long_about = "扫描仓库中的 TypeScript/JavaScript/Svelte/Python/Go/C/C++ 文件，构建函数调用图，并输出可离线打开的 HTML 导览与 JSON 图。"
    )]
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
    #[command(
        about = "按 Git 变更生成审查导览",
        long_about = "比较 Git 基线到目标提交的变更，标注受影响的函数、调用边与审查疑点，输出审查用 HTML、JSON 与可选报告。"
    )]
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
    #[command(
        about = "查询已生成的代码图",
        long_about = "在 JSON 代码图上执行路径、影响面、搜索、邻居与模块查询，结果以 JSON 输出。"
    )]
    Query {
        graph: PathBuf,
        #[command(subcommand)]
        operation: Query,
    },
    #[command(
        about = "由 JSON 图重新渲染 HTML 导览",
        long_about = "读取已有 JSON 代码图，重新渲染为自包含的离线 HTML 导览。"
    )]
    Render {
        graph: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    #[command(
        about = "管理有界稳定记忆",
        long_about = "管理经确认的稳定记忆：查看状态、导出上下文、保存与确认、标记过期、清理和检索。"
    )]
    Memory {
        #[arg(long)]
        graph: PathBuf,
        #[arg(long)]
        store: PathBuf,
        #[command(subcommand)]
        operation: Memory,
    },
    #[command(
        about = "启动 MCP stdio 服务",
        long_about = "以 stdio 方式提供只读 MCP 工具，供编辑器或其他客户端查询代码图与稳定记忆。"
    )]
    Mcp {
        #[arg(long)]
        graph: PathBuf,
        #[arg(long)]
        store: PathBuf,
    },
    #[command(
        about = "统计文本的 token 数",
        long_about = "用 cl100k_base 分词器统计文件的 token 数，并输出字节数与上限。"
    )]
    Tokens {
        input: PathBuf,
    },
    #[command(
        about = "导出内置查看器模板",
        long_about = "将内置的 HTML 查看器模板导出到指定路径，便于自定义后复用。"
    )]
    Template {
        #[arg(long)]
        out: PathBuf,
    },
    #[command(
        about = "生成函数标注模板",
        long_about = "为指定函数（未指定时为全部相关函数）生成待填写的标注模板 JSON。"
    )]
    PrepareAnnotations {
        graph: PathBuf,
        #[arg(long)]
        function: Vec<String>,
    },
    #[command(
        about = "应用标注并写回代码图",
        long_about = "校验并应用人工标注 JSON，输出更新后的代码图。"
    )]
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
    #[command(
        about = "查找两个符号间的最短静态调用路径",
        long_about = "在代码图中从起点到终点查找最短的已解析静态调用路径。"
    )]
    Path {
        from: String,
        to: String,
    },
    #[command(
        about = "列出该函数可能影响的上游入口",
        long_about = "反向遍历调用边，列出可能受该函数影响的上游函数与入口；这是静态可达分析，不代表运行时证明。"
    )]
    Impact {
        function: String,
    },
    #[command(
        about = "按名称查找符号",
        long_about = "按名称或关键词在代码图中查找符号及其文件与行号。"
    )]
    Find {
        text: String,
    },
    #[command(
        about = "检索代码中的能力声明",
        long_about = "检索源码注释中的能力声明（claim），用于审查证据对照。"
    )]
    Claims {
        text: String,
    },
    #[command(
        about = "模糊搜索符号",
        long_about = "对符号名、职责说明等字段做模糊与子序列搜索，并分页返回结果。"
    )]
    Search {
        text: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    #[command(
        about = "列出函数的直接邻居",
        long_about = "列出与该函数直接相连的调用者与被调用者，支持分页。"
    )]
    Neighbors {
        function: String,
        #[arg(long, default_value_t = 0)]
        offset: usize,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    #[command(
        about = "列出模块级节点",
        long_about = "列出各文件的模块顶层节点及其统计信息。"
    )]
    Modules,
}
#[derive(Subcommand)]
enum Memory {
    #[command(
        about = "查看记忆库状态",
        long_about = "查看稳定记忆、待确认与过期条目的数量与状态。"
    )]
    Status,
    #[command(
        about = "导出稳定记忆上下文",
        long_about = "按 token 上限导出已确认的稳定记忆上下文。"
    )]
    Context,
    #[command(
        about = "保存稳定记忆",
        long_about = "保存经确认的稳定记忆条目。"
    )]
    SaveStable {
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        confirmed: bool,
    },
    #[command(
        about = "确认审查结论",
        long_about = "为函数确认审查结论并写入稳定记忆。"
    )]
    ConfirmReview {
        #[arg(long)]
        function: Vec<String>,
        #[arg(long)]
        conclusion: PathBuf,
        #[arg(long)]
        confirmed: bool,
    },
    #[command(
        about = "标记记忆已过期",
        long_about = "将指定记忆标记为过期，使其不再进入默认上下文。"
    )]
    MarkStale {
        id: String,
        #[arg(long)]
        confirmed: bool,
    },
    #[command(
        about = "清理过期记忆",
        long_about = "清理过期与无效的记忆条目。"
    )]
    Prune {
        #[arg(long)]
        confirmed: bool,
    },
    #[command(
        about = "检索记忆",
        long_about = "按文本检索记忆条目，可选择是否包含过期条目。"
    )]
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
