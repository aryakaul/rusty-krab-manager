//! The pluggable LLM "manager" backend.
//!
//! `ManagerBackend` is the trait the TUI talks to. `ClaudeBackend` speaks the
//! Anthropic Messages API and `OpenAiBackend` speaks any OpenAI-compatible
//! `/chat/completions` server (Ollama, llama.cpp, LM Studio, …) — both over raw
//! HTTP (Rust has no official SDK). Each only implements the transport (the
//! `ToolLoop` trait); the three flows — `manage` (edit a task's
//! subtasks/status/fields), `capture` (free text → new tasks), and `chat` (a
//! multi-turn planning conversation) — are written once against `ToolLoop`
//! (`run_manage`/`run_capture`/`run_chat`). `NoneBackend` is the offline no-op.
//! API keys come from the environment ($ANTHROPIC_API_KEY / $OPENAI_API_KEY),
//! never config. The LLM never chooses which task to work on — that stays
//! weighted-random.

use crate::task::{find_tag, parse_due, parse_recur, Status, Task, DUE_FORMAT};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::env;
use std::fmt::Write as _;

// The Anthropic Messages API. Rust has no official SDK, so the backend speaks
// raw HTTP (per the claude-api guidance for unsupported-SDK languages).
const URL: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
// Bound the tool-use loop so a misbehaving model can't spin forever.
const MAX_TURNS: usize = 8;
const MAX_TOKENS: u32 = 2048;

// The result of running the manager over a single task.
#[derive(Debug)]
pub struct ManagerOutcome {
    // The (possibly mutated) task to write back into the store.
    pub task: Task,
    // The assistant's closing text, for display in the TUI.
    pub reply: String,
    // A human-readable line per mutation actually applied (e.g.
    // "added subtask 'write tests'"), shown for review before persisting.
    // Empty means the run only produced a text reply.
    pub changes: Vec<String>,
}

// The running message history of a planning conversation, in the active
// backend's wire format. Opaque outside this module so the wire shape never
// leaks into the UI.
#[derive(Debug, Default, Clone)]
pub struct ChatHistory(Vec<Value>);

impl ChatHistory {
    // Seed a history from one completed plain-text exchange (e.g. continuing
    // a manage/capture reply into the fullscreen chat). A plain role/content
    // pair is the one shape every wire format accepts.
    pub fn from_exchange(user: &str, reply: &str) -> Self {
        Self(vec![
            json!({ "role": "user", "content": user }),
            json!({ "role": "assistant", "content": reply }),
        ])
    }
}

// The result of one turn of a planning conversation.
#[derive(Debug)]
pub struct ChatOutcome {
    // The full updated message history, to feed back into the next turn.
    pub messages: ChatHistory,
    // The assistant's reply text for this turn.
    pub reply: String,
    // Any tasks captured during this turn (added live during planning).
    pub captured: Vec<Task>,
}

// The result of an NL-capture run: proposed new tasks awaiting review.
#[derive(Debug)]
pub struct CaptureOutcome {
    // Tasks parsed from the user's free-form text, not yet added to the store.
    pub tasks: Vec<Task>,
    // The assistant's closing text, for display in the TUI.
    pub reply: String,
    // Adjustments made while building tasks (e.g. an unknown tag remapped to a
    // fallback), surfaced in the review.
    pub notes: Vec<String>,
}

#[derive(Debug)]
pub enum ManagerError {
    Disabled,
    MissingApiKey,
    Http(String),
    Api { status: u16, body: String },
    Protocol(String),
}

impl std::fmt::Display for ManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManagerError::Disabled => write!(f, "manager is disabled"),
            ManagerError::MissingApiKey => {
                write!(f, "ANTHROPIC_API_KEY is not set")
            }
            ManagerError::Http(e) => write!(f, "http error: {e}"),
            ManagerError::Api { status, body } => {
                write!(f, "api error {status}: {body}")
            }
            ManagerError::Protocol(e) => write!(f, "protocol error: {e}"),
        }
    }
}

impl std::error::Error for ManagerError {}

// Pluggable manager backend. The LLM manages a task's *subtasks/status/notes*
// only — never task selection, which stays weighted-random.
#[async_trait]
pub trait ManagerBackend: Send + Sync {
    // Manage a single task's subtasks/status/notes and its tag/name/due. `tags`
    // is the configured tag set; the tag can only be changed to one of these.
    async fn manage(
        &self,
        task: &Task,
        prompt: &str,
        tags: &[String],
    ) -> Result<ManagerOutcome, ManagerError>;

