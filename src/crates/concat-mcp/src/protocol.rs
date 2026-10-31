// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The Model Context Protocol over a pair of streams: newline-delimited
//! JSON-RPC 2.0, one message a line, as the stdio transport has it.
//!
//! Both eras of the protocol are served, since clients of both are in
//! use. A *legacy* client (protocol 2025-11-25 and earlier) opens with
//! `initialize`, is answered with the version it asked for when this
//! server speaks it, and sends `notifications/initialized`. A *modern*
//! one (2026-07-28) sends no handshake: every request carries its
//! protocol version in `params._meta`, `server/discover` says what this
//! server is, and a version this server does not speak is refused with
//! `UnsupportedProtocolVersion` naming the ones it does. Nothing here is
//! kept per connection either way - the protocol version of a legacy
//! client is only remembered to answer `initialize` - so the two can mix.
//!
//! Requests run one at a time, in order, on the caller's thread; a reader
//! thread takes lines in meanwhile, so `ping` is answered and
//! `notifications/cancelled` is heard while a long call - `export_wait` -
//! is in progress. A cancelled call sends nothing more, as the protocol
//! requires.

use std::collections::HashSet;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};

use crate::backend::Backend;
use crate::tools::{Context, Toolbox};

/// The newest protocol, served without a handshake.
pub const MODERN: &str = "2026-07-28";
/// The handshake-era protocols served, newest first.
pub const LEGACY: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
/// The `_meta` keys a modern request carries.
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_SERVER: &str = "io.modelcontextprotocol/serverInfo";
/// The longest line read: a tool call's arguments are small.
const MAX_LINE: u64 = 16 * 1024 * 1024;
/// How many cancellations of requests not yet begun are remembered.
const EARLY_CANCELS: usize = 1024;

/// JSON-RPC's and MCP's error numbers.
mod code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    pub const UNSUPPORTED_VERSION: i64 = -32022;
}

/// What the agent is told about this server, once, when it connects.
const INSTRUCTIONS: &str = "Concat is a video editor; these tools edit its projects. A project is a folder: open one (project_open) or make one (project_create), then pass its folder as `path` to every other tool. Read timeline_read for the ids of lanes (tracks), clips and media. Times and durations are seconds on the timeline; positions within a clip (keyframes) are fractions from 0 to 1; sizes and offsets of titles and pictures are fractions of the frame. Every edit is one undoable step (edit_undo); edit_apply with a batch command makes several into one. Edits autosave as they happen - the document is written after every applied edit - so work survives without project_save, which now only writes under a new name. To render, export_start then export_wait. preview_frame shows the frame at a time. A refused edit says why in the window's words and changes nothing.
CREATIVE LOOP (use it, do not skip it): start with creative_brief to fix audience, thesis, curiosityGap, payoff and emotionalArc; `emotion` (action extract) on the voiceover's transcript to learn what every line FEELS like - its emotion, act, easing curve, colour temperature, duck depth and stock vocabulary - and let that drive the plan, the B-roll (gate candidates with action gate), the motion easing and the mix; plan_validate the structure; stock_contact_sheet to SEE B-roll before choosing it; motion_frame to SEE graphics before baking; export, then watch_render to SEE the whole cut as a contact sheet; craft_critique for an honest six-axis score and concrete revisions; emotion (action verify) with the re-transcribed export to prove nothing was lost; fix what it names and watch_render again at least once. local_vlm sets up a tiny local vision critic (no system install). A path to a PNG is not a picture you can see - only the inline image content blocks from these tools are.";

/// The server's name and version, as `serverInfo`.
fn server_info() -> Value {
    json!({
        "name": "concat",
        "title": "Concat",
        "version": env!("CARGO_PKG_VERSION"),
    })
}

/// The capabilities this server has: tools, and nothing else.
fn capabilities() -> Value {
    json!({ "tools": { "listChanged": false } })
}

