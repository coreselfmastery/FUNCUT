// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Where a tool's request goes: the window's server, or an API of this
//! process's own.
//!
//! Both take a [`Request`] and give back its result as JSON, so a tool
//! cannot tell them apart and the meaning of every request stays the
//! API's. A third backend - the window's own open project, once the
//! window can be asked to edit it ("live mode", #337) - is one more
//! implementation of [`Backend`], with nothing above it changing.
//!
//! - [`Window`]: a JSON-RPC connection to the server the Automation page
//!   runs. Found through the listing the window writes
//!   (`concat_server::discovery`) or named by `CONCAT_API_ADDRESS` and
//!   `CONCAT_API_TOKEN`. Goes through the window's register of open
//!   projects, so the project on screen cannot be opened here and saved
//!   over, and through its one export slot.
//! - [`Headless`]: an [`Api`] in this process, writing under the user's
//!   home (or `CONCAT_MCP_ROOTS`). Nothing to start first, and nothing
//!   shared with a window: a project open in the window can be opened here
//!   too, which is why the window's server is preferred when it is there.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use concat_api::{Api, ApiError, AppDirs, Request, Response, Stopper};
use concat_server::client::Client;
use concat_server::discovery;
use serde_json::Value;

/// How long a connection to the window's server may take to open.
const PATIENCE: Duration = Duration::from_secs(3);

/// What a tool's request is handed to.
pub trait Backend: Send {
    /// Runs one request; its result as the JSON the API replies with.
    fn call(&mut self, request: Request) -> Result<Value, ApiError>;

    /// What this backend is, for the agent and the log: "window" or
    /// "headless", and where.
    fn describe(&self) -> Value;

    /// Where this backend's API allows writes, so tools that write files of
    /// their own - the sense layer's contact sheets and encodes, a motion
    /// bake - can hold an agent to the same line the API already draws for
    /// its own writes. Empty when the backend cannot know: the window's
    /// server holds its own list, and its refusal happens there.
    fn roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// The session is over: stop what is running, and wait for it.
    fn finish(&mut self) {}
}

/// Which backend `concat --mcp` uses.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Mode {
    /// The window's server when it can be reached, this process otherwise.
    #[default]
    Auto,
    /// The window's server, or an error saying how to turn it on.
    Window,
    /// This process, whatever else is running.
    Headless,
}

impl Mode {
    /// The mode a word names: "auto", "window" or "headless".
    pub fn parse(word: &str) -> Option<Mode> {
        match word.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Some(Mode::Auto),
            "window" => Some(Mode::Window),
            "headless" => Some(Mode::Headless),
            _ => None,
        }
    }
}

/// The backend `mode` asks for, with the reason for whatever was not
/// chosen written to standard error, where an MCP client shows a server's
/// log.
pub fn open(mode: Mode) -> Result<Box<dyn Backend>, String> {
    match mode {
        Mode::Headless => Ok(Box::new(Headless::start()?)),
        Mode::Window => Ok(Box::new(Window::find()?)),
        Mode::Auto => match Window::find() {
            Ok(window) => Ok(Box::new(window)),
            Err(why) => {
                eprintln!("concat mcp: working headless: {why}");
                Ok(Box::new(Headless::start()?))
            }
        },
    }
}

/// Where the window's server is, and its token: the environment's if it
/// names them, the window's listing otherwise.
fn locate() -> Result<(SocketAddr, String), String> {
    let address = std::env::var("CONCAT_API_ADDRESS")
        .ok()
        .filter(|text| !text.is_empty());
    let token = std::env::var("CONCAT_API_TOKEN")
        .ok()
        .filter(|text| !text.is_empty());
    if let (Some(address), Some(token)) = (&address, &token) {
        let address = address.parse().map_err(|_| {
            format!("CONCAT_API_ADDRESS {address:?} is not an address like 127.0.0.1:7420")
        })?;
        return Ok((address, token.clone()));
    }
    if address.is_some() != token.is_some() {
        return Err("CONCAT_API_ADDRESS and CONCAT_API_TOKEN go together".to_owned());
    }
    let dirs = AppDirs::locate()?;
    let path = discovery::path_in(&dirs.config);
    match discovery::read(&path)? {
        Some(listing) => Ok((listing.address, listing.token)),
        None => Err(
            "the Concat window is not serving the API: automation is off by \
             default on a fresh install - open Concat, then Settings > \
             Automation, and turn on Scripts and apps"
                .to_owned(),
        ),
    }
}