    // Turn free-form text into one or more proposed new tasks, constraining
    // tags to the supplied (configured) set.
    async fn capture(&self, prompt: &str, tags: &[String]) -> Result<CaptureOutcome, ManagerError>;

    // Continue a planning conversation: append `user` to `history`, run the
    // tool loop, and return the updated history, reply, and any captured tasks.
    async fn chat(
        &self,
        history: ChatHistory,
        user: &str,
        tags: &[String],
    ) -> Result<ChatOutcome, ManagerError>;
}

// Offline no-op backend (classic mode / manager disabled).
pub struct NoneBackend;

#[async_trait]
impl ManagerBackend for NoneBackend {
    async fn manage(
        &self,
        _task: &Task,
        _prompt: &str,
        _tags: &[String],
    ) -> Result<ManagerOutcome, ManagerError> {
        Err(ManagerError::Disabled)
    }

    async fn capture(
        &self,
        _prompt: &str,
        _tags: &[String],
    ) -> Result<CaptureOutcome, ManagerError> {
        Err(ManagerError::Disabled)
    }

    async fn chat(
        &self,
        _history: ChatHistory,
        _user: &str,
        _tags: &[String],
    ) -> Result<ChatOutcome, ManagerError> {
        Err(ManagerError::Disabled)
    }
}

// The transport each concrete backend implements: one tool-use turn loop over
// its own wire format. The manage/capture/chat orchestration below is written
// once against this trait, so `ClaudeBackend` and `OpenAiBackend` only differ
// in `run_tool_loop` (and their inherent `send_turn`).
#[async_trait]
trait ToolLoop: Send + Sync {
    // Send the running history, accumulate assistant text into the returned
    // reply, and hand each tool call to `dispatch` (which returns the
    // tool_result text and whether it errored). Returns the final message
    // history and the accumulated reply.
    async fn run_tool_loop(
        &self,
        messages: Vec<Value>,
        system: &str,
        tools: &Value,
        dispatch: &mut (dyn for<'a, 'b> FnMut(&'a str, &'b Value) -> (String, bool) + Send),
    ) -> Result<(Vec<Value>, String), ManagerError>;
}

// Manage a single task: run the manage tools over a clone, collecting the
// changes actually applied for review. Shared by every backend.
async fn run_manage<B: ToolLoop + ?Sized>(
    be: &B,
    task: &Task,
    prompt: &str,
    tags: &[String],
) -> Result<ManagerOutcome, ManagerError> {
    let mut working = task.clone();
    let mut changes: Vec<String> = Vec::new();

    let first = format!(
        "{}Configured tags (for the tag field): {}\n\n{}",
        task_context(task),
        tags.join(", "),
        prompt
    );
    let messages = vec![json!({ "role": "user", "content": first })];
    let tools = tool_specs(tags);

    let mut dispatch = |name: &str, input: &Value| {
        let (result, did) = apply_tool(&mut working, name, input, tags);
        if did {
            changes.push(result.clone());
        }
        let is_error = !did && result.starts_with("error");
        (result, is_error)
    };
    let (_messages, reply) = be
        .run_tool_loop(messages, SYSTEM_PROMPT, &tools, &mut dispatch)
        .await?;

    Ok(ManagerOutcome {
        task: working,
        reply,
        changes,
    })
}

// Run the add_task tool loop over `messages`; shared by capture and chat.
async fn run_capture_loop<B: ToolLoop + ?Sized>(
    be: &B,
    messages: Vec<Value>,
    system: &str,
    tags: &[String],
) -> Result<LoopResult, ManagerError> {
    let mut tasks: Vec<Task> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let tools = capture_tool_specs();

    let mut dispatch = |name: &str, input: &Value| {
        if name != "add_task" {
            return (format!("error: unknown tool '{name}'"), true);
        }
        match build_task(input, tags) {
            Some((task, note)) => {
                let msg = format!("captured task '{}'", task.name);
                if let Some(n) = note {
                    notes.push(n);
                }
                tasks.push(task);
                (msg, false)
            }
            None => ("error: task name was empty".to_string(), true),
        }
    };
    let (messages, reply) = be
        .run_tool_loop(messages, system, &tools, &mut dispatch)
        .await?;

    Ok(LoopResult {
        messages,
        reply,
        tasks,
        notes,
    })
}

