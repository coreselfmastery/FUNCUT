// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The AUTO EDIT seam: the adapter from a [`Backend`] to the runner's
//! [`ToolCaller`] port, and the runner's own vocabulary re-exported for
//! the window that stands the button on it.
//!
//! The runner crate (`concat-director`) owns the loop - the stages, the
//! progress events, the cancellation with rollback, the retries - and
//! drives one port, [`ToolCaller`]: one MCP-shaped call at a time, a
//! tool's name and its arguments, answered as the JSON the tool replies
//! with. The backend that stands behind that port lives here, which is
//! why the runner cannot name it; this module is the landing the
//! runner's docs promised.
//!
//! [`BackendToolCaller`] performs the same translation `tools/call`
//! performs for the protocol: the tool's name and arguments go to the
//! toolbox, the toolbox runs the tool against the backend, and the
//! outcome crosses back as the runner's `Ok(json)` or `Err(refusal)`.
//! A tool the server does not serve is a refusal that names it - the
//! same answer `tools/call` gives - and an error outcome carries the
//! sentence the tool wrote, with no opinion of the adapter's own about
//! retrying: the runner decides the way the watcher does, from the tool
//! and the message.
//!
//! The window's Forge page drives this adapter directly for its AUTO
//! EDIT button: the same toolbox, the same backend, the same meanings,
//! with the runner's events becoming the button's progress. An MCP
//! `auto_edit` tool for agents would stand on the same adapter.
//!
//! ```
//! use concat_director::{Runner, ToolCaller};
//! use concat_mcp::backend::Backend;
//! use concat_mcp::director::BackendToolCaller;
//! use concat_mcp::tools::Toolbox;
//! use std::sync::atomic::AtomicBool;
//! use std::sync::Mutex;
//! use std::path::PathBuf;
//!
//! struct Answers(Mutex<Vec<serde_json::Value>>);
//!
//! impl Backend for Answers {
//!     fn call(&mut self, _request: concat_api::Request)
//!         -> Result<serde_json::Value, concat_api::ApiError> {
//!         Ok(self.0.lock().unwrap().pop().unwrap_or_default())
//!     }
//!     fn describe(&self) -> serde_json::Value { serde_json::json!("test") }
//! }
//!
//! let mut backend = Answers(Mutex::new(vec![serde_json::json!({ "ok": true })]));
//! let toolbox = Toolbox::new();
//! let cancelled = AtomicBool::new(false);
//! let mut caller = BackendToolCaller::new(&toolbox, &mut backend, &cancelled);
//! let (runner, _events) = Runner::new(PathBuf::from("/edits/Reel"));
//! // ... the program, and runner.run(&program, &mut caller)
//! # let _ = runner;
//! # let _: &mut dyn ToolCaller = &mut caller;
//! ```

use std::sync::atomic::AtomicBool;

use concat_director::ToolCaller;
use serde_json::{Value, json};

use crate::backend::Backend;
use crate::tools::{Context, Outcome, Toolbox};

pub use concat_director::{
    AutoEditError, AutoEditEvent, AutoEditProgress, AutoEditState, EDIT_APPLY, EDIT_UNDO,
    EditProgram, ErrorPolicy, RunOutcome, Runner, Stage, StageStep, ToolCall, ToolError,
};

/// The runner's port, stood on this crate's own ground: the toolbox a
/// `tools/call` would reach, the backend it would run the tool against,
/// and the flag a cancellation raises - the same three the protocol
/// thread holds, so a call the runner makes means exactly what the same
/// call from an agent would mean.
pub struct BackendToolCaller<'a> {
    /// The served tools, with their schemas.
    toolbox: &'a Toolbox,
    /// Where the tool's requests go.
    backend: &'a mut dyn Backend,
    /// Set when the run is asked to stop: a tool that polls it - the
    /// export's wait, an analysis - gives up at its next look, the same
    /// look a protocol cancellation would make it take.
    cancelled: &'a AtomicBool,
}