/// The window's server, over a connection that is reopened once if it
/// drops - the window restarted its server, say.
pub struct Window {
    client: Client,
}

impl Window {
    /// Connects to the window's server.
    pub fn find() -> Result<Window, String> {
        let (address, token) = locate()?;
        let client = Client::connect(address, &token, PATIENCE)
            .map_err(|error| format!("{error} (if the window has closed, its listing is stale)"))?;
        Ok(Window { client })
    }

    /// Connects to the server at `address` with `token`.
    pub fn connect(address: SocketAddr, token: &str) -> Result<Window, String> {
        Ok(Window {
            client: Client::connect(address, token, PATIENCE)?,
        })
    }
}

impl Backend for Window {
    fn call(&mut self, request: Request) -> Result<Value, ApiError> {
        match self.client.call(&request) {
            Err(error)
                if error
                    .message
                    .starts_with("the connection to the Concat server") =>
            {
                let again = Window::find().map_err(|why| {
                    ApiError::failed(format!(
                        "{} - and reconnecting failed: {why}",
                        error.message
                    ))
                })?;
                *self = again;
                self.client.call(&request)
            }
            outcome => outcome,
        }
    }

    fn describe(&self) -> Value {
        serde_json::json!({ "mode": "window", "address": self.client.address().to_string() })
    }
}

/// Asks the window's server to close its session on a project folder, so
/// the window itself can open what a run edited there.
///
/// The server holds every session an agent opened until it is closed this
/// way - a connection ending is not a project closing, and the next caller
/// of the same folder is served from the session that is already there -
/// while the window's claim on the same folder is refused for as long as
/// it stands ("the project is open through a remote connection"). A
/// finished AUTO EDIT lands by the window opening the project the run
/// edited, so the hand-off asks the server to let go first. Nothing is
/// lost: every applied edit autosaves, so the document on disk is the
/// whole edit, and a folder the server does not hold - the run was
/// headless, or never got as far as opening it - is released already,
/// which is the answer, not an error.
pub fn release_window_session(path: &str) -> Result<(), String> {
    release_on(Window::find()?, path)
}

/// The release over a connection the caller already holds: the same ask,
/// without the discovery - the window's own pane hands the connection it
/// found the server by, and a test hands one it made on purpose.
fn release_on(mut window: Window, path: &str) -> Result<(), String> {
    match window.call(concat_api::Request::ProjectClose {
        path: path.to_owned(),
        save: false,
    }) {
        Ok(_) => Ok(()),
        Err(error) if error.code == concat_api::ErrorCode::NotOpen => Ok(()),
        Err(error) => Err(error.message),
    }
}

/// An API in this process.
pub struct Headless {
    api: Api,
    stopper: Stopper,
}

impl Headless {
    /// An API over this machine's app directories, writing under the
    /// roots `CONCAT_MCP_ROOTS` names (the platform's path-list syntax),
    /// or the directory the server was started in.
    pub fn start() -> Result<Headless, String> {
        Ok(Headless::with_api(Api::new(Arc::new(|_| {}))?))
    }