/// Every version this server speaks, newest first.
fn supported() -> Vec<&'static str> {
    std::iter::once(MODERN).chain(LEGACY).collect()
}

/// The one writer of the output stream: a line and a flush, under a lock,
/// so a progress notification from a tool never lands inside a reply.
#[derive(Clone)]
struct Output(Arc<Mutex<Box<dyn Write + Send>>>);

impl Output {
    fn send(&self, message: &Value) {
        if let Ok(mut out) = self.0.lock() {
            // A line, never more: serde_json escapes the newlines a string
            // holds, and the compact form has none of its own.
            let _ = writeln!(out, "{message}");
            let _ = out.flush();
        }
    }
}

/// Which requests were cancelled: the one running, by its flag, and ones
/// not yet begun, by id.
#[derive(Default)]
struct Cancels {
    running: Option<(String, Arc<AtomicBool>)>,
    early: HashSet<String>,
}

/// A request handed from the reader to the worker.
struct Incoming {
    id: Value,
    method: String,
    params: Value,
}

/// How this server is served: the safety net around its own tools.
///
/// Three switches, all off by default so the server an agent has always
/// had is the server it still gets:
///
/// - **Read-only** refuses every tool that changes something, which is
///   the mode a harness or a stranger gets. `motion_reel` keeps its
///   look-before-you-render halves (`plan`, `frame`) and loses only its
///   render.
/// - **The allowlist** names the only tools this server serves: a tool
///   not on the list is refused as if it were not there, though its
///   description still lists it - the server's surface is what the
///   operator said, not what the build happens to carry.
/// - **Per-tool timeouts** answer for a call that outlives its tool's
///   own limit (see [`crate::tools::Tool::timeout_secs`]): the reply
///   goes out at the deadline with the limit in it, and the cancel flag
///   every patient tool already polls is set, so the work itself stops
///   where it can. A sidecar render is bounded a second time, per call,
///   by the Geneva guard.
#[derive(Clone, Debug, Default)]
pub struct Serving {
    /// Refuse every tool that changes something.
    pub read_only: bool,
    /// The only tools served, by name; `None` serves them all.
    pub allowed: Option<Vec<String>>,
}

impl Serving {
    /// The switches from the environment the app's own `concat --mcp`
    /// reads, so a server started by the window has the same safety net
    /// as one started by hand: `CONCAT_MCP_READ_ONLY=1` and
    /// `CONCAT_MCP_ALLOW=a,b,c`.
    pub fn from_environment() -> Serving {
        Serving {
            read_only: std::env::var("CONCAT_MCP_READ_ONLY")
                .is_ok_and(|value| value != "0" && !value.is_empty()),
            allowed: std::env::var("CONCAT_MCP_ALLOW").ok().and_then(|value| {
                let names: Vec<String> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect();
                (!names.is_empty()).then_some(names)
            }),
        }
    }

    /// Whether `tool` may be called with these `arguments`, and the
    /// refusal when it may not.
    fn permits(&self, tool: &crate::tools::Tool, arguments: &Value) -> Result<(), String> {
        if let Some(allowed) = &self.allowed
            && !allowed.iter().any(|name| name == tool.name)
        {
            return Err(format!(
                "{} is not on this server's allowlist; the tools it serves are: {}",
                tool.name,
                allowed.join(", ")
            ));
        }
        if self.read_only && !tool.read_only {
            // The one exception that keeps the creative loop whole: a
            // read-only agent may still LOOK - motion_reel's plan, frame
            // and sheet halves change nothing outside the project's
            // cache, and refusing them takes away the only way to see a
            // design before committing to it.
            let looks = tool.name == "motion_reel"
                && (arguments.get("plan").and_then(Value::as_bool) == Some(true)
                    || arguments.get("frame").and_then(Value::as_f64).is_some()
                    || arguments.get("sheet").and_then(Value::as_bool) == Some(true));
            if !looks {
                return Err(format!(
                    "{} changes the project, and this server is read-only \
                     (CONCAT_MCP_READ_ONLY / --mcp-read-only)",
                    tool.name
                ));
            }
        }
        Ok(())
    }
}