impl<'a> BackendToolCaller<'a> {
    /// The adapter over `toolbox` and `backend`, stopped by `cancelled`.
    pub fn new(
        toolbox: &'a Toolbox,
        backend: &'a mut dyn Backend,
        cancelled: &'a AtomicBool,
    ) -> BackendToolCaller<'a> {
        BackendToolCaller {
            toolbox,
            backend,
            cancelled,
        }
    }
}

impl ToolCaller for BackendToolCaller<'_> {
    fn call(&mut self, call: &ToolCall) -> Result<Value, ToolError> {
        // The same context a tools/call builds: the backend, the cancel
        // flag, and no progress reporter - the runner's own events are
        // the progress this run reports.
        let context = Context {
            backend: &mut *self.backend,
            cancelled: self.cancelled,
            progress: None,
        };
        match self
            .toolbox
            .call(&call.tool, call.arguments.clone(), context)
        {
            None => Err(ToolError {
                message: format!("there is no {} tool on this server", call.tool),
                // A tool this server does not serve will not be served by
                // trying again: the adapter says so, rather than leaving
                // the runner to read the message for a marker it does not
                // carry.
                retryable: Some(false),
                retry_after: None,
                code: None,
            }),
            Some(Outcome {
                is_error: false,
                structured,
                ..
            }) => Ok(structured.unwrap_or_else(|| json!({}))),
            Some(Outcome {
                is_error: true,
                content,
                structured,
                ..
            }) => Err(ToolError {
                message: refusal_message(&content, structured.as_ref()),
                // No opinion: the runner reads the tool and the message
                // the way the watcher does, and a transient-sounding
                // refusal from a tool whose retries land is tried again.
                retryable: None,
                retry_after: None,
                code: refusal_code(structured.as_ref()),
            }),
        }
    }
}

/// The sentence a refusal carries: the structured error's message - the
/// same field a client that reads `structuredContent` shows - falling
/// back to the first words of the content blocks, which is the same
/// sentence in the text form.
fn refusal_message(content: &[Value], structured: Option<&Value>) -> String {
    if let Some(message) = structured
        .and_then(|value| value.get("error"))
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
    {
        return message.to_owned();
    }
    content
        .iter()
        .find_map(|block| block.get("text").and_then(Value::as_str))
        .map(str::to_owned)
        .unwrap_or_else(|| "the tool refused without a message".to_owned())
}

