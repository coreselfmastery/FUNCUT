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
    // The allowlist: `--mcp-allow a,b,c`, or `--mcp-allow=a,b,c`. A
    // restricting flag that silently fails open is the one flag that
    // must not: the word form with no list behind it is a usage error,
    // not "serve everything".
    let allow_at = words
        .iter()
        .position(|word| word == "--mcp-allow")
        .map(|at| {
            let Some(list) = words.get(at + 1) else {
                eprintln!("concat mcp: --mcp-allow names the tools to serve, and none were named");
                eprintln!(
                    "usage: concat-mcp [auto|window|headless] [--mcp-read-only] [--mcp-allow a,b,c]"
                );
                std::process::exit(2);
            };
            list.to_owned()
        });
    let allowed = allow_at
        .or_else(|| {
            words
                .iter()
                .find_map(|word| word.strip_prefix("--mcp-allow="))
                .map(str::to_owned)
        })
        .map(|list| {
            list.split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect::<Vec<String>>()
        })
        .filter(|list| !list.is_empty());
    for flag in words.iter().filter(|word| word.starts_with("--")) {
        if flag != "--mcp-read-only" && !flag.starts_with("--mcp-allow") {
            eprintln!("concat mcp: unknown flag {flag}");
            eprintln!(
                "usage: concat-mcp [auto|window|headless] [--mcp-read-only] [--mcp-allow a,b,c]"
            );
            return ExitCode::from(2);
        }
    }
    // The mode word: the first word that is neither a flag nor the list
    // behind `--mcp-allow` - the list itself is not a mode, whatever
    // order the words arrive in.
    let list_at = words
        .iter()
        .position(|word| word == "--mcp-allow")
        .map(|at| at + 1);
    let word = words
        .iter()
        .enumerate()
        .find(|(at, word)| !word.starts_with("--") && list_at.is_none_or(|list| *at != list))
        .map(|(_, word)| word.clone())
        .or_else(|| std::env::var("CONCAT_MCP_MODE").ok())
        .unwrap_or_default();
    let Some(mode) = concat_mcp::Mode::parse(&word) else {
        eprintln!("usage: concat-mcp [auto|window|headless] [--mcp-read-only] [--mcp-allow a,b,c]");
        return ExitCode::from(2);
    };
    // The same switches the usage line names from the environment, so a
    // server started by a script that sets CONCAT_MCP_READ_ONLY is
    // actually read-only - the flag and the variable are OR'd, and a
    // list given either way is the list served.
    let from_env = concat_mcp::protocol::Serving::from_environment();
    let serving = concat_mcp::protocol::Serving {
        read_only: read_only || from_env.read_only,
        allowed: allowed.or(from_env.allowed),
    };
    match concat_mcp::run_with(mode, serving) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("concat mcp: {error}");
            ExitCode::FAILURE
        }
    }
}