/// Serves MCP on `input` and `output` until `input` ends, with `backend`
/// behind every tool. Returns once the last request has been answered and
/// the backend has finished.
pub fn serve(
    input: impl BufRead + Send + 'static,
    output: impl Write + Send + 'static,
    backend: Box<dyn Backend>,
) {
    serve_with(input, output, backend, Serving::default());
}

/// [`serve`], with the safety net: the serving switches, the per-tool
/// timeouts, and everything else the plain form assumes.
pub fn serve_with(
    input: impl BufRead + Send + 'static,
    output: impl Write + Send + 'static,
    mut backend: Box<dyn Backend>,
    serving: Serving,
) {
    let output = Output(Arc::new(Mutex::new(Box::new(output))));
    let cancels: Arc<Mutex<Cancels>> = Arc::default();
    let (requests, queue) = mpsc::channel::<Incoming>();
    let reader = {
        let output = output.clone();
        let cancels = Arc::clone(&cancels);
        std::thread::Builder::new()
            .name("concat mcp read".to_owned())
            .spawn(move || read(input, &output, &cancels, &requests))
    };
    if reader.is_err() {
        eprintln!("concat mcp: could not start the reader");
        return;
    }
    let toolbox = Toolbox::new();
    let mut server = Server {
        toolbox,
        output: output.clone(),
        cancels,
        serving,
    };
    for incoming in queue {
        server.handle(incoming, backend.as_mut());
    }
    backend.finish();
}

/// Reads lines until the input ends: answers what needs no backend at
/// once, and queues the rest.
fn read(
    input: impl BufRead,
    output: &Output,
    cancels: &Mutex<Cancels>,
    requests: &mpsc::Sender<Incoming>,
) {
    let mut input = input;
    loop {
        let mut bytes = Vec::new();
        match std::io::Read::take(&mut input, MAX_LINE).read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        // A line longer than the cap arrives as a chunk without its
        // newline: answering it as a message would answer the same
        // over-long line many times over, once per chunk, each a parse
        // error about bytes that were never a message at all. It is
        // answered once, named, and the rest of the line is discarded
        // up to the newline that ends it - the session resynchronises
        // instead of drowning.
        if !bytes.ends_with(b"\n") {
            // Discard the rest of the over-long line, up to and including
            // the newline that ends it.
            let mut rest = Vec::new();
            let _ = input.read_until(b'\n', &mut rest);
            output.send(&failure(
                Value::Null,
                code::PARSE,
                &format!(
                    "not JSON: the line is longer than the {} MiB the protocol carries \
                     and was discarded",
                    MAX_LINE / (1024 * 1024)
                ),
                None,
            ));
            continue;
        }
        // JSON is UTF-8. A line that is not is that line's mistake, not the
        // end of the session: `read_line` would fail the read itself and
        // stop the server here without a word.
        let Ok(line) = String::from_utf8(bytes) else {
            output.send(&failure(
                Value::Null,
                code::PARSE,
                "not JSON: the line is not UTF-8",
                None,
            ));
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        let message = match serde_json::from_str::<Value>(&line) {
            Ok(message) => message,
            Err(error) => {
                output.send(&failure(
                    Value::Null,
                    code::PARSE,
                    &format!("not JSON: {error}"),
                    None,
                ));
                continue;
            }
        };
        let Value::Object(mut message) = message else {
            // A batch, or a bare value: neither is a message since
            // 2025-06-18.
            output.send(&failure(
                Value::Null,
                code::INVALID_REQUEST,
                "a message is one JSON-RPC object; batches are not supported",
                None,
            ));
            continue;
        };
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let id = message.remove("id");
        let params = message.remove("params").unwrap_or(Value::Null);
        match (method, id) {
            // A response from the client: this server sends no requests.
            (None, _) => {}
            (Some(method), None) => notification(&method, &params, cancels),
            (Some(_), Some(id)) if !(id.is_string() || id.is_i64() || id.is_u64()) => {
                output.send(&failure(
                    Value::Null,
                    code::INVALID_REQUEST,
                    "a request's id is a string or an integer",
                    None,
                ));
            }
            (Some(method), Some(id)) if method == "ping" => {
                output.send(&success(&id, json!({}), &params));
            }
            (Some(method), Some(id)) => {
                if requests.send(Incoming { id, method, params }).is_err() {
                    return;
                }
            }
        }
    }
}

/// A notification: a cancellation is acted on; the rest need nothing.
fn notification(method: &str, params: &Value, cancels: &Mutex<Cancels>) {
    if method != "notifications/cancelled" {
        return;
    }
    let Some(id) = params.get("requestId") else {
        return;
    };
    let key = id.to_string();
    let Ok(mut cancels) = cancels.lock() else {
        return;
    };
    match &cancels.running {
        Some((running, flag)) if *running == key => flag.store(true, Ordering::Relaxed),
        _ => {
            if cancels.early.len() < EARLY_CANCELS {
                cancels.early.insert(key);
            }
        }
    }
}

/// A result, with the modern era's additions when the request was modern.
fn success(id: &Value, mut result: Value, params: &Value) -> Value {
    if modern_version(params).is_some()
        && let Some(object) = result.as_object_mut()
    {
        object.insert("resultType".to_owned(), json!("complete"));
        let meta = object
            .entry("_meta")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(meta) = meta.as_object_mut() {
            meta.insert(META_SERVER.to_owned(), server_info());
        }
    }
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The caching hints 2026-07-28 requires on these results.
fn cacheable(mut reply: Value) -> Value {
    if let Some(result) = reply.get_mut("result").and_then(Value::as_object_mut)
        && result.get("resultType") == Some(&json!("complete"))
    {
        result.insert("ttlMs".to_owned(), json!(0));
        result.insert("cacheScope".to_owned(), json!("public"));
    }
    reply
}

fn failure(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error })
}