// Turn free-form text into proposed new tasks.
async fn run_capture<B: ToolLoop + ?Sized>(
    be: &B,
    prompt: &str,
    tags: &[String],
) -> Result<CaptureOutcome, ManagerError> {
    let first = format!(
        "Available tags (use ONLY one of these per task): {}\n\n\
         Capture the following as one or more tasks:\n{}",
        tags.join(", "),
        prompt
    );
    let messages = vec![json!({ "role": "user", "content": first })];
    let r = run_capture_loop(be, messages, CAPTURE_SYSTEM_PROMPT, tags).await?;
    Ok(CaptureOutcome {
        tasks: r.tasks,
        reply: r.reply,
        notes: r.notes,
    })
}

// Continue a planning conversation (appends `user` and runs the capture tools).
async fn run_chat<B: ToolLoop + ?Sized>(
    be: &B,
    history: ChatHistory,
    user: &str,
    tags: &[String],
) -> Result<ChatOutcome, ManagerError> {
    let mut messages = history.0;
    messages.push(json!({ "role": "user", "content": user }));
    let r = run_capture_loop(be, messages, PLAN_SYSTEM_PROMPT, tags).await?;
    Ok(ChatOutcome {
        messages: ChatHistory(r.messages),
        reply: r.reply,
        captured: r.tasks,
    })
}

// Send a JSON API request and parse the JSON response, mapping transport
// failures and non-2xx statuses onto ManagerError. Shared by both backends.
async fn post_json(req: reqwest::RequestBuilder) -> Result<Value, ManagerError> {
    let resp = req
        .send()
        .await
        .map_err(|e| ManagerError::Http(e.to_string()))?;
    let status = resp.status();
    let val: Value = resp
        .json()
        .await
        .map_err(|e| ManagerError::Http(e.to_string()))?;
    if !status.is_success() {
        return Err(ManagerError::Api {
            status: status.as_u16(),
            body: val.to_string(),
        });
    }
    Ok(val)
}

// Accumulate one assistant text fragment into the running reply,
// newline-separated. Shared by both tool loops.
fn append_reply(reply: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !reply.is_empty() {
        reply.push('\n');
    }
    reply.push_str(text);
}

// Anthropic Claude backend. The API key is read from the environment, never
// the config file, to keep secrets out of the repo.
pub struct ClaudeBackend {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

impl ClaudeBackend {
    pub fn new(model: String) -> Result<Self, ManagerError> {
        let api_key = env::var("ANTHROPIC_API_KEY").map_err(|_| ManagerError::MissingApiKey)?;
        Ok(Self {
            client: reqwest::Client::new(),
            api_key,
            model,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    // One request/response turn against the Messages API. Returns the response
    // content blocks and the stop reason.
    async fn send_turn(&self, body: &Value) -> Result<(Vec<Value>, String), ManagerError> {
        let req = self
            .client
            .post(URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(body);
        let val = post_json(req).await?;
        let content = val
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| ManagerError::Protocol("response missing content".into()))?
            .clone();
        let stop = val
            .get("stop_reason")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Ok((content, stop))
    }
}

#[async_trait]
impl ToolLoop for ClaudeBackend {
    // Drive the Anthropic tool-use loop over content blocks: assistant text +
    // `tool_use` blocks in, `tool_result` blocks back. Stops when the model
    // stops requesting tools, or after MAX_TURNS.
    async fn run_tool_loop(
        &self,
        mut messages: Vec<Value>,
        system: &str,
        tools: &Value,
        dispatch: &mut (dyn for<'a, 'b> FnMut(&'a str, &'b Value) -> (String, bool) + Send),
    ) -> Result<(Vec<Value>, String), ManagerError> {
        let mut reply = String::new();
        for _ in 0..MAX_TURNS {
            let body = json!({
                "model": self.model,
                "max_tokens": MAX_TOKENS,
                "system": system,
                "tools": tools,
                "messages": messages,
            });

            let (content, stop) = self.send_turn(&body).await?;

            let mut tool_results: Vec<Value> = Vec::new();
            for block in &content {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            append_reply(&mut reply, t);
                        }
                    }
                    Some("tool_use") => {
                        let id = block
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("");
                        let empty = json!({});
                        let input = block.get("input").unwrap_or(&empty);
                        let (result, is_error) = dispatch(name, input);
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": result,
                            "is_error": is_error,
                        }));
                    }
                    _ => {}
                }
            }

            messages.push(json!({ "role": "assistant", "content": content }));

            if stop == "tool_use" && !tool_results.is_empty() {
                messages.push(json!({ "role": "user", "content": tool_results }));
                continue;
            }
            break;
        }
        Ok((messages, reply.trim().to_string()))
    }
}

