use crate::{
    graph::Graph,
    memory::{Store, query_graph, read_graph},
};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
};

const VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2024-11-05"];
fn tools() -> Value {
    json!({"tools":[
        {"name":"code_trail_find","description":"Find analyzed symbols in this fixed repository snapshot (max 100).","inputSchema":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}},
        {"name":"code_trail_search","description":"Fuzzy search identifiers, paths and validated narratives (max 200).","inputSchema":{"type":"object","properties":{"text":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["text"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}},
        {"name":"code_trail_neighbors","description":"Resolved first-order callers and callees with paginated symbols and call-site evidence.","inputSchema":{"type":"object","properties":{"function":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200}},"required":["function"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}},
        {"name":"code_trail_modules","description":"Aggregate actual package/directory modules and their evidenced calls.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}},
        {"name":"code_trail_path","description":"Shortest resolved static path; no path does not prove runtime unreachability.","inputSchema":{"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"}},"required":["from","to"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}},
        {"name":"code_trail_impact","description":"Potential static upstream reach for one function; not runtime impact proof.","inputSchema":{"type":"object","properties":{"function":{"type":"string"}},"required":["function"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}},
        {"name":"code_trail_memory","description":"Query only human-confirmed review records; stale records excluded by default.","inputSchema":{"type":"object","properties":{"text":{"type":"string"},"include_stale":{"type":"boolean"}},"required":["text"],"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}},
        {"name":"code_trail_context","description":"Load the bounded confirmed stable Markdown only if its source snapshot is current and unedited.","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false}}
    ]})
}

pub struct Server {
    graph_path: PathBuf,
    store: Store,
    initialized: bool,
    ready: bool,
}
impl Server {
    pub fn new(graph_path: &Path, store_path: &Path) -> Result<Self> {
        let graph_path = graph_path.canonicalize()?;
        let _: Graph = read_graph(&graph_path)?;
        Ok(Self {
            graph_path,
            store: Store::open(store_path)?,
            initialized: false,
            ready: false,
        })
    }
    pub fn dispatch(&mut self, request: Value) -> Option<Value> {
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        if id.is_none() {
            if method == "notifications/initialized" && self.initialized {
                self.ready = true
            }
            return None;
        }
        let id = id.unwrap();
        if request["jsonrpc"] != "2.0" || !(id.is_string() || id.is_number() || id.is_null()) {
            return Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}}),
            );
        }
        let error = |code, message: &str| json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}});
        if method == "initialize" {
            if self.initialized {
                return Some(error(-32600, "Already initialized"));
            }
            let requested = request["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = if VERSIONS.contains(&requested) {
                requested
            } else {
                VERSIONS[0]
            };
            self.initialized = true;
            return Some(
                json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":version,"capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"code-trail","version":env!("CARGO_PKG_VERSION")},"instructions":"Read-only, fixed-snapshot static code analysis. No write or LLM tools. Stable context requires current human confirmation."}}),
            );
        }
        if method == "ping" {
            return Some(json!({"jsonrpc":"2.0","id":id,"result":{}}));
        }
        if !self.ready {
            return Some(error(
                -32002,
                "Initialize and send notifications/initialized before using tools",
            ));
        }
        if method == "tools/list" {
            return Some(json!({"jsonrpc":"2.0","id":id,"result":tools()}));
        }
        if method != "tools/call" {
            return Some(error(-32601, "Method not found"));
        }
        let name = request["params"]["name"].as_str().unwrap_or("");
        let args = request["params"]
            .get("arguments")
            .cloned()
            .unwrap_or(json!({}));
        if !args.is_object() {
            return Some(error(-32602, "arguments must be an object"));
        }
        let result = (|| -> Result<Value> {
            let graph = read_graph(&self.graph_path)?;
            match name {
                "code_trail_find" => query_graph(&graph, "find", &args),
                "code_trail_search" => {
                    let text = args["text"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("text must be a string"))?;
                    Ok(
                        json!({"hits":crate::index::search(&graph, text, args["limit"].as_u64().unwrap_or(50) as usize)}),
                    )
                }
                "code_trail_neighbors" => {
                    let function = args["function"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("function must be a string"))?;
                    crate::index::neighbors(
                        &graph,
                        function,
                        args["offset"].as_u64().unwrap_or(0) as usize,
                        args["limit"].as_u64().unwrap_or(100) as usize,
                    )
                }
                "code_trail_modules" => Ok(crate::index::modules(&graph)),
                "code_trail_path" => query_graph(&graph, "path", &args),
                "code_trail_impact" => query_graph(&graph, "impact", &args),
                "code_trail_memory" => {
                    ensure!(args["text"].is_string(), "text must be a string");
                    self.store.query(
                        &graph,
                        args["text"].as_str().unwrap(),
                        args["include_stale"].as_bool().unwrap_or(false),
                    )
                }
                "code_trail_context" => self.store.stable_context(&graph),
                _ => anyhow::bail!("Unknown tool: {name}"),
            }
        })();
        Some(match result {
            Ok(value) => {
                json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":serde_json::to_string(&value).unwrap()}],"structuredContent":{"data":value},"isError":false}})
            }
            Err(e) => {
                json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":e.to_string()}],"isError":true}})
            }
        })
    }
}

pub fn serve(graph: &Path, store: &Path) -> Result<()> {
    let mut server = Server::new(graph, store)?;
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut output = io::stdout().lock();
    loop {
        let mut bytes = Vec::new();
        let read = input
            .by_ref()
            .take(1_048_578)
            .read_until(b'\n', &mut bytes)?;
        if read == 0 {
            break;
        }
        if bytes.len() > 1_048_576 {
            anyhow::bail!("MCP message exceeds 1 MiB")
        }
        let response = match serde_json::from_slice(&bytes) {
            Ok(value) => server.dispatch(value),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}),
            ),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}