/// The protocol version a request names in its `_meta`, if it names one.
fn modern_version(params: &Value) -> Option<&str> {
    params
        .get("_meta")
        .and_then(|meta| meta.get(META_VERSION))
        .and_then(Value::as_str)
}

struct Server {
    toolbox: Toolbox,
    output: Output,
    cancels: Arc<Mutex<Cancels>>,
    serving: Serving,
}

impl Server {
    fn handle(&mut self, incoming: Incoming, backend: &mut dyn Backend) {
        let Incoming { id, method, params } = incoming;
        let key = id.to_string();
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut cancels) = self.cancels.lock() {
            if cancels.early.remove(&key) {
                return;
            }
            cancels.running = Some((key, Arc::clone(&flag)));
        }
        // The per-tool deadline: a call that outlives its tool's own
        // limit is answered for at the deadline - the reply names the
        // limit, the cancel flag every patient tool already polls is
        // set, and the work stops where it can. The answer runs on the
        // calling thread as it always did; a watcher races it against
        // the deadline and is the one voice that decides the reply. A
        // test scales the table down through CONCAT_MCP_TIMEOUT_SCALE
        // (a fraction of a second per second); production never sets it.
        let tool = params.get("name").and_then(Value::as_str);
        let limit = tool
            .and_then(|name| self.toolbox.find(name).map(|(tool, _)| tool.timeout_secs()))
            .unwrap_or(60);
        let scale = std::env::var("CONCAT_MCP_TIMEOUT_SCALE")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|scale| scale.is_finite() && *scale > 0.0 && *scale <= 1.0)
            .unwrap_or(1.0);
        let deadline = Duration::from_millis((limit as f64 * scale * 1000.0) as u64);
        let (reply_tx, reply_rx) = mpsc::channel::<Value>();
        let output = self.output.clone();
        let watcher = {
            let flag = Arc::clone(&flag);
            let id = id.clone();
            let tool = tool.map(str::to_owned);
            std::thread::spawn(move || match reply_rx.recv_timeout(deadline) {
                Ok(reply) => (reply, false),
                Err(_) => {
                    // The tool did not answer in time: cancel it - the
                    // patient tools poll this flag and stop - and answer
                    // for it NOW, at the deadline, named and bounded.
                    // The late answer, when the call finally gives up,
                    // is dropped on the floor: the agent has been told.
                    flag.store(true, Ordering::Relaxed);
                    let reply = failure(
                        id,
                        code::INTERNAL,
                        &format!(
                            "the {} tool did not answer within its {limit} s limit and was \
                             cancelled",
                            tool.as_deref().unwrap_or("unknown")
                        ),
                        None,
                    );
                    output.send(&reply);
                    (Value::Null, true)
                }
            })
        };
        let reply = self.answer(&id, &method, &params, backend, &flag);
        // The watcher is the one voice: it either took this reply or had
        // already answered for the call at the deadline.
        let _ = reply_tx.send(reply);
        let (reply, timed_out) = watcher.join().unwrap_or((Value::Null, true));
        if let Ok(mut cancels) = self.cancels.lock() {
            cancels.running = None;
        }
        // A client cancellation drops the reply, as the protocol
        // requires; a timeout has already had its own.
        if !timed_out && !flag.load(Ordering::Relaxed) {
            self.output.send(&reply);
        }
    }

    fn answer(
        &mut self,
        id: &Value,
        method: &str,
        params: &Value,
        backend: &mut dyn Backend,
        cancelled: &AtomicBool,
    ) -> Value {
        // A modern request is checked before anything else: its version,
        // then the metadata every modern request must carry.
        if let Some(version) = modern_version(params) {
            if !supported().contains(&version) {
                return failure(
                    id.clone(),
                    code::UNSUPPORTED_VERSION,
                    "Unsupported protocol version",
                    Some(json!({ "supported": supported(), "requested": version })),
                );
            }
            if version == MODERN
                && params
                    .pointer(&format!("/_meta/{}", META_CAPABILITIES.replace('/', "~1")))
                    .is_none()
            {
                return failure(
                    id.clone(),
                    code::INVALID_PARAMS,
                    &format!("a {MODERN} request carries {META_CAPABILITIES} in _meta"),
                    None,
                );
            }
        }
        match method {
            "initialize" => {
                let asked = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                // The version asked for when it is one of the handshake
                // era's this server speaks; the newest of them otherwise,
                // which the client may then decline.
                let version = LEGACY
                    .iter()
                    .find(|known| **known == asked)
                    .copied()
                    .unwrap_or(LEGACY[0]);
                success(
                    id,
                    json!({
                        "protocolVersion": version,
                        "capabilities": capabilities(),
                        "serverInfo": server_info(),
                        "instructions": INSTRUCTIONS,
                    }),
                    params,
                )
            }
            "server/discover" => {
                let mut result = json!({
                    "supportedVersions": supported(),
                    "capabilities": capabilities(),
                    "instructions": INSTRUCTIONS,
                });
                let mut meta = Map::new();
                meta.insert(META_SERVER.to_owned(), server_info());
                result["_meta"] = Value::Object(meta);
                let mut reply = success(id, result, params);
                reply["result"]["resultType"] = json!("complete");
                cacheable(reply)
            }
            "tools/list" => cacheable(success(id, json!({ "tools": self.toolbox.list() }), params)),
            "tools/call" => self.call(id, params, backend, cancelled),
            _ => failure(
                id.clone(),
                code::METHOD_NOT_FOUND,
                &format!("Method not found: {method}"),
                None,
            ),
        }
    }

    fn call(
        &mut self,
        id: &Value,
        params: &Value,
        backend: &mut dyn Backend,
        cancelled: &AtomicBool,
    ) -> Value {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return failure(
                id.clone(),
                code::INVALID_PARAMS,
                "tools/call names a tool",
                None,
            );
        };
        let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
        if !(arguments.is_object() || arguments.is_null()) {
            return failure(
                id.clone(),
                code::INVALID_PARAMS,
                "a tool's arguments are an object",
                None,
            );
        }
        let token = params
            .pointer("/_meta/progressToken")
            .filter(|token| token.is_string() || token.is_i64() || token.is_u64())
            .cloned();
        let output = self.output.clone();
        let report = move |progress: f64, total: f64, message: String| {
            if let Some(token) = &token {
                output.send(&json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/progress",
                    "params": {
                        "progressToken": token,
                        "progress": progress,
                        "total": total,
                        "message": message,
                    },
                }));
            }
        };
        let wants_progress = params.pointer("/_meta/progressToken").is_some();
        let context = Context {
            backend,
            cancelled,
            progress: wants_progress.then_some(&report as &dyn Fn(f64, f64, String)),
        };
        // The safety net first: a tool this server does not serve, or a
        // change a read-only server refuses, is answered before anything
        // runs - with the tool's own name in the message and the list it
        // is not on, when there is one.
        if let Some((tool, _)) = self.toolbox.find(name)
            && let Err(why) = self.serving.permits(tool, &arguments)
        {
            let mut result = json!({ "isError": true, "content": [{
                "type": "text",
                "text": format!("refused: {why}"),
            }]});
            if let Some(object) = result.as_object_mut() {
                object.insert(
                    "structuredContent".to_owned(),
                    json!({ "error": { "message": why } }),
                );
            }
            return success(id, result, params);
        }
        // A panic in a tool is that call's failure, not the server's end.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.toolbox.call(name, arguments, context)
        }));
        match outcome {
            Ok(Some(outcome)) => {
                let mut result = json!({ "content": outcome.content, "isError": outcome.is_error });
                if let Some(structured) = outcome.structured {
                    result["structuredContent"] = structured;
                }
                success(id, result, params)
            }
            Ok(None) => failure(
                id.clone(),
                code::INVALID_PARAMS,
                &format!("Unknown tool: {name}"),
                None,
            ),
            Err(_) => failure(
                id.clone(),
                code::INTERNAL,
                &format!("the {name} tool failed inside the server"),
                None,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_api::{ApiError, Request};
    use std::io::Cursor;

    /// A backend that answers every request with an empty object.
    struct Idle;

    impl Backend for Idle {
        fn call(&mut self, _request: Request) -> Result<Value, ApiError> {
            Ok(json!({}))
        }

        fn describe(&self) -> Value {
            json!({ "mode": "test" })
        }
    }

    /// A writer whose bytes the test can read after `serve` has returned.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Serves `input` to its end and returns every line written.
    fn serve_bytes(input: &[u8]) -> Vec<Value> {
        let written = Shared::default();
        serve(Cursor::new(input.to_vec()), written.clone(), Box::new(Idle));
        let bytes = written.0.lock().expect("lock").clone();
        String::from_utf8(bytes)
            .expect("the server writes UTF-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("each line is JSON"))
            .collect()
    }

    #[test]
    fn a_line_that_is_not_utf8_is_refused_and_the_session_goes_on() {
        let mut input = b"\xff\xfe not text\n".to_vec();
        input.extend_from_slice(br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        input.push(b'\n');
        let replies = serve_bytes(&input);
        assert_eq!(replies.len(), 2, "{replies:?}");
        assert_eq!(replies[0]["error"]["code"], code::PARSE);
        assert_eq!(replies[0]["id"], Value::Null);
        assert_eq!(replies[1]["id"], 1);
        assert!(replies[1]["result"].is_object());
    }

    /// The safety net, driven through the protocol itself: read-only
    /// refusals, one per blocked tool; the allowlist; and the per-tool
    /// deadline, scaled down through CONCAT_MCP_TIMEOUT_SCALE so the
    /// test does not wait a minute.
    fn serve_safety(input: &[u8], serving: Serving) -> Vec<Value> {
        let written = Shared::default();
        serve_with(
            Cursor::new(input.to_vec()),
            written.clone(),
            Box::new(Idle),
            serving,
        );
        let bytes = written.0.lock().expect("lock").clone();
        String::from_utf8(bytes)
            .expect("the server writes UTF-8")
            .lines()
            .map(|line| serde_json::from_str(line).expect("each line is JSON"))
            .collect()
    }

    fn call(name: &str, arguments: Value) -> String {
        format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{{\"name\":\"{name}\",\"arguments\":{}}}}}\n",
            serde_json::to_string(&arguments).unwrap_or_default()
        )
    }

    fn refused(reply: &Value) -> bool {
        reply.pointer("/result/isError") == Some(&json!(true))
            && reply
                .pointer("/result/content/0/text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.starts_with("refused:"))
    }

    #[test]
    fn read_only_refuses_the_tools_that_change_something() {
        let serving = Serving {
            read_only: true,
            allowed: None,
        };
        // One per blocked tool, the ones the net names: an edit, an
        // export, a stock import, and the reel's render half.
        for tool in [
            "clip_add",
            "export_start",
            "openvid_import",
            "project_create",
        ] {
            let replies = serve_safety(call(tool, json!({})).as_bytes(), serving.clone());
            assert_eq!(replies.len(), 1, "{tool}: {replies:?}");
            assert!(refused(&replies[0]), "{tool}: {replies:?}");
            let text = replies[0]
                .pointer("/result/content/0/text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(text.contains(tool), "{text}");
            assert!(text.contains("read-only"), "{text}");
        }
    }

    #[test]
    fn read_only_keeps_the_reads_and_the_reels_look_halves() {
        let serving = Serving {
            read_only: true,
            allowed: None,
        };
        // A read is served: the timeline is the agent's map.
        let replies = serve_safety(
            call("timeline_read", json!({ "path": "/tmp/p" })).as_bytes(),
            serving.clone(),
        );
        assert!(!refused(&replies[0]), "{replies:?}");
        // The reel's look halves are served - plan changes nothing
        // outside the cache - and its render half is refused.
        let plan = serve_safety(
            call(
                "motion_reel",
                json!({ "project": "/tmp/p", "storyboard": { "scenes": [] }, "plan": true }),
            )
            .as_bytes(),
            serving.clone(),
        );
        assert!(!refused(&plan[0]), "plan is a look: {plan:?}");
        let frame = serve_safety(
            call(
                "motion_reel",
                json!({ "project": "/tmp/p", "storyboard": { "scenes": [] }, "frame": 1.0 }),
            )
            .as_bytes(),
            serving.clone(),
        );
        assert!(!refused(&frame[0]), "frame is a look: {frame:?}");
        let render = serve_safety(
            call(
                "motion_reel",
                json!({ "project": "/tmp/p", "storyboard": { "scenes": [] } }),
            )
            .as_bytes(),
            serving.clone(),
        );
        assert!(refused(&render[0]), "the render is a change: {render:?}");
    }

    #[test]
    fn an_allowlist_serves_only_what_it_names() {
        let serving = Serving {
            read_only: false,
            allowed: Some(vec![
                "timeline_read".to_owned(),
                "concat_version".to_owned(),
            ]),
        };
        let served = serve_safety(
            call("timeline_read", json!({ "path": "/tmp/p" })).as_bytes(),
            serving.clone(),
        );
        assert!(!refused(&served[0]), "{served:?}");
        let reply = serve_safety(call("clip_add", json!({})).as_bytes(), serving.clone());
        assert!(refused(&reply[0]), "{reply:?}");
        let text = reply[0]
            .pointer("/result/content/0/text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // The refusal names the tool and the list it is not on.
        assert!(text.contains("clip_add"), "{text}");
        assert!(text.contains("timeline_read"), "{text}");
        assert!(text.contains("allowlist"), "{text}");
    }

    #[test]
    fn the_default_serving_changes_nothing() {
        // No switches: every tool is served as it always was, which is
        // the server an agent has always had.
        let serving = Serving::default();
        assert!(!serving.read_only);
        assert_eq!(serving.allowed, None);
        let replies = serve_safety(call("concat_version", json!({})).as_bytes(), serving);
        assert!(!refused(&replies[0]), "{replies:?}");
    }

    #[test]
    fn the_environment_switches_are_read() {
        // The app's own server reads the same switches from the
        // environment; the test sets them and takes them back.
        // Edition 2024 makes the environment's mutators unsafe: the
        // tests are single-threaded through the runner's own guarantees
        // around these lines, which is what the unsafe blocks say.
        unsafe {
            std::env::set_var("CONCAT_MCP_READ_ONLY", "1");
            std::env::set_var("CONCAT_MCP_ALLOW", "timeline_read");
        }
        let serving = Serving::from_environment();
        unsafe {
            std::env::remove_var("CONCAT_MCP_READ_ONLY");
            std::env::remove_var("CONCAT_MCP_ALLOW");
        }
        assert!(serving.read_only);
        assert_eq!(serving.allowed, Some(vec!["timeline_read".to_owned()]));
    }

    #[test]
    fn a_tool_that_outlives_its_limit_is_answered_for_at_the_deadline() {
        // A hanging tool under a scaled-down deadline: the reply is the
        // timeout's, named and bounded, not the 900-second client
        // timeout's silence. The hang is a backend whose call for
        // timeline_read spins until cancelled - the shape of every
        // patient tool.
        struct Hangs;

        impl Backend for Hangs {
            fn call(&mut self, _request: Request) -> Result<Value, ApiError> {
                // A tool that honours nothing and stops for nothing:
                // five seconds is a hundred times the scaled deadline,
                // so the reply the agent reads can only be the
                // watcher's. The session itself keeps going - the reply
                // is not the return - which is the whole design.
                std::thread::sleep(Duration::from_secs(5));
                Ok(json!({}))
            }

            fn describe(&self) -> Value {
                json!({ "mode": "hangs" })
            }
        }

        unsafe {
            std::env::set_var("CONCAT_MCP_TIMEOUT_SCALE", "0.001");
        }
        let written = Shared::default();
        let started = std::time::Instant::now();
        serve_with(
            Cursor::new(call("timeline_read", json!({ "path": "/tmp/p" })).into_bytes()),
            written.clone(),
            Box::new(Hangs),
            Serving::default(),
        );
        unsafe {
            std::env::remove_var("CONCAT_MCP_TIMEOUT_SCALE");
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_secs(30),
            "the session did not go on past the hung call: {elapsed:?}"
        );
        let bytes = written.0.lock().expect("lock").clone();
        let reply: Value = serde_json::from_str(
            String::from_utf8(bytes)
                .expect("UTF-8")
                .lines()
                .next()
                .expect("a reply"),
        )
        .expect("JSON");
        let message = reply
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(
            message.contains("timeline_read") && message.contains("limit"),
            "{message}"
        );
    }

    #[test]
    fn the_timeout_table_gives_each_tool_its_own_limit() {
        use crate::tools::catalogue;
        let limit = |name: &str| {
            catalogue()
                .into_iter()
                .find(|tool| tool.name == name)
                .unwrap_or_else(|| panic!("no tool {name}"))
                .timeout_secs()
        };
        assert_eq!(limit("transcribe"), 900);
        assert_eq!(limit("analysis_wait"), 900);
        assert_eq!(limit("export_start"), 600);
        assert_eq!(limit("export_wait"), 600);
        assert_eq!(limit("motion_reel"), 300);
        assert_eq!(limit("motion_bake"), 300);
        assert_eq!(limit("watch_render"), 300);
        assert_eq!(limit("timeline_read"), 60);
        assert_eq!(limit("clip_add"), 60);
    }
}
