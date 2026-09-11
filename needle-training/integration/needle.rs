//! Needle Mode spike — a warm Python subprocess running the `cactus-needle`
//! tool-calling model, driven over JSON-lines stdin/stdout.
//!
//! This is a SPIKE to prove the bundling path is viable on Windows. The host
//! keeps one long-lived worker process, sends `{id, query, tools}` requests,
//! and reads back the model's `complete()` response. In production the host
//! would then execute the returned `function_calls` itself.
//!
//! If the bundled Python/`cactus-needle` runtime is unavailable, the command
//! returns a structured error rather than hanging.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

#[derive(Debug, Serialize, Deserialize)]
pub struct NeedleRequest {
    pub query: String,
    /// JSON schemas describing the available tools (Needle's `tools` param).
    pub tools: Vec<serde_json::Value>,
    #[serde(default)]
    pub reset: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NeedleResponseFailure {
    pub error: String,
    pub ms: f64,
    pub response: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NeedleResponse {
    pub id: u64,
    pub ok: bool,
    pub response: Option<serde_json::Value>,
    pub error: Option<String>,
    pub ms: f64,
}

pub struct NeedleWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl NeedleWorker {
    /// Spawn the worker. `python` must resolve to the interpreter that has
    /// `cactus-needle` installed (adjust path in production bundling).
    pub fn spawn() -> Result<Self, String> {
        let worker_script = worker_script_path();
        let mut child = Command::new(python_exe())
            .arg(worker_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("cannot spawn needle worker: {e}"))?;
        let stdin = child.stdin.take().ok_or("no worker stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no worker stdout")?);
        Ok(NeedleWorker { child, stdin, stdout, next_id: 0 })
    }

    fn request(&mut self, req: &NeedleRequest) -> Result<NeedleResponse, String> {
        self.next_id += 1;
        let id = self.next_id;
        let line = serde_json::json!({
            "id": id,
            "query": req.query,
            "tools": req.tools,
            "reset": req.reset,
        });
        let mut text = serde_json::to_string(&line).map_err(|e| e.to_string())?;
        text.push('\n');
        self.stdin
            .write_all(text.as_bytes())
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("write to needle worker: {e}"))?;

        // Wait for the matching response line (worker answers 1:1).
        loop {
            let mut raw = String::new();
            let n = self.stdout
                .read_line(&mut raw)
                .map_err(|e| format!("read from needle worker: {e}"))?;
            if n == 0 {
                // Worker exited (e.g. Python failed to import cactus-needle).
                return Err("needle worker exited (is cactus-needle installed?)".into());
            }
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let parsed: NeedleResponse = serde_json::from_str(trimmed)
                .map_err(|e| format!("bad worker response: {e}"))?;
            if parsed.id == id {
                return Ok(parsed);
            }
        }
    }
}

impl Drop for NeedleWorker {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

// One shared worker (serialized) — a single warm model, one turn at a time.
// Stored in AppState as `Mutex<Option<NeedleWorker>>`.

fn worker_script_path() -> std::path::PathBuf {
    // During dev this lives next to the crate source; in a bundled build it
    // would be shipped in the resource dir.
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("needle_worker.py")
}

fn python_exe() -> String {
    std::env::var("NEEDLE_PYTHON").unwrap_or_else(|_| "python".to_string())
}

/// Ensure a worker exists, then run one `complete()` turn.
pub fn needle_run_impl(
    state: &Mutex<Option<NeedleWorker>>,
    req: &NeedleRequest,
) -> Result<serde_json::Value, String> {
    let mut guard = state.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(NeedleWorker::spawn()?);
    }
    let resp = guard.as_mut().unwrap().request(req)?;
    if !resp.ok {
        return Err(resp.error.unwrap_or_else(|| "needle worker error".into()));
    }
    let mut out = resp.response.unwrap_or(serde_json::Value::Null);
    // Attach the latency so the UI can show where time went.
    if let Some(o) = out.as_object_mut() {
        o.insert("needle_ms".into(), serde_json::json!(resp.ms));
    }
    Ok(out)
}