    /// An API made by the caller - a test's, over a scratch folder.
    pub fn with_api(mut api: Api) -> Headless {
        let roots: Vec<PathBuf> = std::env::var_os("CONCAT_MCP_ROOTS")
            .map(|roots| {
                // An empty component - CONCAT_MCP_ROOTS="" or ":" - is not
                // a root; left in, it passed the emptiness check and then
                // matched every path there is, turning "confine writes"
                // into "confine nothing".
                std::env::split_paths(&roots)
                    .filter(|root| !root.as_os_str().is_empty())
                    .collect::<Vec<PathBuf>>()
            })
            .filter(|roots: &Vec<PathBuf>| !roots.is_empty())
            .unwrap_or_else(|| {
                // The directory the server was started in, not the whole
                // home. "Anywhere under home" read as a safety property
                // while it handed an agent every folder that matters -
                // .ssh, .config, the documents - when a headless session's
                // work area is where it was started. An operator who wants
                // more names CONCAT_MCP_ROOTS.
                std::env::current_dir().into_iter().collect()
            });
        api.add_capability("mcp");
        // The stock index, opened on a background thread the moment the
        // headless session exists: an agent's first openvid search arrives
        // seconds after the session opens, and the cold open of a 435 MB
        // SQLite index is seconds of that first call's time otherwise -
        // time a caller reads as a hang. Best-effort by construction: the
        // thread discards every outcome, a missing index is still the
        // prepare's to download, and a race with the first real asker is
        // settled by the library's own OnceLock.
        api.warm_openvid();
        if api.roots().is_empty() {
            api.restrict_writes_to(roots);
        }
        let stopper = api.stopper();
        Headless { api, stopper }
    }
}

impl Backend for Headless {
    fn call(&mut self, request: Request) -> Result<Value, ApiError> {
        match self.api.dispatch(request) {
            Response::Result(reply) => serde_json::to_value(&reply)
                .map_err(|error| ApiError::failed(format!("the reply did not encode: {error}"))),
            Response::Error(error) => Err(error),
        }
    }

    fn describe(&self) -> Value {
        let roots: Vec<String> = self
            .api
            .roots()
            .iter()
            .map(|root| root.display().to_string())
            .collect();
        serde_json::json!({ "mode": "headless", "writesUnder": roots })
    }

    fn roots(&self) -> Vec<PathBuf> {
        self.api.roots().to_vec()
    }

    fn finish(&mut self) {
        self.stopper.cancel_all();
        self.api.finish();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use concat_api::{Api, AppDirs, Request};

    /// A finished run's hand-off, over a real server: the session an
    /// agent opened is closed by the release, a second release of the
    /// same folder is already-released rather than an error, and the
    /// close takes nothing with it - the document on disk is what the
    /// run's own autosaves wrote, so the close asks for no save of its
    /// own and the edits are all still there for the window to open.
    #[test]
    fn a_release_closes_the_session_an_agent_left_open() {
        let scratch = tempfile::tempdir().expect("scratch");
        let home = scratch.path().to_path_buf();
        let dirs = AppDirs {
            config: home.join("config"),
            data: home.join("data"),
        };
        let server = concat_server::Server::start(
            concat_server::Config {
                json: Some("127.0.0.1:0".parse().expect("an address")),
                roots: vec![home.clone()],
                ..concat_server::Config::default()
            },
            move |events| Ok(Api::with_dirs(dirs, events)),
        )
        .expect("a server");
        let address = server.json_addr().expect("listening");
        let mut agent = Window::connect(address, server.token()).expect("a connection");
        agent
            .call(Request::ProjectCreate {
                location: home.display().to_string(),
                name: "Edited".to_owned(),
                video: None,
            })
            .expect("the project is made");
        let path = home.join("Edited").display().to_string();
        // The server holds the project open, the way a run's own calls
        // would leave it: a read through a second connection is served
        // from that session.
        let mut other = Window::connect(address, server.token()).expect("a second connection");
        assert!(
            other
                .call(Request::ProjectGet { path: path.clone() })
                .is_ok()
        );
        // The release closes it, and says so with nothing.
        release_on(agent, &path).expect("the release closes the session");
        // A folder the server no longer holds reads as not open, and
        // releasing it again is the same answer as releasing one it never
        // had: already released, not an error - a run that fell back
        // headless hands the window a folder no server session holds.
        let mut third = Window::connect(address, server.token()).expect("a third connection");
        let refused = third
            .call(Request::ProjectGet { path: path.clone() })
            .expect_err("the session is closed");
        assert_eq!(refused.code, concat_api::ErrorCode::NotOpen);
        release_on(third, &path).expect("an already-released folder is not an error");
        // The document is the whole edit: the close asked for no save,
        // and the project's own manifest is still on disk for the window
        // to open.
        assert!(std::path::Path::new(&path).join("concat.json").is_file());
    }
}