/// The JSON-RPC number a refusal carried, when it carried one: the
/// structured error's `code`, read as the number the runner's records
/// keep. The protocol's own error codes are words (`invalid`,
/// `notFound`); those stay in the message, where the sentence already
/// says them.
fn refusal_code(structured: Option<&Value>) -> Option<i64> {
    structured
        .and_then(|value| value.get("error"))
        .and_then(|error| error.get("code"))
        .and_then(Value::as_i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_api::{ApiError, Request};
    use concat_director::{AutoEditError, Stage};
    use serde_json::json;

    /// A backend that answers from a script, one reply per request, and
    /// records every request it took.
    struct Scripted {
        replies: Vec<Result<Value, ApiError>>,
        taken: Vec<Request>,
    }

    impl Backend for Scripted {
        fn call(&mut self, request: Request) -> Result<Value, ApiError> {
            self.taken.push(request);
            self.replies.pop().unwrap_or(Ok(json!({})))
        }

        fn describe(&self) -> Value {
            json!({ "mode": "test" })
        }
    }

    /// The one call every test makes, against a fresh adapter over
    /// `backend`'s remaining replies.
    fn answered(tool: &str, arguments: Value, backend: &mut Scripted) -> Result<Value, ToolError> {
        let toolbox = Toolbox::new();
        let cancelled = AtomicBool::new(false);
        let mut caller = BackendToolCaller::new(&toolbox, backend, &cancelled);
        concat_director::ToolCaller::call(
            &mut caller,
            &ToolCall {
                tool: tool.to_owned(),
                arguments,
            },
        )
    }

    #[test]
    fn a_call_that_lands_crosses_as_the_structured_reply() {
        let mut backend = Scripted {
            replies: vec![Ok(json!({
                "state": "done", "stage": "muxing",
                "frame": 100, "total": 100, "fraction": 1.0,
            }))],
            taken: Vec::new(),
        };
        let reply = answered("export_status", json!({ "job": "j1" }), &mut backend)
            .expect("the call landed");
        assert_eq!(reply["state"], "done");
        assert_eq!(reply["fraction"], 1.0);
        // The tool ran against the backend exactly once.
        assert_eq!(backend.taken.len(), 1);
    }

    #[test]
    fn a_tool_this_server_does_not_serve_is_refused_by_name() {
        let mut backend = Scripted {
            replies: Vec::new(),
            taken: Vec::new(),
        };
        let error =
            answered("no_such_tool", json!({}), &mut backend).expect_err("the tool is not served");
        assert_eq!(
            error.message,
            "there is no no_such_tool tool on this server"
        );
        // The adapter's own word: trying again cannot serve a tool this
        // server does not have.
        assert_eq!(error.retryable, Some(false));
        // And the runner records the same word, whatever the stage.
        let recorded = AutoEditError::new(
            Stage::Plan,
            Some(ToolCall {
                tool: "no_such_tool".to_owned(),
                arguments: json!({}),
            }),
            error,
        );
        assert!(!recorded.retryable);
    }

    #[test]
    fn a_refusal_crosses_with_the_sentence_the_tool_wrote() {
        // The shape Outcome::error writes: content text, and a
        // structured error beside it. A backend refusal the tool passes
        // through arrives this way.
        let mut backend = Scripted {
            replies: vec![Err(ApiError::failed("the render queue is busy"))],
            taken: Vec::new(),
        };
        let error = answered(
            "export_start",
            json!({ "path": "/edits/Reel", "output": "/edits/Reel/export.mp4" }),
            &mut backend,
        )
        .expect_err("the export was refused");
        assert_eq!(error.message, "the render queue is busy");
        // No opinion of the adapter's own: the runner reads the tool and
        // the message, and a busy queue is not the network's side.
        assert_eq!(error.retryable, None);
    }

    #[test]
    fn a_transient_refusal_is_left_to_the_runners_own_reading() {
        let mut backend = Scripted {
            replies: vec![Err(ApiError::failed("the connection was reset"))],
            taken: Vec::new(),
        };
        let error = answered(
            "transcribe",
            json!({ "path": "/vo/take.wav" }),
            &mut backend,
        )
        .expect_err("the transcription was refused");
        assert_eq!(error.retryable, None);
        // The runner's reading of that same refusal: a tool whose retries
        // land, and a failure that sounds like the network's side.
        let recorded = AutoEditError::new(
            Stage::Transcribe,
            Some(ToolCall {
                tool: "transcribe".to_owned(),
                arguments: json!({ "path": "/vo/take.wav" }),
            }),
            error,
        );
        assert!(recorded.retryable);
    }

    #[test]
    fn a_schema_refusal_names_the_misspelled_key() {
        // The toolbox checks a call's arguments against the schema it was
        // listed with before anything runs; a key the schema does not
        // name - `lufsTarget` where the words go - is refused with the
        // key in the sentence, the same answer an agent's tools/call
        // gets.
        let mut backend = Scripted {
            replies: Vec::new(),
            taken: Vec::new(),
        };
        let error = answered(
            "measure_loudness",
            json!({ "path": "/o.mp4", "lufsTarget": -14 }),
            &mut backend,
        )
        .expect_err("the arguments are not the schema's");
        assert!(
            error.message.contains("lufsTarget"),
            "the refusal names what was wrong: {message}",
            message = error.message
        );
        // Nothing reached the backend: the safety net answered first.
        assert!(backend.taken.is_empty());
    }
}