// The accumulated output of the capture tool loop.
struct LoopResult {
    messages: Vec<Value>,
    reply: String,
    tasks: Vec<Task>,
    notes: Vec<String>,
}

// Every transport gets the three flows for free: the orchestration above is
// written once against `ToolLoop`, so a new backend only implements that.
#[async_trait]
impl<T: ToolLoop> ManagerBackend for T {
    async fn manage(
        &self,
        task: &Task,
        prompt: &str,
        tags: &[String],
    ) -> Result<ManagerOutcome, ManagerError> {
        run_manage(self, task, prompt, tags).await
    }

    async fn capture(&self, prompt: &str, tags: &[String]) -> Result<CaptureOutcome, ManagerError> {
        run_capture(self, prompt, tags).await
    }

    async fn chat(
        &self,
        history: ChatHistory,
        user: &str,
        tags: &[String],
    ) -> Result<ChatOutcome, ManagerError> {
        run_chat(self, history, user, tags).await
    }
}

// Convert the shared Anthropic-style tool specs to OpenAI's function-tool
// shape, so both backends drive one schema source. Anthropic's `input_schema`
// is the same JSON Schema OpenAI expects under `parameters`.
fn to_openai_tools(tools: &Value) -> Value {
    tools
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.get("name").cloned().unwrap_or(Value::Null),
                    "description": t.get("description").cloned().unwrap_or(Value::Null),
                    "parameters": t.get("input_schema").cloned().unwrap_or_else(|| json!({})),
                }
            })
        })
        .collect()
}

// OpenAI-compatible backend: talks to any server that implements the OpenAI
// `/chat/completions` API — local (Ollama, llama.cpp, LM Studio, vLLM,
// LocalAI) or remote. The API key is optional (many local servers ignore it)
// and read from $OPENAI_API_KEY, never config.
pub struct OpenAiBackend {
    client: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
    model: String,
}

impl OpenAiBackend {
    pub fn new(model: String, base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: env::var("OPENAI_API_KEY").ok().filter(|k| !k.is_empty()),
            model,
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    // One request/response turn. Returns `choices[0].message`.
    async fn send_turn(&self, body: &Value) -> Result<Value, ManagerError> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(body);
        if let Some(key) = &self.api_key {
            req = req.header("authorization", format!("Bearer {key}"));
        }
        let val = post_json(req).await?;
        val.get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
            .and_then(|c| c.get("message"))
            .cloned()
            .ok_or_else(|| ManagerError::Protocol("response missing choices[0].message".into()))
    }
}

#[async_trait]
impl ToolLoop for OpenAiBackend {
    // Drive the OpenAI tool-calling loop: a leading system message, assistant
    // messages carrying `tool_calls`, and `{role:"tool"}` results keyed by
    // `tool_call_id`.
    async fn run_tool_loop(
        &self,
        mut messages: Vec<Value>,
        system: &str,
        tools: &Value,
        dispatch: &mut (dyn for<'a, 'b> FnMut(&'a str, &'b Value) -> (String, bool) + Send),
    ) -> Result<(Vec<Value>, String), ManagerError> {
        let has_system = messages
            .first()
            .and_then(|m| m.get("role"))
            .and_then(Value::as_str)
            == Some("system");
        if !has_system {
            messages.insert(0, json!({ "role": "system", "content": system }));
        }
        let tools = to_openai_tools(tools);
        let mut reply = String::new();
        for _ in 0..MAX_TURNS {
            let body = json!({
                "model": self.model,
                "messages": messages,
                "tools": tools,
                "stream": false,
            });

            let message = self.send_turn(&body).await?;

            if let Some(t) = message.get("content").and_then(Value::as_str) {
                append_reply(&mut reply, t);
            }

            let tool_calls = message
                .get("tool_calls")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            messages.push(message);

            if tool_calls.is_empty() {
                break;
            }
            for call in &tool_calls {
                let id = call.get("id").and_then(Value::as_str).unwrap_or("");
                let func = call.get("function");
                let name = func
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                // OpenAI encodes arguments as a JSON string; tolerate an object.
                let args: Value = match func.and_then(|f| f.get("arguments")) {
                    Some(Value::String(s)) => serde_json::from_str(s).unwrap_or_else(|_| json!({})),
                    Some(v) => v.clone(),
                    None => json!({}),
                };
                let (result, _is_error) = dispatch(name, &args);
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": result,
                }));
            }
        }
        Ok((messages, reply.trim().to_string()))
    }
}

