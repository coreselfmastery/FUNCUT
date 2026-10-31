// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The MCP server on its own, for development and the end-to-end test.
//! What ships is the same server inside the app: `concat --mcp`.
//!
//! usage: concat-mcp [auto|window|headless] [--mcp-read-only] [--mcp-allow a,b,c]
//!                  (or CONCAT_MCP_MODE, CONCAT_MCP_READ_ONLY, CONCAT_MCP_ALLOW)
//!
//! The flags are the safety net: `--mcp-read-only` refuses every tool
//! that changes something (a harness or a stranger gets a server it
//! cannot edit through), and `--mcp-allow` names the only tools served -
//! a tool not on the list is refused as if it were not there. The app's
//! own `concat --mcp` reads the same switches from the environment, so
//! both doors have the same net.

use std::process::ExitCode;

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let read_only = words.iter().any(|word| word == "--mcp-read-only");
    let allowed = words
        .iter()
        .position(|word| word == "--mcp-allow")
        .and_then(|at| words.get(at + 1))
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect::<Vec<String>>()
        })
        .filter(|list| !list.is_empty());
    let word = words
        .iter()
        .find(|word| !word.starts_with("--"))
        .cloned()
        .or_else(|| std::env::var("CONCAT_MCP_MODE").ok())
        .unwrap_or_default();
    let Some(mode) = concat_mcp::Mode::parse(&word) else {
        eprintln!("usage: concat-mcp [auto|window|headless] [--mcp-read-only] [--mcp-allow a,b,c]");
        return ExitCode::from(2);
    };
    let serving = concat_mcp::protocol::Serving { read_only, allowed };
    match concat_mcp::run_with(mode, serving) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("concat mcp: {error}");
            ExitCode::FAILURE
        }
    }
}
