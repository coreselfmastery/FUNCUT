// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The window's own open project: live mode (#337).
//!
//! A window that embeds this API - the Automation page's server - holds
//! one project itself, the one on screen. Until live mode, that project
//! was the one folder the API refused to touch: opening it answered
//! "the window holds it", and an agent - or the AUTO EDIT spine, or a
//! Director over the socket - was handed a sibling project to edit
//! blind, with the finished cut only appearing at the end.
//!
//! [`LiveProjectControl`] is the other design: the window installs a
//! bridge - see [`Api::share_live_project`] - and from then on every
//! project-scoped request for the folder the window holds is run
//! *through the window*: the edit applies on the window's own session,
//! lands on its undo stack, autosaves on its schedule, and the
//! timeline on screen changes while the caller watches. The project
//! stops being a file an agent rewrites and becomes the edit the user
//! is looking at - "a thing being made rather than a file that
//! appears".
//!
//! The protocol is small and closed on purpose: the operations are the
//! session-level halves of the API's own project methods, and each
//! carries its answer back as the type the API would have replied
//! with, so a caller cannot tell live mode from a project the API
//! holds itself. What stays the API's - the idempotency ring and its
//! durable ledger (F-05/F-06), the write roots, the recents list - is
//! kept there, around the bridge call, so a live edit answers under
//! the same key discipline an API-held one does.
//!
//! What live mode deliberately does not do:
//!
//! - `project.close` on the held project refuses: closing the user's
//!   editor from a socket is not an edit, and the refusal says so.
//! - `export.run` on the held project refuses the same way: the
//!   window's Export sheet owns the render. A timeline-ready run ends
//!   at the revised cut on the timeline, and the export - if one is
//!   wanted - is the user's own click.
//! - Everything that is not project-scoped - searches, probes,
//!   analysis, speech, motion bakes - never crosses the bridge: those
//!   read files, and the window autosaves after every edit, so the
//!   disk is as current as the screen.

use std::sync::Arc;

use concat_host::session::EditorView;
use concat_project::Command;
use concat_project::model::VideoSettings;

/// One operation on the project the window holds.
#[derive(Debug)]
pub enum LiveOp {
    /// The session's view - the same shape `project.get` and a fresh
    /// `project.open` answer with.
    View,
    /// The document, exactly as a save would write it.
    Document,
    /// Applies one edit command as the window's own hands would: the
    /// window's history, its autosave, its monitor and mix - the edit
    /// the user watches land. The command is the API's own
    /// [`Command`], unchanged.
    Apply {
        /// The edit. Boxed: a command is the largest thing a request
        /// carries, and the other operations are words.
        command: Box<Command>,
    },
    /// Steps the window's history back one edit.
    Undo,
    /// Steps the window's history forward one edit.
    Redo,
    /// Writes the document, as `project.save` does.
    Save {
        /// A new name for the project, when renaming.
        name: Option<String>,
    },
    /// Sets the active timeline's frame and rate, as an undoable edit.
    SetVideo {
        /// The frame and rate.
        video: VideoSettings,
    },
    /// The composited RGBA frame at `time`, `width` by `height` - the
    /// paused monitor's picture, titles and motion bakes included,
    /// read through the proxies where one serves the size whole.
    Frame {
        /// The instant on the timeline, in seconds.
        time: f64,
        /// The frame's width.
        width: u32,
        /// The frame's height.
        height: u32,
        /// Whether the window reads its proxies - a frame asked for
        /// small, the same call `preview.frame` makes of its own pool.
        proxy: bool,
    },
}

/// What the window answers one [`LiveOp`] with.
pub enum LiveReply {
    /// A view of the session as it stands.
    View(Box<EditorView>),
    /// The document, as a save would write it.
    Document(serde_json::Value),
    /// A rendered frame's bytes.
    Frame {
        /// The frame's width.
        width: u32,
        /// The frame's height.
        height: u32,
        /// The RGBA pixels, four bytes a pixel.
        pixels: Vec<u8>,
    },
    /// An operation with nothing to hand back: a save.
    Done,
}

/// The window's side of live mode. Implemented by the window around
/// its own session - the bridge hops to the window's event loop, the
/// way the playhead control does - and by tests around a session of
/// their own; the API neither knows nor cares which.
pub trait LiveBridge: Send + Sync {
    /// The project folder the window's session holds, if it holds one.
    /// The API compares this with the folder a request names, the same
    /// `key_of` way it compares its own, so a caller that spells the
    /// project differently - a symlink, a trailing slash - is still
    /// recognised. `None` - no project, the start page - makes every
    /// live route fall through to the API's own behaviour.
    fn path(&self) -> Option<String>;

    /// Runs one operation on that session. The error is the sentence
    /// the refusal comes back as - the bridge's own words, the way a
    /// command error is the window's.
    fn run(&self, op: LiveOp) -> Result<LiveReply, String>;
}

/// The control a window installs; see [`Api::share_live_project`].
/// Cheap to clone; every clone is the same bridge.
#[derive(Clone)]
pub struct LiveProjectControl(Arc<dyn LiveBridge>);

impl LiveProjectControl {
    /// Makes a control around `bridge`.
    pub fn new(bridge: impl LiveBridge + 'static) -> Self {
        LiveProjectControl(Arc::new(bridge))
    }

    /// The folder the window holds, if any.
    pub fn path(&self) -> Option<String> {
        self.0.path()
    }

    /// Runs one operation, as the window was asked to.
    pub fn run(&self, op: LiveOp) -> Result<LiveReply, String> {
        self.0.run(op)
    }

    /// The session's view, unwrapped: the reply every view-shaped
    /// operation hands the API.
    pub fn view(&self) -> Result<EditorView, String> {
        match self.run(LiveOp::View)? {
            LiveReply::View(view) => Ok(*view),
            _ => Err("the window answered a view-shaped ask without a view".to_owned()),
        }
    }
}