const SYSTEM_PROMPT: &str = "You are SpongeBob SquarePants, the endlessly \
cheerful fry cook of the Krusty Krab, acting as the user's task manager inside \
a pomodoro TUI. Stay in character: upbeat, optimistic, and eager to help, fond \
of an 'I'm ready!' or 'Aye aye!' — but keep it light and never let the bit get \
in the way of being useful. Help the user break the task shown into \
concrete subtasks and keep its status and notes up to date. Use the tools to \
add, complete, or remove subtasks, set the task status, and record short \
progress notes. You can also fix the task's details: correct its tag (only to \
one of the configured tags you're given), rename it, set/clear its due date, \
or make it recurring (completing a recurring task reschedules it instead of \
finishing it). \
Keep any text replies to one or two sentences. Do not invent work or make \
changes the user did not ask for.";

// A human-readable snapshot of the task, prepended to the first user message.
fn task_context(task: &Task) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "Current task: {}", task.name);
    let _ = writeln!(s, "Tag: {}", task.tag);
    let _ = writeln!(s, "Status: {:?}", task.status);
    if let Some(d) = task.due {
        let _ = writeln!(s, "Due: {}", d.format(DUE_FORMAT));
    }
    if let Some(r) = &task.recur {
        let _ = writeln!(s, "Recurs: every {r}");
    }
    if task.subtasks.is_empty() {
        let _ = writeln!(s, "Subtasks: (none yet)");
    } else {
        let _ = writeln!(s, "Subtasks:");
        for st in &task.subtasks {
            let mark = if st.done { "x" } else { " " };
            let _ = writeln!(s, "  [{mark}] {}", st.name);
        }
    }
    s
}

// The tools exposed to the model. All mutate the managed task only. `tags`
// constrains the tag the model may set via `set_tag`.
fn tool_specs(tags: &[String]) -> Value {
    json!([
        {
            "name": "add_subtask",
            "description": "Add a concrete next step to the current task.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Short subtask description" }
                },
                "required": ["name"]
            }
        },
        {
            "name": "toggle_subtask",
            "description": "Mark an existing subtask done or not done, by its name.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "done": { "type": "boolean" }
                },
                "required": ["name", "done"]
            }
        },
        {
            "name": "remove_subtask",
            "description": "Remove an existing subtask by its name.",
            "input_schema": {
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"]
            }
        },
        {
            "name": "set_status",
            "description": "Set the status of the current task.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": ["todo", "in_progress", "done"] }
                },
                "required": ["status"]
            }
        },
        {
            "name": "add_note",
            "description": "Append a timestamped progress note to the task history.",
            "input_schema": {
                "type": "object",
                "properties": { "note": { "type": "string" } },
                "required": ["note"]
            }
        },
        {
            "name": "set_tag",
            "description": "Correct the task's tag. Must be one of the configured tags.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "tag": {
                        "type": "string",
                        "enum": tags,
                        "description": "One of the configured tags (verbatim)"
                    }
                },
                "required": ["tag"]
            }
        },
        {
            "name": "rename_task",
            "description": "Rename the task (fix a typo or reword it).",
            "input_schema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "New task name" }
                },
                "required": ["name"]
            }
        },
        {
            "name": "set_due",
            "description": "Set or clear the task's due date. Provide \
                'YYYY-MM-DD HH:MM', or an empty string to clear it.",
            "input_schema": {
                "type": "object",
                "properties": { "due": { "type": "string" } },
                "required": ["due"]
            }
        },
        {
            "name": "set_recur",
            "description": "Make the task recurring, or stop it recurring. \
                Provide an interval like '12h', '3d', or '2w', or an empty \
                string to clear it. Completing a recurring task reschedules \
                its due date that far ahead instead of finishing it, and it \
                never expires when overdue.",
            "input_schema": {
                "type": "object",
                "properties": { "recur": { "type": "string" } },
                "required": ["recur"]
            }
        }
    ])
}

// Apply one tool call to the working task. Returns (result_text, changed). This
// is the pure, network-free core the unit tests exercise. `tags` constrains the
// tag `set_tag` may assign.
fn apply_tool(task: &mut Task, name: &str, input: &Value, tags: &[String]) -> (String, bool) {
    let field = |k: &str| {
        input
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string()
    };
    match name {
        "add_subtask" => {
            let n = field("name");
            if n.is_empty() {
                return ("error: subtask name was empty".into(), false);
            }
            task.add_subtask(n.clone());
            (format!("added subtask '{n}'"), true)
        }
        "toggle_subtask" => {
            let n = field("name");
            let done = input.get("done").and_then(Value::as_bool).unwrap_or(true);
            if task.set_subtask_done(&n, done) {
                (format!("set '{n}' done={done}"), true)
            } else {
                (format!("error: no subtask named '{n}'"), false)
            }
        }
        "remove_subtask" => {
            let n = field("name");
            if task.remove_subtask(&n) {
                (format!("removed subtask '{n}'"), true)
            } else {
                (format!("error: no subtask named '{n}'"), false)
            }
        }
        "set_status" => match field("status").as_str() {
            "todo" => {
                task.set_status(Status::Todo);
                ("status set to todo".into(), true)
            }
            "in_progress" => {
                task.set_status(Status::InProgress);
                ("status set to in_progress".into(), true)
            }
            "done" => {
                task.set_status(Status::Done);
                ("status set to done".into(), true)
            }
            other => (format!("error: unknown status '{other}'"), false),
        },
        "add_note" => {
            let note = field("note");
            if note.is_empty() {
                return ("error: note was empty".into(), false);
            }
            task.log(note);
            ("note added".into(), true)
        }
        "set_tag" => {
            let requested = field("tag");
            match find_tag(tags, &requested) {
                Some(t) => {
                    task.tag = t.clone();
                    (format!("changed tag to '{t}'"), true)
                }
                None => (
                    format!(
                        "error: '{requested}' is not a configured tag ({})",
                        tags.join(", ")
                    ),
                    false,
                ),
            }
        }
        "rename_task" => {
            let n = field("name");
            if n.is_empty() {
                return ("error: name was empty".into(), false);
            }
            task.name = n.clone();
            (format!("renamed to '{n}'"), true)
        }
        "set_due" => {
            let d = field("due");
            if d.is_empty() {
                task.due = None;
                return ("cleared due date".into(), true);
            }
            match parse_due(&d) {
                Some(parsed) => {
                    task.due = Some(parsed);
                    (format!("set due to {}", parsed.format(DUE_FORMAT)), true)
                }
                None => ("error: bad due date (use YYYY-MM-DD HH:MM)".into(), false),
            }
        }
        "set_recur" => {
            let r = field("recur");
            if r.is_empty() {
                task.recur = None;
                return ("cleared recurrence".into(), true);
            }
            match parse_recur(&r) {
                Some(_) => {
                    task.recur = Some(r.clone());
                    (format!("set to recur every {r}"), true)
                }
                None => (
                    "error: bad recur interval (use 12h / 3d / 2w)".into(),
                    false,
                ),
            }
        }
        _ => (format!("error: unknown tool '{name}'"), false),
    }
}

const CAPTURE_SYSTEM_PROMPT: &str = "You are SpongeBob SquarePants of the \
Krusty Krab, turning the user's free-form description into concrete tasks for \
their pomodoro \
manager. Call the add_task tool once per task you identify. Always choose the \
tag from the provided list of available tags — never invent a new one. Add a \
due date only if the user clearly states one, formatted 'YYYY-MM-DD HH:MM'. If \
the user says a task repeats (daily, every 3 days, weekly, …), set recur to \
the interval ('1d', '3d', '1w'). Add \
subtasks only if the user spelled out steps. After recording the tasks, reply \
in one short sentence, in character. Do not invent tasks the user did not \
mention.";

const PLAN_SYSTEM_PROMPT: &str = "You are SpongeBob SquarePants of the Krusty \
Krab, helping the user plan their day at startup. You'll be shown their \
leftover (not-done) \
tasks. Greet them in character, briefly note what's carried over, and ask what \
they want to get done today. As the conversation goes, call the add_task tool \
to record any NEW tasks they mention — always choosing the tag from the \
available list, never inventing one. You do NOT decide what they work on next \
(a weighted-random picker handles that) and you cannot change existing tasks \
yet — focus on reviewing leftovers and capturing new work. Keep replies short \
and conversational, a sentence or two.";

// The single tool exposed during NL capture: record one new task.
fn capture_tool_specs() -> Value {
    json!([
        {
            "name": "add_task",
            "description": "Record one new task the user described.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Short task name" },
                    "tag": {
                        "type": "string",
                        "description": "One of the available tags (verbatim)"
                    },
                    "due": {
                        "type": "string",
                        "description": "Optional due date, 'YYYY-MM-DD HH:MM'"
                    },
                    "recur": {
                        "type": "string",
                        "description": "Optional recurrence interval like \
                            '12h', '3d', '2w' — only when the user says the \
                            task repeats"
                    },
                    "subtasks": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional ordered subtask names"
                    }
                },
                "required": ["name", "tag"]
            }
        }
    ])
}

// Build a Task from an add_task tool input, constraining the tag to `tags`
// (case-insensitive; falls back to the first tag). Returns the task and an
// optional note when the tag was remapped. None if the name was empty. Pure
// and network-free, so the unit tests exercise it directly.
fn build_task(input: &Value, tags: &[String]) -> Option<(Task, Option<String>)> {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let requested = input
        .get("tag")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let mut notes: Vec<String> = Vec::new();
    let tag = match find_tag(tags, requested) {
        Some(t) => t.clone(),
        None => {
            let fallback = tags.first().cloned().unwrap_or_default();
            notes.push(if requested.is_empty() {
                format!("'{name}': no tag given → '{fallback}'")
            } else {
                format!("'{name}': unknown tag '{requested}' → '{fallback}'")
            });
            fallback
        }
    };
    let due = input.get("due").and_then(Value::as_str).and_then(parse_due);
    let mut task = Task::new(tag, name, due);
    if let Some(r) = input.get("recur").and_then(Value::as_str).map(str::trim) {
        if r.is_empty() {
            // absent/blank: not recurring
        } else if parse_recur(r).is_some() {
            task.recur = Some(r.to_string());
        } else {
            notes.push(format!("'{}': ignored bad recur '{r}'", task.name));
        }
    }
    if let Some(subs) = input.get("subtasks").and_then(Value::as_array) {
        for s in subs.iter().filter_map(Value::as_str) {
            let s = s.trim();
            if !s.is_empty() {
                task.add_subtask(s.to_string());
            }
        }
    }
    let note = (!notes.is_empty()).then(|| notes.join("; "));
    Some((task, note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::Status;

    fn task() -> Task {
        Task::new("job".into(), "ship it".into(), None)
    }

    #[test]
    fn add_toggle_remove_subtask() {
        let mut t = task();
        let (_, c) = apply_tool(
            &mut t,
            "add_subtask",
            &json!({ "name": "write tests" }),
            &[],
        );
        assert!(c);
        assert_eq!(t.subtasks.len(), 1);

        let (_, c) = apply_tool(
            &mut t,
            "toggle_subtask",
            &json!({ "name": "WRITE TESTS", "done": true }),
            &[],
        );
        assert!(c, "match is case-insensitive");
        assert!(t.subtasks[0].done);

        let (_, c) = apply_tool(
            &mut t,
            "remove_subtask",
            &json!({ "name": "write tests" }),
            &[],
        );
        assert!(c);
        assert!(t.subtasks.is_empty());
    }

    #[test]
    fn toggle_unknown_subtask_is_error() {
        let mut t = task();
        let (msg, c) = apply_tool(
            &mut t,
            "toggle_subtask",
            &json!({ "name": "nope", "done": true }),
            &[],
        );
        assert!(!c);
        assert!(msg.starts_with("error"));
    }

    #[test]
    fn set_status_and_add_note() {
        let mut t = task();
        let (_, c) = apply_tool(
            &mut t,
            "set_status",
            &json!({ "status": "in_progress" }),
            &[],
        );
        assert!(c);
        assert_eq!(t.status, Status::InProgress);

        let (_, c) = apply_tool(&mut t, "add_note", &json!({ "note": "made progress" }), &[]);
        assert!(c);
        assert_eq!(t.history.len(), 1);
    }

    #[test]
    fn empty_and_unknown_inputs_are_errors() {
        let mut t = task();
        assert!(!apply_tool(&mut t, "add_subtask", &json!({ "name": "  " }), &[]).1);
        assert!(!apply_tool(&mut t, "bogus", &json!({}), &[]).1);
        assert!(!apply_tool(&mut t, "set_status", &json!({ "status": "x" }), &[]).1);
    }

    #[test]
    fn set_tag_rename_and_due() {
        let tags = vec!["job".to_string(), "school".to_string()];
        let mut t = task(); // starts tag "job"

        // set_tag: case-insensitive match, canonical casing from config.
        let (_, c) = apply_tool(&mut t, "set_tag", &json!({ "tag": "SCHOOL" }), &tags);
        assert!(c);
        assert_eq!(t.tag, "school");
        // set_tag: unconfigured tag is rejected, task unchanged.
        let (msg, c) = apply_tool(&mut t, "set_tag", &json!({ "tag": "errands" }), &tags);
        assert!(!c);
        assert!(msg.starts_with("error"));
        assert_eq!(t.tag, "school");

        // rename_task: empty rejected, non-empty applied.
        assert!(!apply_tool(&mut t, "rename_task", &json!({ "name": " " }), &tags).1);
        let (_, c) = apply_tool(
            &mut t,
            "rename_task",
            &json!({ "name": "ship it v2" }),
            &tags,
        );
        assert!(c);
        assert_eq!(t.name, "ship it v2");

        // set_due: set a valid date, then clear with an empty string.
        let (_, c) = apply_tool(
            &mut t,
            "set_due",
            &json!({ "due": "2050-01-01 10:00" }),
            &tags,
        );
        assert!(c);
        assert!(t.due.is_some());
        let (_, c) = apply_tool(&mut t, "set_due", &json!({ "due": "" }), &tags);
        assert!(c);
        assert!(t.due.is_none());
    }

    #[test]
    fn set_recur_sets_clears_and_rejects() {
        let mut t = task();
        let (_, c) = apply_tool(&mut t, "set_recur", &json!({ "recur": "3d" }), &[]);
        assert!(c);
        assert_eq!(t.recur.as_deref(), Some("3d"));

        // A bad interval errors and leaves the task unchanged.
        let (msg, c) = apply_tool(&mut t, "set_recur", &json!({ "recur": "yearly" }), &[]);
        assert!(!c);
        assert!(msg.starts_with("error"));
        assert_eq!(t.recur.as_deref(), Some("3d"));

        // An empty string clears it.
        let (_, c) = apply_tool(&mut t, "set_recur", &json!({ "recur": "" }), &[]);
        assert!(c);
        assert!(t.recur.is_none());
    }

    #[test]
    fn build_task_reads_recur_and_notes_bad_intervals() {
        let tags = vec!["job".to_string()];
        let (t, note) = build_task(
            &json!({ "name": "standup", "tag": "job", "recur": "1d" }),
            &tags,
        )
        .unwrap();
        assert_eq!(t.recur.as_deref(), Some("1d"));
        assert!(note.is_none());

        let (t, note) = build_task(
            &json!({ "name": "standup", "tag": "job", "recur": "sometimes" }),
            &tags,
        )
        .unwrap();
        assert!(t.recur.is_none());
        assert!(note.unwrap().contains("sometimes"));
    }

    #[tokio::test]
    async fn none_backend_reports_disabled() {
        let err = NoneBackend.manage(&task(), "help", &[]).await.unwrap_err();
        assert!(matches!(err, ManagerError::Disabled));
        let err = NoneBackend.capture("buy stuff", &[]).await.unwrap_err();
        assert!(matches!(err, ManagerError::Disabled));
    }

    #[test]
    fn build_task_keeps_known_tag_and_subtasks() {
        let tags = vec!["work".to_string(), "home".to_string()];
        let (t, note) = build_task(
            &json!({ "name": "ship", "tag": "WORK", "subtasks": ["a", "b"] }),
            &tags,
        )
        .unwrap();
        assert_eq!(t.tag, "work"); // canonical casing from config
        assert_eq!(t.subtasks.len(), 2);
        assert!(note.is_none());
    }

    #[test]
    fn build_task_remaps_unknown_tag_to_fallback() {
        let tags = vec!["work".to_string(), "home".to_string()];
        let (t, note) = build_task(&json!({ "name": "x", "tag": "errands" }), &tags).unwrap();
        assert_eq!(t.tag, "work");
        assert!(note.unwrap().contains("errands"));
    }

    #[test]
    fn build_task_rejects_empty_name() {
        let tags = vec!["work".to_string()];
        assert!(build_task(&json!({ "name": "  ", "tag": "work" }), &tags).is_none());
    }

    #[test]
    fn to_openai_tools_converts_shape() {
        let converted = to_openai_tools(&tool_specs(&["job".into()]));
        let arr = converted.as_array().unwrap();
        assert!(!arr.is_empty());
        for t in arr {
            assert_eq!(t["type"], "function");
            let f = &t["function"];
            assert!(f.get("name").and_then(Value::as_str).is_some());
            // the Anthropic input_schema carries over verbatim as parameters
            assert!(f.get("parameters").and_then(Value::as_object).is_some());
        }
    }

    #[test]
    fn to_openai_tools_preserves_add_task() {
        let converted = to_openai_tools(&capture_tool_specs());
        let names: Vec<&str> = converted
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["function"]["name"].as_str())
            .collect();
        assert!(names.contains(&"add_task"));
    }
}
