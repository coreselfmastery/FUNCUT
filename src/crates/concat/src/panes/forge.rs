// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The library's Forge page (#12): an AI director for the project.
//!
//! The page hands the script and three choices to the `forge` harness
//! (<https://github.com/Adulari/forge>), which reads the funcut-director
//! skill - `.claude/skills/funcut-director/`, this repository - and drives
//! Concat through its MCP server: the window's own when Automation is on,
//! headless otherwise. The pane owns the child and its words: every line
//! the harness prints is posted to the log as it comes, the run ends by
//! its exit code, and a Cancel stops the child at its next line. Setup is
//! two buttons and two commands - `forge mcp add funcut concat -- --mcp
//! window -s user`, and `forge skill install` of this repository - both
//! run as they are, their exit codes reported honestly.
//!
//! The skill itself is the product: the pane only delivers a director who
//! has one. What makes the output professional lives in the skill's
//! playbooks, not here.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use concat_mcp::director::{
    AutoEditError, AutoEditEvent, AutoEditProgress, AutoEditState, EditProgram, Runner, Stage,
    ToolCall, ToolCaller, ToolError, auto_edit_program,
};
use slint::{ComponentHandle, SharedString};

use crate::i18n::{t, tf};
use crate::panes::Msg;
use crate::studio::Studio;

/// How many lines of the run the log keeps: the tail is what is read, and
/// a very long run must not grow a model without end.
const LOG_KEPT: usize = 150;

/// The voices the page's select offers, in its order: Kokoro's names, as
/// Settings > Speech shows them.
const VOICES: [&str; 5] = ["af_bella", "af_river", "am_adam", "am_fenrir", "am_onyx"];
/// The niche the director picks itself: no playbook is forced, the
/// creative brief and the material decide the look. Row 0 of the select.
const UNIVERSAL: &str = "Director decides";
/// The niches the page's select offers: the director's own choice first,
/// then the playbooks the skill carries as optional presets. A niche is
/// a starting point for a look, never a rail the edit must stay on.
const NICHES: [&str; 11] = [
    UNIVERSAL,
    "Science",
    "History",
    "Tech",
    "Luxury",
    "Motivation",
    "Finance",
    "Horror",
    "Fitness",
    "Gaming",
    "How-to",
];
/// Where a run's narration comes from: row 0 of the Source select - the
/// script box, read by TTS - or row 1 - the voiceover already on the
/// timeline, which the run builds a whole edit around without TTS.
const SOURCE_SCRIPT: i32 = 0;
const SOURCE_VOICEOVER: i32 = 1;
/// The music moods the page's select offers: bgm_search words.
const MOODS: [&str; 6] = [
    "ambient wonder",
    "documentary tension",
    "epic build",
    "synth pulse",
    "elegant strings",
    "lo-fi chill",
];

/// The runner's event stream as the pane shares it with the drain thread
/// each run spawns: a runner outlives a single run (a retryable end keeps
/// it, and the resume reuses it), so the receiver must too - one channel,
/// locked by whichever run is draining it now.
type AutoEditEvents = Arc<Mutex<std::sync::mpsc::Receiver<AutoEditEvent>>>;

/// Everything that can happen to the Forge page.
#[derive(Clone, Debug)]
pub enum ForgeMsg {
    /// Start: the script and the choices.
    Start {
        /// The words the voiceover will say, or - when the source is the
        /// timeline's own voiceover - extra directions for the director,
        /// which may be empty.
        script: String,
        /// The Source select's row: the script box read by TTS (0), or
        /// the voiceover already on the timeline (1).
        source: i32,
        /// [`VOICES`]'s row.
        voice: i32,
        /// [`NICHES`]'s row.
        niche: i32,
        /// [`MOODS`]'s row.
        mood: i32,
    },
    /// Cancel: the child stops at its next line.
    Cancel,
    /// The followed document changed on disk: the timeline re-reads it,
    /// so the edit is watched as it lands. Only the pane's own watcher
    /// sends this, and only while a run is going.
    LiveDocument,
    /// One line the harness printed.
    Line(String),
    /// The run ended, by its exit code.
    Ended(i32),
    /// A library pane turned to the page or away from it: showing it
    /// probes the setup, once, quietly.
    Shown(bool),
    /// The Connect button: `forge mcp add funcut concat -- --mcp window
    /// -s user`, after asking `forge mcp get funcut` whether it is needed.
    Connect,
    /// The probe or the add came back: wired, not wired, or why it could
    /// not be asked.
    Connected(Result<bool, String>),
    /// The Install button: `forge skill install` of this repository.
    Install,
    /// The install came back.
    Installed(Result<bool, String>),
    /// A provider's key was edited: kept in the settings file, injected
    /// into the harness's environment when a run starts.
    KeyEdited { provider: String, key: String },
    /// AUTO EDIT: run the unattended edit - the open project's voiceover
    /// to a measured, verified export, deterministically, with no agent.
    /// The runner (`concat-director`, through the
    /// `concat_mcp::director` seam) owns the stages, the progress, the
    /// cancellation with rollback and the retries. The first string is
    /// the script box's own text, when the page carried one: the run hands
    /// it to the voice reading, which aligns the transcript against the
    /// author's wording. The second is the orientation the page's pick
    /// named, when it named one: the sibling the run edits is born in
    /// that frame, so everything downstream - the stock the plan shops,
    /// the caption band, the motion canvas, the plan's own aspect -
    /// follows it; empty ("same as project") keeps the frame the
    /// project already has.
    AutoEditStart(String, String),
    /// AUTO EDIT: stop at the next stage boundary; the edits the run
    /// applied are rolled back, one `edit_undo` each.
    AutoEditCancel,
    /// The auto-edit's clock moved, or its stage did: reread the
    /// runner's last event into the page's fields.
    AutoEditTick,
    /// AUTO EDIT: a batch of the runner's events, drained off the
    /// window's thread. One message carries whatever arrived together -
    /// a fan's lanes answering at once are one wakeup, not five - and
    /// the pane reads each batch into its own fields: the stage word,
    /// the bar creeping through a stage's slot, and a line in the log
    /// per stage and per call.
    AutoEditEvents(Vec<AutoEditEvent>),
    /// The auto-edit ended. The state says which of the four ends the
    /// run met.
    AutoEditEnded(Box<AutoEditState>),
    /// The Director run AUTO EDIT hands the edit to has ended, by its
    /// exit code.
    AutoDirectorEnded(i32),
    /// The check the pane runs on a finished Director run came back: the
    /// receipt is the verdict, and the exit code is not.
    DirectorChecked(Box<concat_mcp::acceptance::DirectorAcceptance>),
    /// The post-run acceptance worker could not reach the editor's MCP backend.
    DirectorCheckFailed(String),
    /// The signal catalogue came back: the families the lab's buttons are
    /// built from, or why the catalogue could not be read.
    SignalCatalog(Result<Vec<SignalFamily>, String>),
    /// A family was chosen in the lab: its signals become the buttons.
    SignalFamily(usize),
    /// One signal button: extract that reading from the timeline's own
    /// voiceover. The row is the button's place in the chosen family.
    SignalExtract(i32),
    /// The reading came back: the signal's label, the reading's title
    /// and its body - or why it could not be taken.
    SignalRead(Result<(String, String), String>),
}

/// One family of the signal catalogue, as the lab shows it: the
/// family's own label and the readings it carries, each with the name
/// extraction asks for and the label its button says.
#[derive(Clone, Debug, Default)]
pub struct SignalFamily {
    /// The family's label, as the catalogue names it ("Forensics").
    pub label: String,
    /// Its signals: (the catalogue name, the button label).
    pub signals: Vec<(String, String)>,
}

/// The page's state.
#[derive(Default)]
pub struct ForgePane {
    /// A library is showing the page.
    pub shown: bool,
    /// A run is going.
    pub running: bool,
    /// What the run printed, oldest first, [`LOG_KEPT`] lines of it.
    pub log: Vec<SharedString>,
    /// One line of state, for the status field.
    pub status: String,
    /// Whether the harness is wired: unknown until probed.
    pub connected: Option<bool>,
    /// Whether the skill is installed: unknown until probed.
    pub installed: Option<bool>,
    /// The setup probe is in flight: a press that arrives before its
    /// answer waits on the same answer rather than asking the harness
    /// again - and never on the window's own thread.
    probing: bool,
    /// The project folder a voiceover run is editing, opened in the
    /// window when the run ends well: the edit lands on the timeline
    /// the user is looking at, which is the point of the run.
    result_path: Option<String>,
    /// How the child is stopped. The pane never holds the child: the
    /// reader thread owns it, and a Cancel only raises the flag it is
    /// checked against, one line away.
    kill: Arc<AtomicBool>,
    /// The AUTO EDIT's standing: what the progress pane shows, and the
    /// handles a cancel or a tick needs.
    pub auto: AutoEditPane,
    /// The voice-signal lab's standing: the catalogue, the chosen
    /// family, and the last reading a button took.
    pub signals: SignalsPane,
    /// The live follow's watcher: the flag that stops the thread polling
    /// the run's document. The follow itself lives on the studio; this is
    /// only the pane's way of saying "look again" while the run writes.
    live_kill: Option<Arc<AtomicBool>>,
}

/// The AUTO EDIT's standing on the page: the fields the progress pane
/// is filled from, and the handles that reach the run.
#[derive(Default)]
pub struct AutoEditPane {
    /// A run is going.
    pub running: bool,
    /// A Director process has exited, but its timeline is still being independently checked.
    /// Keep both editing entry points locked until that check has a terminal result.
    pub checking: bool,
    /// The stage's own word, localized - or the terminal state's.
    pub stage: String,
    /// The elapsed clock, as the pane spells it.
    pub elapsed: String,
    /// How far through the program's own stages the run is, 0 to 1. The
    /// stages the program does not carry are not counted: the fraction
    /// is of the work this run actually runs.
    pub fraction: f32,
    /// Whether the progress pane is shown: from the first run on, so the
    /// last outcome stays readable beside the button that offers to try
    /// again.
    pub visible: bool,
    /// 0 while idle or running; 1 when the run failed retryably (the
    /// button offers Retry); 2 when it failed hard (Fix and restart).
    pub outcome: i32,
    /// The run's handles: the runner (its state and its stop), the flag
    /// its tools poll, and the clock the elapsed line reads.
    runner: Option<Runner>,
    tool_cancel: Option<Arc<AtomicBool>>,
    /// Stops the independent acceptance pass after Forge itself has exited.
    check_cancel: Option<Arc<AtomicBool>>,
    started: Option<Instant>,
    /// The sibling this run edits: where the report is written, and the
    /// project that opens when the run ships.
    project: Option<String>,
    /// The source a retryable end keeps: the same sibling, the same
    /// program, the same runner - whose completed-stage record is the
    /// resume the next press of the button picks up from, instead of
    /// building a fresh sibling and paying for every stage again.
    source: Option<AutoEditSource>,
    /// The Director child's stop flag, when the run was handed over: the
    /// fixed spine's runner has its own cancel, and this is the other
    /// one's.
    kill: Option<Arc<AtomicBool>>,
    /// The runner's event stream, when a spine run holds the runner: the
    /// channel each run's drain thread reads, kept beside the runner a
    /// retryable end keeps for the resume.
    events: Option<AutoEditEvents>,
    /// The press that waits on the setup probe: the script, the
    /// orientation and the monetized word the button carried, held
    /// while the probe asks the harness off the window's thread. The
    /// start continues where the press left off once the answer is in -
    /// the button reads as a Stop the while, because the run has begun.
    pending: Option<(String, Option<String>, bool)>,
    /// The stage the events last named, and how many of its events have
    /// arrived beyond the stage's own beginning: the bar creeps toward
    /// the stage's end with each, never claiming it before the run's
    /// own accepted end does.
    stage_index: Option<usize>,
    stage_events: usize,
    /// Which of the Director skill's twenty-five phases the harness's
    /// own output named, one flag per phase. A handed-over run has no
    /// runner to receipt its stages, so the phases it printed are the
    /// only evidence each one ran - evidence the acceptance gate reads
    /// before the pane may call the edit finished. The fixed spine's
    /// runs leave it empty: their nineteen stages carry their own
    /// receipts, and the two systems are not confused.
    phases_seen: [bool; 25],
    /// A waiting job's own reading of its progress - the export's
    /// frames, as a fraction of a thousand - shared with the wait that
    /// reports it and read by the tick: the one piece of a run that
    /// knows how long its middle is.
    job: Option<Arc<AtomicU32>>,
}

/// The voice-signal lab's standing on the page: the catalogue its
/// buttons are built from, the family whose signals are showing, and
/// the last reading a button took.
#[derive(Default)]
pub struct SignalsPane {
    /// The catalogue's families, once loaded: every reading the profile
    /// answers, grouped as the catalogue groups it.
    pub families: Vec<SignalFamily>,
    /// The chosen family's row.
    pub family: usize,
    /// The catalogue is being read.
    pub loading: bool,
    /// A reading is being taken: the buttons wait their turn.
    pub busy: bool,
    /// The last reading's title - the signal's own label - or the
    /// sentence that says why there is none.
    pub title: String,
    /// The last reading's body: what it measures, why the edit wants
    /// it, the ear that answered, the confidence, and the values.
    pub body: String,
}

impl SignalsPane {
    /// The chosen family's signals as the page's buttons: one label per
    /// reading, in the catalogue's own order.
    pub fn rows(&self) -> Vec<SharedString> {
        self.families
            .get(self.family)
            .map(|family| {
                family
                    .signals
                    .iter()
                    .map(|(_, label)| SharedString::from(label.as_str()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The families as the page's select is filled: their own labels,
    /// in the catalogue's order.
    pub fn family_rows(&self) -> Vec<SharedString> {
        self.families
            .iter()
            .map(|family| SharedString::from(family.label.as_str()))
            .collect()
    }
}

fn auto_edit_stock_provider(pexels: &str, pixabay: &str) -> String {
    let env_has_key = |name: &str| std::env::var(name).is_ok_and(|value| !value.trim().is_empty());
    if !pexels.trim().is_empty() || env_has_key("PEXELS_API_KEY") {
        "pexels".to_owned()
    } else if !pixabay.trim().is_empty() || env_has_key("PIXABAY_API_KEY") {
        "pixabay".to_owned()
    } else {
        // Keep the call explicit: stock.search returns a named missing-key
        // refusal, and the warn-policy gather stage leaves the gap empty.
        "pexels".to_owned()
    }
}

impl ForgePane {
    /// Applies one message. The studio is the rest of the window; while
    /// this runs the studio's copy of the pane is a blank it must not read.
    pub fn update(&mut self, msg: ForgeMsg, studio: &mut Studio) {
        match msg {
            ForgeMsg::Shown(on) => {
                self.shown = on;
                // The first look probes the setup quietly: whether the
                // harness is there and wired, and whether the skill is.
                // A probe already in flight answers this look too.
                if on && self.connected.is_none() && !self.probing {
                    self.probe();
                }
                // The same first look reads the signal catalogue, quietly:
                // the seventy buttons the lab shows are built from it, and
                // it is a static table - no transcript, no audio, no
                // server - so the page is ready before a run ever starts.
                if on && self.signals.families.is_empty() && !self.signals.loading {
                    self.signals.loading = true;
                    crate::host::spawn(
                        || {
                            concat_mcp::sense::voice_signals_catalog()
                                .map(|reply| signal_families_of(&reply))
                                .map_err(|error| error.message)
                        },
                        |studio, _, _, families| {
                            studio.handle(Msg::Forge(ForgeMsg::SignalCatalog(families)));
                        },
                    );
                }
            }
            ForgeMsg::Start {
                script,
                source,
                voice,
                niche,
                mood,
            } => self.run(script, source, voice, niche, mood, studio),
            ForgeMsg::Cancel => {
                if self.running {
                    self.kill.store(true, Ordering::Relaxed);
                    self.set_status(t("forge.stopping"));
                }
            }
            ForgeMsg::Line(line) => {
                // A handed-over run has no runner to report a stage, so the
                // stage is read from the harness's own words as they come:
                // the phase names the director skill prints, mapped onto
                // the fraction of its twenty-five phases they sit at. A line
                // that names no phase moves nothing, and a line naming an
                // earlier phase than the one the run is in - a recap, a
                // quoted plan - moves it backwards neither: the bar only
                // ever advances on evidence.
                if self.auto.running
                    && self.auto.runner.is_none()
                    && let Some((phase, fraction)) = director_phase(&line)
                {
                    // The same match is the phase's evidence, kept for the
                    // acceptance gate: a phase the output never named is a
                    // phase the run cannot prove it ran.
                    if let Some(seen) = self.auto.phases_seen.get_mut(phase) {
                        *seen = true;
                    }
                    if fraction > self.auto.fraction {
                        self.auto.stage = director_phase_stage(phase);
                        self.auto.fraction = fraction;
                    }
                }
                // The handed-over run has no ticker of its own: its clock
                // is the harness's own output, which arrives as long as
                // the agent is speaking - and a multi-minute Director
                // run used to read 0s from start to finish.
                if self.auto.running
                    && let Some(started) = self.auto.started
                {
                    self.auto.elapsed = elapsed_text(started.elapsed());
                }
                self.log.push(line.into());
                let extra = self.log.len().saturating_sub(LOG_KEPT);
                self.log.drain(0..extra);
            }
            ForgeMsg::Ended(code) => {
                self.running = false;
                self.live_follow_stop();
                if self.kill.load(Ordering::Relaxed) {
                    self.set_status(t("forge.stopped"));
                    // The killed harness's claim on the sibling outlives the
                    // kill - a connection ending is not a project closing,
                    // so the server still holds the folder the agent opened
                    // - and nothing is coming back for that claim: the Start
                    // that may follow builds a fresh sibling beside the
                    // half-made one (`edit 2`, `edit 3`), never resuming this
                    // one. Without this release the stopped run's edit stays
                    // locked for the rest of the session, a folder the
                    // window refuses to open; the sibling itself is kept on
                    // disk, untouched - only the server's hold is let go.
                    if let Some(path) = self.result_path.clone()
                        && let Err(error) = concat_mcp::backend::release_window_session(&path)
                    {
                        log::warn!("forge: the run's session did not release: {error}");
                    }
                    // A stopped run keeps its sibling for the retry; the
                    // window goes back to the project it held.
                    studio.end_live_follow_back();
                } else if code == 0 {
                    self.set_status(t("forge.done"));
                    // A voiceover run's whole point is the edit landing on
                    // the timeline the user is looking at: the run edited
                    // a sibling project through the server (whose claim on
                    // it outlives the run), so the server lets it go and
                    // the finished edit opens here, in the window, now -
                    // from the live display straight to the real thing.
                    // The voiceover project itself is untouched on disk.
                    if let Some(path) = self.result_path.take() {
                        if let Err(error) = concat_mcp::backend::release_window_session(&path) {
                            log::warn!("forge: the run's session did not release: {error}");
                        }
                        // The display goes first: open_project's
                        // same-folder early return would otherwise leave
                        // the follow's read-only guard standing over the
                        // finished edit.
                        studio.land_live_follow();
                        match concat_host::projects::open(&path)
                            .and_then(|info| studio.open_project(info))
                        {
                            Ok(()) => {
                                studio.notify(&tf("forge.editReady", &[&path]), false);
                            }
                            Err(error) => {
                                studio.notify(&tf("forge.openFailed", &[&error]), true);
                            }
                        }
                    }
                } else {
                    self.set_status(tf("forge.exited", &[&code.to_string()]));
                    // A failed run's sibling is in the stopped run's
                    // position: the server still holds the folder the
                    // harness opened (an exit is a connection ending, not
                    // a project closing), and no retry is coming for the
                    // claim - a Start after a failed run builds a fresh
                    // sibling rather than resuming the half-made one. The
                    // claim is let go here, so the edit the failed run
                    // half-made stays one the window can open.
                    if let Some(path) = self.result_path.clone()
                        && let Err(error) = concat_mcp::backend::release_window_session(&path)
                    {
                        log::warn!("forge: the run's session did not release: {error}");
                    }
                    studio.end_live_follow_back();
                }
            }
            ForgeMsg::LiveDocument => {
                studio.refresh_live_follow();
            }
            ForgeMsg::Connect => {
                self.set_status(t("forge.connecting"));
                crate::host::spawn(connect_forge, |studio, _, _, result| {
                    studio.handle(Msg::Forge(ForgeMsg::Connected(result)));
                });
            }
            ForgeMsg::Connected(Ok(true)) => {
                self.connected = Some(true);
                self.set_status(t("forge.connected"));
                self.auto_edit_probe_answered(studio);
            }
            ForgeMsg::Connected(Ok(false)) => {
                self.connected = Some(false);
                self.set_status(t("forge.notConnected"));
                self.auto_edit_probe_answered(studio);
            }
            ForgeMsg::Connected(Err(error)) => {
                self.connected = Some(false);
                self.set_status(t("forge.notConnected"));
                studio.notify(&tf("forge.connectFailed", &[&error]), true);
                self.auto_edit_probe_answered(studio);
            }
            ForgeMsg::Install => {
                self.set_status(t("forge.installing"));
                crate::host::spawn(install_skill, |studio, _, _, result| {
                    studio.handle(Msg::Forge(ForgeMsg::Installed(result)));
                });
            }
            ForgeMsg::Installed(Ok(true)) => {
                self.installed = Some(true);
                self.set_status(t("forge.installedStatus"));
                self.auto_edit_probe_answered(studio);
            }
            ForgeMsg::Installed(Ok(false)) => {
                // The answer is remembered as a known no, the way the
                // connect arm remembers its own: a press waiting on the
                // probe would wait forever on a None, and the page draws
                // a known no and an unknown exactly the same.
                self.installed = Some(false);
                self.set_status(t("forge.installFailed"));
                self.auto_edit_probe_answered(studio);
            }
            ForgeMsg::Installed(Err(error)) => {
                // The same remembering, for a probe that could not ask:
                // the answer it owes is "not installed", not silence.
                self.installed = Some(false);
                self.set_status(t("forge.installFailed"));
                studio.notify(&tf("forge.installFailedDetail", &[&error]), true);
                self.auto_edit_probe_answered(studio);
            }
            ForgeMsg::KeyEdited { provider, key } => {
                let key = key.trim().to_owned();
                let forge = &mut studio.prefs.forge;
                match provider.as_str() {
                    "groq" => forge.groq = key,
                    "gemini" => forge.gemini = key,
                    "zai" => forge.zai = key,
                    "openrouter" => forge.openrouter = key,
                    "mistral" => forge.mistral = key,
                    "cerebras" => forge.cerebras = key,
                    "sambanova" => forge.sambanova = key,
                    "nim" => forge.nim = key,
                    "hf" => forge.hf = key,
                    _ => return,
                }
                studio.prefs.save(&studio.host.dirs);
            }
            ForgeMsg::AutoEditStart(script, orientation) => {
                self.auto_edit_start(script, orientation, studio)
            }
            ForgeMsg::AutoEditCancel => {
                if self.auto.running {
                    // A press still waiting on the setup probe has no run
                    // to stop - the wait itself is what ends, and no edit
                    // was ever made. The probe's answer, when it lands,
                    // only caches itself: the press it was for is gone.
                    if self.auto.pending.take().is_some() {
                        self.auto.running = false;
                        self.auto.checking = false;
                        self.auto.check_cancel = None;
                        self.auto.started = None;
                        self.auto.outcome = 0;
                        self.auto.fraction = 0.0;
                        self.auto.stage = t("forge.autoEditCancelledStage");
                        self.set_status(t("forge.autoEditCancelledStage"));
                        return;
                    }
                    if let Some(runner) = &self.auto.runner {
                        runner.cancel();
                    }
                    // A handed-over run has no runner: its stop is the
                    // flag the supervisor polls.
                    if let Some(kill) = &self.auto.kill {
                        kill.store(true, Ordering::Relaxed);
                    }
                    // The flag the tools themselves poll: a wait in
                    // flight gives up at its next look instead of
                    // running out its clock.
                    if let Some(flag) = &self.auto.tool_cancel {
                        flag.store(true, Ordering::Relaxed);
                    }
                    if let Some(flag) = &self.auto.check_cancel {
                        flag.store(true, Ordering::Relaxed);
                    }
                    self.set_status(t("forge.autoEditStopping"));
                }
            }
            ForgeMsg::AutoEditTick => self.auto_edit_tick(),
            ForgeMsg::AutoEditEvents(events) => self.auto_edit_events(events),
            ForgeMsg::AutoEditEnded(state) => self.auto_edit_ended(*state, studio),
            ForgeMsg::AutoDirectorEnded(code) => self.auto_director_ended(code, studio),
            ForgeMsg::DirectorChecked(checked) => self.auto_director_checked(*checked, studio),
            ForgeMsg::DirectorCheckFailed(error) => self.auto_director_check_failed(error, studio),
            ForgeMsg::SignalCatalog(families) => self.signal_catalog(families),
            ForgeMsg::SignalFamily(row) => {
                if !self.signals.families.is_empty() {
                    self.signals.family = row.min(self.signals.families.len() - 1);
                }
            }
            ForgeMsg::SignalExtract(row) => self.signal_extract(row, studio),
            ForgeMsg::SignalRead(reading) => self.signal_read(reading),
        }
    }

    /// A quiet look at the setup, on the first time the page is shown:
    /// is the harness wired, is the skill where it reads. The asking is
    /// two bounded harness commands on a worker - never on the window's
    /// own thread, which a slow harness would freeze for as long as its
    /// timeouts allow - and the answers cache themselves exactly as the
    /// Connect and Install buttons' own answers do.
    fn probe(&mut self) {
        self.probing = true;
        self.set_status(t("forge.probing"));
        crate::host::spawn(probe_setup, |studio, _, _, (connected, installed)| {
            studio.handle(Msg::Forge(ForgeMsg::Connected(connected)));
            studio.handle(Msg::Forge(ForgeMsg::Installed(installed)));
        });
    }

    /// The signal catalogue came back: the lab's families are its own,
    /// the first one's signals the first buttons. A catalogue that
    /// refused leaves the lab hidden and says why in the status line -
    /// the buttons never appear as if there were nothing to read.
    fn signal_catalog(&mut self, families: Result<Vec<SignalFamily>, String>) {
        self.signals.loading = false;
        match families {
            Ok(families) if !families.is_empty() => {
                self.signals.families = families;
                self.signals.family = 0;
            }
            Ok(_) => self.set_status(t("forge.signalsEmpty")),
            Err(why) => {
                self.set_status(tf("forge.signalsUnavailable", &[&why]));
            }
        }
    }

    /// One signal button: extract that reading from the timeline's own
    /// voiceover, off the window's thread. The reading is taken through
    /// the same toolbox and the same window server an agent's call
    /// means, so the button's answer is the director's answer; a take
    /// that was never transcribed is transcribed first, the way the
    /// AUTO EDIT's own program would.
    fn signal_extract(&mut self, row: i32, studio: &mut Studio) {
        if self.signals.busy || self.signals.loading {
            return;
        }
        // One edit at a time: the lab reads the open project's own
        // voiceover, and while a run is editing the same material the
        // reading would describe a take in motion. The refusals are the
        // lab's own words - one for each runner - not an entry point's
        // wording borrowed for a reading (and never a progress line's
        // {0} placeholder shown raw).
        if self.running {
            studio.notify(&t("forge.signalsAgentBusy"), true);
            return;
        }
        if self.auto.running || self.auto.checking {
            studio.notify(&t("forge.signalsAutoBusy"), true);
            return;
        }
        let row = row.max(0) as usize;
        let Some(family) = self.signals.families.get(self.signals.family) else {
            return;
        };
        let Some((name, label)) = family.signals.get(row) else {
            return;
        };
        let (name, label) = (name.clone(), label.clone());
        let Some(audio) = voiceover_file(studio) else {
            studio.notify(&t("forge.noVoiceover"), true);
            return;
        };
        self.signals.busy = true;
        self.set_status(tf("forge.signalsBusy", &[&label]));
        // Plain `spawn`, not `spawn_in_project`: the reading is of the
        // file the button was pressed on (its path is captured), and an
        // epoch-guarded delivery would DROP the reply when the project
        // changed mid-read - leaving `busy` true forever, every signal
        // button dead for the rest of the session. The reading arrives,
        // the card names its signal, and `signal_read` always clears
        // the flag.
        crate::host::spawn(
            move || read_one_signal(&audio, &name),
            move |studio, _, _, reading| {
                studio.handle(Msg::Forge(ForgeMsg::SignalRead(reading)));
            },
        );
    }

    /// The reading came back: the card carries the signal's own label
    /// over what it measured, why the edit wants it, the ear that
    /// answered and the values; a refusal says itself in the card's
    /// title, so the sentence stays beside the button that earned it.
    fn signal_read(&mut self, reading: Result<(String, String), String>) {
        self.signals.busy = false;
        match reading {
            Ok((title, body)) => {
                self.signals.title = title;
                self.signals.body = body;
                self.set_status(t("forge.signalsRead"));
            }
            Err(why) => {
                self.signals.title = t("forge.signalsFailed");
                self.signals.body = why;
                self.set_status(t("forge.signalsFailed"));
            }
        }
    }

    /// Starts a run: the prompt is the script and the choices, the
    /// harness is `forge run`, and its words come back one line at a time.
    ///
    /// A run's narration comes from one of two places. From the script
    /// box, read by the TTS voice the select names. Or from the voiceover
    /// already on the open timeline: a sibling project is made beside it,
    /// with the voiceover already reconstructed on its own lane at the
    /// original timing - every clip's start, in/out points, speed, volume
    /// and fades - and the director builds the whole edit around it
    /// there - the window holds the source project, and an agent cannot
    /// open what the window holds - so the finished edit can open back
    /// here, on the timeline, when the run ends well.
    fn run(
        &mut self,
        script: String,
        source: i32,
        voice: i32,
        niche: i32,
        mood: i32,
        studio: &mut Studio,
    ) {
        // Both rows are named, and a value that is neither row - one the
        // page never offers - reads as the script box: the choice that
        // has to carry its own words.
        let from_timeline = match source {
            SOURCE_SCRIPT => false,
            SOURCE_VOICEOVER => true,
            _ => false,
        };
        if self.running {
            return;
        }
        // The AUTO EDIT and the agent run edit the same sibling project
        // through the same server. Two writers on one project interleave
        // their edits into a timeline neither of them planned, so only
        // one may be going: a Start while the auto edit runs is refused,
        // and the user is told which run to stop first.
        if let Some(refusal) =
            other_run_refusal(auto_edit_busy(self.auto.running, self.auto.checking), true)
        {
            studio.notify(&refusal, true);
            return;
        }
        if !from_timeline && script.trim().is_empty() {
            return;
        }
        let mut voiceover = None;
        if from_timeline {
            match voiceover_project(studio, false) {
                Ok((made, _)) => voiceover = Some(made),
                Err(error) => {
                    studio.notify(&error, true);
                    return;
                }
            }
        }
        let voice = VOICES
            .get(voice.max(0) as usize)
            .copied()
            .unwrap_or(VOICES[0]);
        let niche = NICHES
            .get(niche.max(0) as usize)
            .copied()
            .unwrap_or(NICHES[0]);
        let mood = MOODS.get(mood.max(0) as usize).copied().unwrap_or(MOODS[0]);
        self.result_path = voiceover.as_ref().map(|made| made.project.clone());
        // The prompt is built while the window still holds the SOURCE
        // project: its creative brief is the run's, and the follow below
        // swaps the timeline to the sibling.
        let prompt = director_run_prompt(&script, voiceover.as_ref(), voice, niche, mood, studio);
        // The window follows the edit live: the timeline becomes the
        // sibling's while the director works on it, refreshed as the
        // run's own autosaves land - the edit is watched as it happens,
        // not discovered when it ends.
        if let Some(made) = voiceover.as_ref() {
            self.live_follow_start(&made.project, studio);
        }
        self.kill = Arc::new(AtomicBool::new(false));
        let kill = Arc::clone(&self.kill);
        let mut command = Command::new("forge");
        command.arg("run");
        // Forge's default router optimises for cost. A creative director
        // needs the model the user chose, which is usually a vision-capable
        // one: the quality loop below returns images. Both spellings are
        // accepted so a setting made for the harness works unchanged.
        let model = std::env::var("FUNCUT_FORGE_MODEL")
            .or_else(|_| std::env::var("FORGE_MODEL"))
            .unwrap_or_default();
        if !model.trim().is_empty() {
            command.arg("--model").arg(model.trim());
        }
        command.arg(prompt);
        // The provider keys the page keeps are handed to the harness as
        // its environment, in each provider's own spelling, so the free
        // tiers work without leaving the editor. Keys stacked in the
        // harness's own keyring arrive on their own. The HuggingFace key
        // is both spellings the emotion sidecar reads: with it, the
        // director's `emotion` tool runs on the multilingual
        // tabularisai/multilingual-emotion-classification model instead of
        // the English-only lexicon.
        let forge = &studio.prefs.forge;
        let stock = &studio.prefs.stock;
        for (name, value) in [
            ("GROQ_API_KEY", &forge.groq),
            ("GEMINI_API_KEY", &forge.gemini),
            ("ZAI_API_KEY", &forge.zai),
            ("OPENROUTER_API_KEY", &forge.openrouter),
            ("MISTRAL_API_KEY", &forge.mistral),
            ("CEREBRAS_API_KEY", &forge.cerebras),
            ("SAMBANOVA_API_KEY", &forge.sambanova),
            ("NVIDIA_API_KEY", &forge.nim),
            ("FUNCUT_EMOTION_API_KEY", &forge.hf),
            ("HF_TOKEN", &forge.hf),
            // The live stock shelves' keys ride the same road as the
            // provider keys: a Pexels or Pixabay key typed once in the
            // Stock page reaches the Director's gather stage here, so the
            // Forge Director launch hands the harness the same stock
            // environment the AUTO EDIT launch does.
            ("PEXELS_API_KEY", &stock.pexels),
            ("PIXABAY_API_KEY", &stock.pixabay),
        ] {
            if !value.trim().is_empty() {
                command.env(name, value.trim());
            }
        }
        configure_forge_process_group(&mut command);
        let spawned = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        match spawned {
            Ok(child) => {
                self.running = true;
                self.log.clear();
                self.set_status(t("forge.running"));
                read_the_run(child, kill);
            }
            Err(error) => {
                self.connected = Some(false);
                self.set_status(t("forge.notFound"));
                studio.notify(&tf("forge.notFoundDetail", &[&error.to_string()]), true);
                // A run that never started edits nothing: the follow it
                // began goes back to the project the window held.
                self.live_follow_stop();
                studio.end_live_follow_back();
            }
        }
    }

    /// The log as the page's model is filled with it.
    pub fn log_lines(&self) -> Vec<SharedString> {
        self.log.clone()
    }

    fn set_status(&mut self, line: String) {
        self.status = line;
    }

    /// Whether AUTO EDIT can run at all: a project is open and its
    /// timeline carries narration - an audio clip that is not the bed.
    /// The button is greyed until it is, and the take itself is the same
    /// test the sibling build makes.
    pub fn auto_edit_available(&self, studio: &Studio) -> bool {
        let Some(session) = studio.session.as_ref() else {
            return false;
        };
        let root = std::path::Path::new(session.path());
        let project = studio.project();
        let clips: Vec<&concat_project::model::Clip> = project
            .active()
            .clips
            .iter()
            .map(|clip| clip.as_ref())
            .collect();
        clips.iter().any(|clip| is_narration(clip, project, root))
            || (clips.iter().all(|clip| !is_narration(clip, project, root))
                && clips
                    .iter()
                    .any(|clip| is_talking_head(clip, project, root)))
    }

    /// Starts an AUTO EDIT: the entry point the button reaches. The
    /// sibling is built exactly as a forge run's is - the same handoff,
    /// the same voiceover lane reconstruction - and then either the
    /// Director agent or the deterministic spine of the pipeline runs
    /// over it ([`Self::auto_edit_run`]). One thing can hold the press
    /// before any of that: the setup's answers, without which the run
    /// cannot know whether the Director may take the edit at all - the
    /// probe for them runs off the window's thread and the press waits
    /// as the run's first visible stage, where asking synchronously
    /// froze the window for as long as the harness took to answer.
    fn auto_edit_start(&mut self, script: String, orientation: String, studio: &mut Studio) {
        // The page's pick arrives as a word, the empty word meaning "the
        // project's own frame": an empty pick is no pick at all.
        let trimmed = orientation.trim().to_owned();
        let orientation = (!trimmed.is_empty()).then_some(trimmed);
        if auto_edit_busy(self.auto.running, self.auto.checking) {
            studio.notify(&t("forge.autoEditStillBusy"), true);
            return;
        }
        // A Start and an AUTO EDIT share the same sibling project and
        // server: while the agent run is going, the auto edit would write
        // over the run it cannot see. Refuse it with the same sentence the
        // Start uses, so the two entry points speak with one voice.
        if let Some(refusal) = other_run_refusal(self.running, false) {
            studio.notify(&refusal, true);
            return;
        }
        // The licence word the page's own switch carries is read at the
        // same moment the page reads its script box: the choice belongs
        // to the press, and travels with it.
        let monetized = monetized_choice();
        // The button is the one entry point that must not fall back
        // silently. The pane's probe only ran if the Forge page was
        // shown once, so a user who never opened the tab would be
        // handed the fixed spine without the Director ever being asked
        // for - the exact mistake the fallback toast was written to
        // prevent. The answer is asked for off the window's thread -
        // the same two bounded harness commands the page's own quiet
        // probe runs - and the press becomes the run's first visible
        // stage while it flies: the fields are held, the button reads
        // as a Stop, and the start continues the moment the answer
        // lands, its checks already stood.
        if self.connected.is_none() || self.installed.is_none() {
            self.auto.pending = Some((script, orientation, monetized));
            self.auto.running = true;
            self.auto.checking = false;
            self.auto.stage = t("forge.probing");
            self.auto.elapsed = elapsed_text(Duration::ZERO);
            self.auto.fraction = 0.0;
            self.auto.visible = true;
            self.auto.outcome = 0;
            self.auto.stage_index = None;
            self.auto.stage_events = 0;
            self.auto.phases_seen = [false; 25];
            self.auto.started = Some(Instant::now());
            self.set_status(t("forge.probing"));
            if !self.probing {
                self.probe();
            }
            return;
        }
        self.auto_edit_run(script, orientation, monetized, studio);
    }

    /// The probe a pressed AUTO EDIT waited on has answered, or one of
    /// the two answers it needed has: when both the wiring and the skill
    /// are known, the held press continues where it left off - the
    /// run's own stages take the state from there, exactly as a press
    /// that found the answers already cached does.
    fn auto_edit_probe_answered(&mut self, studio: &mut Studio) {
        // Both answers in means no probe can be in flight; a later one
        // would only cache what is already cached.
        if self.connected.is_some() && self.installed.is_some() {
            self.probing = false;
        }
        if self.auto.pending.is_none() || self.connected.is_none() || self.installed.is_none() {
            return;
        }
        let (script, orientation, monetized) = self
            .auto
            .pending
            .take()
            .expect("the pending press was checked above");
        self.auto_edit_run(script, orientation, monetized, studio);
    }

    /// The AUTO EDIT's run itself, once the setup's answers are known:
    /// either the Director takes the sibling - the same handover a
    /// failed Director run retries - or the deterministic spine of the
    /// pipeline runs over the window's own server: the repair,
    /// readings, tighten, grade, licensed live-stock B-roll, captions,
    /// title entrances, music, preview, revise, save the timeline,
    /// verify and watch. The plan's creative brief, the eye-picked
    /// stock and the baked motion comps remain the Director agent's;
    /// this is the unattended voiceover edit. `monetized` is the
    /// licence word the press carried: it rides the source, so the
    /// Director's prompt and the spine's program answer one choice.
    fn auto_edit_run(
        &mut self,
        script: String,
        orientation: Option<String>,
        monetized: bool,
        studio: &mut Studio,
    ) {
        // A Director run has no deterministic Runner to resume. Its retry
        // must instead reopen the exact sibling it already edited; building
        // another sibling here would discard the previous creative work.
        if self.auto.runner.is_none()
            && let Some(source) = self.auto.source.clone()
            && director_retry_available(&source)
        {
            if let Err(why) = director_ready(self.connected, self.installed, studio) {
                studio.notify(&why, true);
                return;
            }
            let kill = Arc::new(AtomicBool::new(false));
            self.auto.running = true;
            self.auto.checking = false;
            self.auto.stage = t("forge.autoEditDirectorStage");
            self.auto.elapsed = elapsed_text(Duration::ZERO);
            self.auto.fraction = 0.0;
            self.auto.visible = true;
            self.auto.outcome = 0;
            self.auto.check_cancel = None;
            self.auto.phases_seen = [false; 25];
            self.auto.started = Some(Instant::now());
            self.auto.project = Some(source.project.clone());
            self.auto.source = Some(source.clone());
            self.auto.kill = Some(Arc::clone(&kill));
            self.set_status(t("forge.autoEditDirectorStage"));
            self.live_follow_start(&source.project, studio);
            launch_auto_director(studio, &source, kill);
            return;
        }
        // A retryable end kept its sibling, its source and its runner:
        // the resume picks up after the stages the failed run completed
        // (IDEM), instead of building a fresh sibling and paying for the
        // repair, the transcription and every reading again.
        if let (Some(runner), Some(source), Some(events)) = (
            self.auto.runner.clone(),
            self.auto.source.clone(),
            self.auto.events.clone(),
        ) && source
            .takes
            .iter()
            .all(|take| std::path::Path::new(&take.take).exists())
            && std::path::Path::new(&source.project)
                .join("concat.json")
                .exists()
        {
            // The prior run's unread tail - events emitted between the
            // last batch its drain thread took and its end - is dropped
            // here, before this run's threads exist: its terminal state
            // was reported through AutoEditEnded, and a stale stage index
            // must not move this run's bar.
            while events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .try_recv()
                .is_ok()
            {}
            let backend = match concat_mcp::backend::open(concat_mcp::backend::Mode::Window) {
                Ok(backend) => backend,
                Err(error) => {
                    studio.notify(&tf("forge.autoEditNeedServer", &[&error]), true);
                    return;
                }
            };
            let program = auto_edit_program(&concat_mcp::director::AutoEditProgramSource {
                project: source.project.clone(),
                takes: source
                    .takes
                    .iter()
                    .map(|take| concat_mcp::director::TakePaths {
                        take: take.take.clone(),
                        enhanced: take.enhanced.clone(),
                        aroll: take.aroll.clone(),
                    })
                    .collect(),
                placements: source
                    .placements
                    .iter()
                    .map(|placed| concat_mcp::director::TakePlacement {
                        take: placed.take.clone(),
                        start: placed.start,
                        source_in: placed.source_in,
                        source_out: placed.source_out,
                        speed: placed.speed,
                    })
                    .collect(),
                export: source.export.clone(),
                sheet: source.sheet.clone(),
                stock_provider: auto_edit_stock_provider(
                    &studio.prefs.stock.pexels,
                    &studio.prefs.stock.pixabay,
                ),
                script: source.script.clone(),
                // The sibling this resume reopens was born in the frame it
                // was born in: no frame-set is wanted.
                orientation: None,
                frame_rate: None,
                // The licence word the pressed switch carried when this
                // sibling's run began: the source kept it, because the
                // plan stage that spends it is the one a resume skips -
                // the brief the prior run wrote is the one this run ships.
                monetized: source.monetized,
            });
            let tool_cancel = Arc::new(AtomicBool::new(false));
            let job = Arc::new(AtomicU32::new(0));
            let project = source.project.clone();
            let project_for_follow = project.clone();
            self.auto = AutoEditPane {
                running: true,
                checking: false,
                stage: t("forge.autoEditStarting"),
                elapsed: elapsed_text(Duration::ZERO),
                fraction: 0.0,
                visible: true,
                outcome: 0,
                runner: Some(runner.clone()),
                tool_cancel: Some(Arc::clone(&tool_cancel)),
                check_cancel: None,
                kill: None,
                started: Some(Instant::now()),
                project: Some(project),
                source: Some(source),
                events: Some(Arc::clone(&events)),
                pending: None,
                stage_index: None,
                stage_events: 0,
                phases_seen: [false; 25],
                job: Some(Arc::clone(&job)),
            };
            self.set_status(t("forge.autoEditRunning"));
            self.live_follow_start(&project_for_follow, studio);
            run_auto_edit(runner, program, backend, tool_cancel, events, job);
            return;
        }
        let Some(session) = studio.session.as_ref() else {
            studio.notify(&t("forge.needProject"), true);
            return;
        };
        let built = build_voiceover_sibling(
            studio.project(),
            std::path::Path::new(session.path()),
            true,
            orientation.as_deref(),
        );
        let (_, auto) = match built {
            Ok(built) => built,
            Err(error) => {
                studio.notify(&error, true);
                return;
            }
        };
        let Some(mut auto) = auto else {
            return;
        };
        // The script the button carried is the run's own authority on
        // wording: it travels with the source (the Director's prompt and
        // the deterministic spine's voice reading both read it), so a
        // run whose voiceover was read from a script shows the author's
        // words, not the recogniser's guesses. An empty box leaves the
        // transcript as it was transcribed.
        if !script.trim().is_empty() {
            auto.script = Some(script);
        }
        // The licence word the switch carried is the run's own, the way
        // the script is: it rides the source - the Director's prompt and
        // the spine's program both read it - so the choice the press made
        // stands for the retry too, and the report's reader can see what
        // regime the edit was made under.
        auto.monetized = monetized;
        // The keys the Forge page keeps are the environment of the
        // in-process tool calls too, not only the spawned harness: with a
        // hosted critic key saved here, the program's watch stage gets
        // the strong VLM tier and the verify stage's emotion read runs on
        // the multilingual model - all without the user exporting a
        // single variable. An exported variable always wins: the editor
        // never overrides the environment it was started with.
        let forge = &studio.prefs.forge;
        for (name, value) in [
            ("ZAI_API_KEY", &forge.zai),
            ("GROQ_API_KEY", &forge.groq),
            ("GEMINI_API_KEY", &forge.gemini),
            ("OPENROUTER_API_KEY", &forge.openrouter),
            ("FUNCUT_EMOTION_API_KEY", &forge.hf),
            ("HF_TOKEN", &forge.hf),
        ] {
            if !value.trim().is_empty() && std::env::var_os(name).is_none() {
                // SAFETY: the editor is single-window and this runs on the
                // UI thread before the run's worker threads are spawned;
                // the same pattern `--software` rendering already uses.
                unsafe { std::env::set_var(name, value.trim()) };
            }
        }
        // The live stock keys are installed before a new runner is
        // spawned. A retry stays in this process and reuses this run's
        // environment, so it does not mutate the global environment again.
        let stock = &studio.prefs.stock;
        for (name, value) in [
            ("PEXELS_API_KEY", &stock.pexels),
            ("PIXABAY_API_KEY", &stock.pixabay),
        ] {
            let exported = std::env::var(name)
                .ok()
                .is_some_and(|existing| !existing.trim().is_empty());
            if !value.trim().is_empty() && !exported {
                // SAFETY: this is the existing UI-thread handoff before
                // the AUTO EDIT worker is spawned.
                unsafe { std::env::set_var(name, value.trim()) };
            }
        }
        let program = auto_edit_program(&concat_mcp::director::AutoEditProgramSource {
            project: auto.project.clone(),
            takes: auto
                .takes
                .iter()
                .map(|take| concat_mcp::director::TakePaths {
                    take: take.take.clone(),
                    enhanced: take.enhanced.clone(),
                    aroll: take.aroll.clone(),
                })
                .collect(),
            placements: auto
                .placements
                .iter()
                .map(|placed| concat_mcp::director::TakePlacement {
                    take: placed.take.clone(),
                    start: placed.start,
                    source_in: placed.source_in,
                    source_out: placed.source_out,
                    speed: placed.speed,
                })
                .collect(),
            export: auto.export.clone(),
            sheet: auto.sheet.clone(),
            stock_provider: auto_edit_stock_provider(
                &studio.prefs.stock.pexels,
                &studio.prefs.stock.pixabay,
            ),
            script: auto.script.clone(),
            // The sibling was born in the picked frame (or the source's
            // own, when nothing was picked): the handoff needs no set.
            orientation: None,
            frame_rate: None,
            // The licence word the pressed switch carried: it rules the
            // non-commercial beds - the music model's, the free shelves -
            // out of a monetized edit, and opens them to one that is not.
            monetized: auto.monetized,
        });
        // The Director gets the edit whenever the page has a model to run
        // it: AUTO EDIT is the one-click form of Start Forge, so the
        // creative decisions belong to the same agent, over the same
        // sibling. Only when the harness, its wiring, the skill or a key
        // is missing does the run fall back to the fixed spine - and the
        // pane says which one is missing, so a lesser edit is never
        // mistaken for the Director's.
        // The stock preflight: a run with no live-shelf key would meet
        // the refusal one refused search at a time, minutes in. Said
        // here, before a single stage starts, the user can key the shelf
        // and press again - or let the run fill the picture with the
        // project's own footage and typography, which is what it will do.
        if !stock_shelves_ready(studio) {
            studio.notify_warning(&t("forge.autoEditNoStockKey"));
        }
        let mut director_fallback: Option<String> = None;
        let handed_over = match director_ready(self.connected, self.installed, studio) {
            Ok(()) => {
                let kill = Arc::new(AtomicBool::new(false));
                self.auto = AutoEditPane {
                    running: true,
                    checking: false,
                    stage: t("forge.autoEditDirectorStage"),
                    elapsed: elapsed_text(Duration::ZERO),
                    fraction: 0.0,
                    visible: true,
                    outcome: 0,
                    runner: None,
                    tool_cancel: None,
                    check_cancel: None,
                    started: Some(Instant::now()),
                    project: Some(auto.project.clone()),
                    source: Some(auto.clone()),
                    kill: Some(Arc::clone(&kill)),
                    events: None,
                    pending: None,
                    stage_index: None,
                    stage_events: 0,
                    phases_seen: [false; 25],
                    job: None,
                };
                self.set_status(t("forge.autoEditDirectorStage"));
                self.live_follow_start(&auto.project, studio);
                launch_auto_director(studio, &auto, kill);
                true
            }
            Err(why) => {
                studio.notify(&tf("forge.autoEditDirectorFallback", &[&why]), true);
                // The reason rides the source into the report: a missed
                // toast is not a lost explanation.
                director_fallback = Some(why.clone());
                false
            }
        };
        if handed_over {
            return;
        }
        auto.director = director_fallback;
        // The fixed spine runs the stages in this process, so it needs the
        // window's own server as its backend. The handover above does not:
        // the Director reaches the same server through the harness, which
        // is why it is decided first.
        let backend = match concat_mcp::backend::open(concat_mcp::backend::Mode::Window) {
            Ok(backend) => backend,
            Err(error) => {
                studio.notify(&tf("forge.autoEditNeedServer", &[&error]), true);
                return;
            }
        };
        // The runner is born with its event channel, and the pane keeps
        // the receiving end: the drain thread each run spawns reads it,
        // and a retryable end keeps it with the runner for the resume.
        let (runner, events) = Runner::new(PathBuf::from(&auto.project));
        let events = Arc::new(Mutex::new(events));
        let tool_cancel = Arc::new(AtomicBool::new(false));
        let job = Arc::new(AtomicU32::new(0));
        let follow = auto.project.clone();
        self.auto = AutoEditPane {
            running: true,
            checking: false,
            stage: t("forge.autoEditStarting"),
            elapsed: elapsed_text(Duration::ZERO),
            fraction: 0.0,
            visible: true,
            outcome: 0,
            runner: Some(runner.clone()),
            tool_cancel: Some(Arc::clone(&tool_cancel)),
            check_cancel: None,
            started: Some(Instant::now()),
            project: Some(auto.project.clone()),
            source: Some(auto),
            kill: None,
            events: Some(Arc::clone(&events)),
            pending: None,
            stage_index: None,
            stage_events: 0,
            job: Some(Arc::clone(&job)),
        };
        self.set_status(t("forge.autoEditRunning"));
        self.live_follow_start(&follow, studio);
        run_auto_edit(runner, program, backend, tool_cancel, events, job);
    }

    /// The Director run's end. A zero exit is the harness saying the
    /// agent finished; the edit it made is opened here in the window, the
    /// same landing a Start Forge run has, and the pane reports the run
    /// rather than a measured timeline - the Director's own craft_critique
    /// and its final watch_render are what judged the file, and the pane
    /// does not invent a score of its own.
    fn auto_director_ended(&mut self, code: i32, studio: &mut Studio) {
        // A cancelled run is not a failed one: the button killed the
        // harness, and the honest report is the stop. The sibling and
        // its source stay for the retry, exactly as the fixed spine's
        // cancelled run keeps them.
        let cancelled = self
            .auto
            .kill
            .as_ref()
            .is_some_and(|kill| kill.load(Ordering::Relaxed));
        self.auto.started = None;
        self.auto.kill = None;
        if cancelled {
            self.auto.running = false;
            self.auto.checking = false;
            self.auto.check_cancel = None;
            self.auto.outcome = 1;
            self.auto.stage = t("forge.autoEditCancelledStage");
            self.set_status(t("forge.autoEditCancelledStage"));
            studio.notify(&t("forge.autoEditCancelled"), true);
            self.live_follow_stop();
            // The sibling and its source stay for the retry, but the
            // server's claim need not stay with them: the Director's
            // retry re-opens the project through the server as its
            // first act, so letting go here loses nothing of the
            // resume - and held, the half-made edit stays one the
            // window cannot open for the rest of the session.
            if let Some(path) = self.auto.project.clone()
                && let Err(error) = concat_mcp::backend::release_window_session(&path)
            {
                log::warn!("forge: the run's session did not release: {error}");
            }
            studio.end_live_follow_back();
            // The sibling stays for the retry, but the Director's hold on
            // it does not: the handover opened it through the window's
            // own server, and a session that outlives its Director
            // leaves the sibling unopenable by hand until the retry
            // succeeds or the app restarts.
            if let Some(path) = self.auto.project.clone()
                && let Err(error) = concat_mcp::backend::release_window_session(&path)
            {
                log::warn!("forge: the run's session did not release: {error}");
            }
            return;
        }
        if code == 0 {
            // Keep the UI's running state during acceptance so the same
            // Stop button can cancel the remaining independent checks.
            self.auto.running = true;
            // The agent has exited, but the timeline is not accepted yet. Keep
            // the project locked while the independent checks run: otherwise
            // a second Start/AUTO EDIT can replace the active project and the
            // first run's receipt can be written beside the wrong project.
            self.auto.checking = true;
            let check_cancel = Arc::new(AtomicBool::new(false));
            self.auto.check_cancel = Some(Arc::clone(&check_cancel));
            // The Director phases have ended, but independent acceptance is
            // still outstanding. Do not display 100% until every required
            // timeline gate has actually passed.
            self.auto.fraction = director_acceptance_fraction(false);
            // The harness exiting zero says the agent stopped, not that the
            // edit exists. The Director's deliverable is the saved timeline,
            // not a render: the acceptance reads the open project, judges
            // its composited picture and checks the lanes. An export is
            // optional and only adds the encoded-file gates.
            self.auto.stage = t("forge.autoEditCheckingStage");
            self.set_status(t("forge.autoEditCheckingStage"));
            let Some(project) = self.auto.project.clone() else {
                self.auto.checking = false;
                self.auto.running = false;
                self.auto.check_cancel = None;
                self.auto.outcome = 2;
                self.auto.stage = t("forge.autoEditDirectorFailedStage");
                self.set_status(t("forge.autoEditDirectorFailedStage"));
                studio.notify(&t("forge.autoEditDirectorNoProject"), true);
                self.live_follow_stop();
                studio.end_live_follow_back();
                if let Some(path) = self.auto.project.clone()
                    && let Err(error) = concat_mcp::backend::release_window_session(&path)
                {
                    log::warn!("forge: the run's session did not release: {error}");
                }
                return;
            };
            let export = self
                .auto
                .source
                .as_ref()
                .map(|source| source.export.clone())
                .filter(|export| {
                    !export.trim().is_empty() && std::path::Path::new(export).is_file()
                });
            // The phases the harness's own output named, one flag per
            // phase of the skill's twenty-five: the only evidence a
            // handed-over run has that each phase ran, now the
            // acceptance's to weigh. Copied before the thread takes the
            // closure, because the pane's state is not Send.
            let phases_seen = self.auto.phases_seen;
            crate::host::spawn_detached(move || {
                let checked = match concat_mcp::backend::open(concat_mcp::backend::Mode::Window) {
                    Ok(mut backend) => {
                        let toolbox = concat_mcp::tools::Toolbox::new();
                        let check = concat_mcp::acceptance::DirectorCheck {
                            project: &project,
                            export: export.as_deref(),
                            phases: Some(phases_seen),
                        };
                        concat_mcp::acceptance::check(
                            &check,
                            &toolbox,
                            &mut *backend,
                            &check_cancel,
                        )
                    }
                    // The window's server is how the check reaches the
                    // tools; without it there is no measurement, and an
                    // unmeasured run is not a finished one. Only the
                    // backend's own words ride the message: the pane's
                    // wording for this landing is the localized sentence
                    // `auto_director_check_failed` shows, and an English
                    // prefix written here would reach the user raw inside
                    // that sentence's {0}.
                    Err(error) => {
                        let message = error;
                        crate::host::on_ui(move |studio, _, _| {
                            studio.handle(Msg::Forge(ForgeMsg::DirectorCheckFailed(message)));
                        });
                        return;
                    }
                };
                crate::host::on_ui(move |studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::DirectorChecked(Box::new(checked))));
                });
            });
            return;
        }
        // A handover that failed keeps the sibling and the source, so a
        // retry hands the same project over again rather than rebuilding
        // it; the Director's own reads are its to redo.
        self.auto.running = false;
        self.auto.checking = false;
        self.auto.check_cancel = None;
        self.auto.outcome = 1;
        self.auto.stage = t("forge.autoEditDirectorFailedStage");
        self.set_status(t("forge.autoEditDirectorFailedStage"));
        studio.notify(
            &tf("forge.autoEditDirectorFailed", &[&code.to_string()]),
            true,
        );
        self.live_follow_stop();
        studio.end_live_follow_back();
        // The sibling stays for the retry, but the Director's hold on it
        // does not: the handover opened it through the window's own
        // server, and a session that outlives its Director leaves the
        // sibling unopenable by hand - refused as open through a remote
        // connection - until a retry succeeds or the app restarts.
        if let Some(path) = self.auto.project.clone()
            && let Err(error) = concat_mcp::backend::release_window_session(&path)
        {
            log::warn!("forge: the run's session did not release: {error}");
        }
    }

    /// The measured check came back: the receipt decides whether the run
    /// is a finished edit or a recorded refusal. Either way the receipt is
    /// written beside the project, so the answer is inspectable rather than
    /// a toast that faded.
    fn auto_director_checked(
        &mut self,
        checked: concat_mcp::acceptance::DirectorAcceptance,
        studio: &mut Studio,
    ) {
        self.auto.checking = false;
        self.auto.running = false;
        self.auto.check_cancel = None;
        let accepted = checked.accepted();
        self.auto.fraction = director_acceptance_fraction(accepted);
        let receipt = checked.receipt;
        if let Some(project) = self.auto.project.clone() {
            let path = std::path::Path::new(&project).join("auto-edit-receipt.json");
            match serde_json::to_vec_pretty(&receipt.to_json()) {
                Ok(bytes) => {
                    if let Err(error) = std::fs::write(&path, bytes) {
                        log::warn!("the acceptance receipt cannot be written: {error}");
                    }
                }
                Err(error) => log::warn!("the acceptance receipt cannot be written: {error}"),
            }
        }
        let summary = receipt.summary();
        self.log
            .push(tf("forge.autoEditAcceptance", &[&summary]).into());
        let extra = self.log.len().saturating_sub(LOG_KEPT);
        self.log.drain(0..extra);
        if accepted {
            self.auto.outcome = 0;
            self.auto.stage = t("forge.autoEditDirectorDoneStage");
            self.set_status(t("forge.autoEditDirectorDoneStage"));
            self.live_follow_stop();
            if let Some(path) = self.auto.project.clone() {
                studio.notify(&tf("forge.autoEditDirectorDone", &[&path]), false);
                // The server held the sibling for the run and the check;
                // the window cannot open what it holds, so it lets go
                // first - the edit itself is on disk, autosaved edit by
                // autosaved edit.
                if let Err(error) = concat_mcp::backend::release_window_session(&path) {
                    log::warn!("forge: the run's session did not release: {error}");
                }
                // The display goes first, or the landing's open reads
                // as the display it already is and claims nothing.
                studio.land_live_follow();
                match concat_host::projects::open(&path).and_then(|info| studio.open_project(info))
                {
                    Ok(()) => {}
                    Err(error) => studio.notify(&error, true),
                }
            }
            self.auto.source = None;
        } else {
            // The sibling stays: the edit is real, it just is not proven.
            // A second run resumes from it rather than rebuilding it.
            self.auto.outcome = 2;
            self.auto.stage = t("forge.autoEditDirectorUnprovenStage");
            self.set_status(tf("forge.autoEditDirectorUnproven", &[&summary]));
            studio.notify(&tf("forge.autoEditDirectorUnproven", &[&summary]), true);
            self.live_follow_stop();
            studio.end_live_follow_back();
            // The sibling stays and opens: the edit is real, it just is not
            // proven. The user can inspect and keep editing it, and a
            // second run resumes from it rather than rebuilding it.
            if let Some(path) = self.auto.project.clone() {
                // The server held the sibling through the run and the
                // acceptance check; without letting go first, the open
                // below is refused - the project is open through the
                // remote connection - and the window is left on the
                // launch screen with an error toast instead of the edit.
                if let Err(error) = concat_mcp::backend::release_window_session(&path) {
                    log::warn!("forge: the run's session did not release: {error}");
                }
                match concat_host::projects::open(&path).and_then(|info| studio.open_project(info))
                {
                    Ok(()) => {}
                    Err(error) => studio.notify(&error, true),
                }
            }
        }
    }

    /// A failed acceptance worker is not a successful edit. Release the
    /// checking lock, preserve the sibling for inspection, and leave a
    /// retryable failure visible instead of freezing the button forever.
    fn auto_director_check_failed(&mut self, error: String, studio: &mut Studio) {
        self.auto.checking = false;
        self.auto.running = false;
        self.auto.check_cancel = None;
        self.auto.outcome = 2;
        self.auto.stage = t("forge.autoEditDirectorUnprovenStage");
        self.set_status(tf("forge.autoEditDirectorUnproven", &[&error]));
        studio.notify(&tf("forge.autoEditDirectorCheckFailed", &[&error]), true);
        self.live_follow_stop();
        studio.end_live_follow_back();
        // The sibling opens for inspection, and the server held it for the
        // run and the half-made check: without a release the open below
        // is refused with "the project is open through a remote
        // connection" and the edit stays locked for the rest of the
        // session. The retry that may follow re-opens the project through
        // the server, so nothing of the resume is lost.
        if let Some(path) = self.auto.project.clone() {
            // The server held the sibling through the check the worker
            // failed; without letting go first, the open below is
            // refused and the user never sees the edit the check could
            // not judge.
            if let Err(error) = concat_mcp::backend::release_window_session(&path) {
                log::warn!("forge: the run's session did not release: {error}");
            }
            match concat_host::projects::open(&path).and_then(|info| studio.open_project(info)) {
                Ok(()) => {}
                Err(open) => studio.notify(&open, true),
            }
        }
    }

    /// Begins following a run's edit live: the window's timeline becomes
    /// the project the run edits, refreshed as the run's own autosaves
    /// land - B-roll arriving, captions appearing, motion cards placed,
    /// the bed ducking - read-only until the run ends and the edit opens
    /// for real. Best-effort by design: a follow that cannot begin says
    /// so and the run goes on, because the edit does not depend on being
    /// watched.
    fn live_follow_start(&mut self, project: &str, studio: &mut Studio) {
        if let Err(error) = studio.begin_live_follow(project) {
            log::warn!("forge: the live follow could not begin: {error}");
            studio.notify(&tf("forge.liveFollowFailed", &[&error]), true);
            return;
        }
        studio.notify(&t("forge.liveFollow"), false);
        let kill = Arc::new(AtomicBool::new(false));
        let watch = Arc::clone(&kill);
        self.live_kill = Some(kill);
        // The watcher only says "look again": the studio re-reads the
        // document only when its fingerprint changed, so a quiet run
        // costs a stat every half second and nothing else.
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if watch.load(Ordering::Relaxed) {
                    return;
                }
                crate::host::on_ui(|studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::LiveDocument));
                });
            }
        });
    }

    /// Stops the live follow's watcher. The follow itself is ended by the
    /// landing (the open replaces the display) or by
    /// [`Studio::end_live_follow_back`]; this only retires the thread
    /// that kept saying "look again".
    fn live_follow_stop(&mut self) {
        if let Some(kill) = self.live_kill.take() {
            kill.store(true, Ordering::Relaxed);
        }
    }

    /// Rereads the runner's last event into the page's own fields: the
    /// stage's word, the fraction of the program's stages, and the clock.
    fn auto_edit_tick(&mut self) {
        if !self.auto.running {
            return;
        }
        if let Some(runner) = &self.auto.runner
            && let Some(progress) = runner.progress()
        {
            self.auto.stage = stage_label(&progress.stage);
            // A stage in progress sits halfway through its own slot at
            // the least: the event stream's own creep may have carried
            // the bar further through it, and the bar only ever
            // advances on evidence - a reread of the same reading moves
            // nothing backwards, and 100% is the run's own accepted
            // end's alone to show, not its longest stage's.
            let total = progress.total_stages.max(1) as f32;
            self.auto.fraction = self
                .auto
                .fraction
                .max((progress.stage_index as f32 + 0.5) / total);
            // A job the stage is waiting on reports its own reading -
            // the export's frames, as a fraction of the whole render -
            // and the bar rides it through the stage's own slot: the
            // one piece of a run that knows how long its middle is. A
            // finished job may claim its stage's slot whole; the run's
            // accepted end is still the only thing that finishes the
            // bar. The ride is through the slot of the stage the
            // events last named, never the fresher one the runner may
            // already be in: the batch that names the next stage is
            // also what resets the reading, and in the window before
            // it lands, a whole reading against the newer index would
            // carry the bar a slot past where any evidence put it -
            // and the bar only ever advances on evidence.
            if let (Some(stage_index), Some(job)) = (self.auto.stage_index, &self.auto.job) {
                let done = job.load(Ordering::Relaxed) as f32 / 1000.0;
                if done > 0.0 {
                    let ridden = stage_index as f32 / total + done / total;
                    self.auto.fraction = self.auto.fraction.max(ridden);
                }
            }
        }
        if let Some(started) = self.auto.started {
            self.auto.elapsed = elapsed_text(started.elapsed());
        }
    }

    /// A batch of the runner's events, drained off the window's thread:
    /// the stage word and the bar follow the events as they come - the
    /// bar creeping through a stage's own slot with each of its calls -
    /// and each stage and call leaves its line in the log, the way the
    /// harness's own words fill the log of a Director run.
    fn auto_edit_events(&mut self, events: Vec<AutoEditEvent>) {
        // A batch that outlived its run - the end landed first, or the
        // run was handed to the Director, whose progress is read from
        // the harness's own words - moves nothing: the terminal state
        // has already said where the run stands.
        if !self.auto.running || self.auto.runner.is_none() {
            return;
        }
        for event in &events {
            // The terminal states are AutoEditEnded's to report: their
            // toast, their receipt and their button are decided there,
            // and a second report from the stream would only race it.
            let AutoEditState::Running { stage, .. } = &event.state else {
                continue;
            };
            let progress = &event.progress;
            if self.auto.stage_index != Some(progress.stage_index) {
                self.auto.stage_index = Some(progress.stage_index);
                self.auto.stage_events = 0;
                self.auto.stage = stage_label(stage);
                // A new stage's slot starts empty: the reading the last
                // stage's wait filled is not this stage's to claim.
                if let Some(job) = &self.auto.job {
                    job.store(0, Ordering::Relaxed);
                }
            } else {
                self.auto.stage_events += 1;
            }
            // The bar only ever advances on evidence: a stage's own
            // beginning, then each of its calls - never a reread of a
            // reading the bar has already passed.
            self.auto.fraction = self.auto.fraction.max(event_fraction(
                progress.stage_index,
                progress.total_stages,
                self.auto.stage_events,
            ));
            let line = auto_edit_event_line(progress);
            // The run's own repeats - the export's wait polls - say
            // themselves once: a line the log already ends with is the
            // same call still waiting, not a new fact.
            if self.log.last().is_none_or(|last| last.as_str() != line) {
                self.log.push(line.into());
            }
        }
        let extra = self.log.len().saturating_sub(LOG_KEPT);
        self.log.drain(0..extra);
    }

    /// The run's terminal state: the toast it earned, the button it
    /// leaves behind, the report it writes beside the edit - and, when it
    /// shipped, the edit opened here on the timeline, which is the point
    /// of the run.
    fn auto_edit_ended(&mut self, state: AutoEditState, studio: &mut Studio) {
        // Keep the source's provenance before terminal cleanup clears the
        // retry state. The report must say whether the Director actually
        // ran, not infer that from a source object already discarded.
        let director_reason = self
            .auto
            .source
            .as_ref()
            .and_then(|source| source.director.clone());
        self.auto.running = false;
        self.auto.started = None;
        self.live_follow_stop();
        // A retryable end keeps the runner and its source for the resume;
        // a completed or hard-failed run has nothing to pick up from.
        // The event stream follows the runner: kept for the resume, let
        // go with it when there is nothing left to resume.
        let keep_for_retry = matches!(
            &state,
            AutoEditState::Failed { error, .. } if error.retryable
        ) || matches!(&state, AutoEditState::Cancelled { .. });
        if !keep_for_retry {
            self.auto.runner = None;
            self.auto.source = None;
            self.auto.tool_cancel = None;
            self.auto.events = None;
            self.auto.job = None;
        } else {
            self.auto.tool_cancel = None;
        }
        match &state {
            AutoEditState::Completed {
                export, duration, ..
            }
            | AutoEditState::CompletedWithWarnings {
                export, duration, ..
            } => {
                self.auto.fraction = 1.0;
                self.auto.elapsed = elapsed_text(*duration);
                self.auto.outcome = 0;
                let warnings = match &state {
                    AutoEditState::CompletedWithWarnings { warnings, .. } => warnings.len(),
                    _ => 0,
                };
                let exported = if export == std::path::Path::new("timeline") {
                    self.auto
                        .project
                        .clone()
                        // The one deliverable with no file to name is the
                        // pane's own word for it, riding the toast's {0}:
                        // a localized line, not an English fragment
                        // baked into the code.
                        .unwrap_or_else(|| t("forge.autoEditEditableTimeline"))
                } else {
                    export.display().to_string()
                };
                if warnings == 0 {
                    self.auto.stage = t("forge.autoEditDoneStage");
                    self.set_status(t("forge.autoEditDoneStage"));
                    studio.notify(&tf("forge.autoEditDone", &[&exported]), false);
                } else {
                    self.auto.stage = t("forge.autoEditWarnedStage");
                    self.set_status(t("forge.autoEditWarnedStage"));
                    studio.notify_warning(&tf("forge.autoEditWarned", &[&warnings.to_string()]));
                }
                // The finished edit opens here, in the window, now - the
                // same landing a forge run's edit has, straight from the
                // live display. The voiceover source project itself stays
                // untouched on disk. The server held the sibling for the
                // run; it lets go so the window's claim can stand.
                if let Some(path) = self.auto.project.clone() {
                    if let Err(error) = concat_mcp::backend::release_window_session(&path) {
                        log::warn!("forge: the run's session did not release: {error}");
                    }
                    // The display goes first: the landing's open must
                    // claim the sibling, not early-return as the display
                    // it already is, or the finished edit stays
                    // read-only under the follow's guard.
                    studio.land_live_follow();
                    match concat_host::projects::open(&path)
                        .and_then(|info| studio.open_project(info))
                    {
                        // A warned export is opened for inspection and repair,
                        // not announced as ready to publish.
                        Ok(()) if warnings == 0 => {
                            studio.notify(&tf("forge.editReady", &[&path]), false)
                        }
                        Ok(()) => {}
                        Err(error) => studio.notify(&tf("forge.openFailed", &[&error]), true),
                    }
                }
            }
            AutoEditState::Failed { error, .. } => {
                self.auto.stage = t("forge.autoEditFailedStage");
                self.auto.outcome = if error.retryable { 1 } else { 2 };
                self.set_status(tf("forge.autoEditFailedStatus", &[&error.message]));
                studio.notify(&tf("forge.autoEditFailed", &[&error.message]), true);
                // A hard failure has no resume coming - the runner and its
                // source were cleared above - so the server's claim on the
                // sibling outlives nothing: it is let go here, and the
                // half-made edit stays one the window can open. A
                // retryable failure keeps the claim, exactly as the
                // cancelled run does, because the resume runs on the same
                // server session the failed run opened.
                if !error.retryable
                    && let Some(path) = self.auto.project.clone()
                    && let Err(error) = concat_mcp::backend::release_window_session(&path)
                {
                    log::warn!("forge: the run's session did not release: {error}");
                }
                studio.end_live_follow_back();
            }
            AutoEditState::Cancelled { rolled_back, .. } => {
                self.auto.stage = t("forge.autoEditCancelledStage");
                // Retry, not done: the fixed spine kept its sibling,
                // its source and its runner (keep_for_retry), so the
                // next press resumes after the stages the cancelled run
                // completed instead of building a fresh sibling - and
                // the button must say so rather than reading as an edit
                // that simply finished.
                self.auto.outcome = 1;
                self.set_status(t("forge.autoEditCancelledStage"));
                // The two endings a cancelled run has: everything the run
                // applied was stepped back, or the rollback stopped
                // partway and says so.
                let note = if *rolled_back {
                    t("forge.autoEditCancelled")
                } else {
                    t("forge.autoEditCancelledPartial")
                };
                studio.notify(&note, false);
                studio.end_live_follow_back();
            }
            // The run is still going, or never left idle: nothing ended.
            AutoEditState::Idle | AutoEditState::Running { .. } => return,
        }
        // The run's record, beside the edit it made: the terminal state
        // exactly as the runner wrote it, for the acceptance read - the
        // same shape the voiceover mapping beside it is written in.
        if let Some(project) = &self.auto.project {
            let mut report = serde_json::Map::new();
            report.insert(
                "schema".to_owned(),
                serde_json::json!("funcut.auto-edit-report/1"),
            );
            match serde_json::to_value(&state) {
                Ok(value) => {
                    report.insert("state".to_owned(), value);
                }
                Err(error) => {
                    log::warn!("the auto-edit report cannot be written: {error}");
                    return;
                }
            }
            // Who made the edit, and why, if it was not the Director: the
            // fallback's reason was only ever a toast the user could miss
            // - here it is the record's own first fact, beside the edit
            // it explains.
            report.insert(
                "director".to_owned(),
                match director_reason {
                    Some(why) => serde_json::json!({ "used": false, "why": why }),
                    None => serde_json::json!({ "used": true }),
                },
            );
            let path = std::path::Path::new(project).join("auto-edit-report.json");
            let bytes = match serde_json::to_vec_pretty(&report) {
                Ok(bytes) => bytes,
                Err(error) => {
                    log::warn!("the auto-edit report cannot be written: {error}");
                    return;
                }
            };
            if let Err(error) = std::fs::write(&path, bytes) {
                log::warn!(
                    "could not write the auto-edit report {}: {error}",
                    path.display()
                );
            }
        }
        // The acceptance receipt, written as its own artifact beside the
        // edit: the verdict, the gates that earned it, the capability
        // areas the run reached, and the rights every imported asset
        // stands on. A run that shipped a file the gates rejected says so
        // here, and the pane's log carries the one-line summary rather
        // than only a green word.
        if let Some(project) = &self.auto.project {
            let receipt = match &state {
                AutoEditState::Completed { receipt, .. }
                | AutoEditState::CompletedWithWarnings { receipt, .. }
                // A failed or cancelled run ships its partial evidence
                // too: the stages that had completed, the gates their
                // answers had measured. The report beside the edit and
                // the toast the pane showed are not evidence; without
                // this the first thing a failed run leaves is nothing.
                | AutoEditState::Failed { receipt, .. }
                | AutoEditState::Cancelled { receipt, .. } => receipt.as_deref(),
                _ => None,
            };
            if let Some(receipt) = receipt {
                let path = std::path::Path::new(project).join("auto-edit-receipt.json");
                match serde_json::to_vec_pretty(&receipt.to_json()) {
                    Ok(bytes) => {
                        if let Err(error) = std::fs::write(&path, bytes) {
                            log::warn!(
                                "could not write the acceptance receipt {}: {error}",
                                path.display()
                            );
                        }
                    }
                    Err(error) => {
                        log::warn!("the acceptance receipt cannot be written: {error}");
                    }
                }
                let summary = receipt.summary();
                studio.handle(Msg::Forge(ForgeMsg::Line(tf(
                    "forge.autoEditAcceptance",
                    &[&summary],
                ))));
                // The pane's own log carries the line too: the nested
                // handle above lands in the blank pane `update` runs
                // against, so without this the fixed spine's one-line
                // summary never appears in the log the user reads.
                self.log
                    .push(tf("forge.autoEditAcceptance", &[&summary]).into());
                let extra = self.log.len().saturating_sub(LOG_KEPT);
                self.log.drain(0..extra);
            }
        }
    }
}

/// Put Forge in its own process group so cancellation can stop the
/// harness and any subprocesses it launched. Descendants inherit this
/// group by default, which also guarantees they cannot keep the reader
/// pipes open after the parent exits.
#[cfg(unix)]
fn configure_forge_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_forge_process_group(_command: &mut Command) {}

/// Terminate the whole Forge process tree on Unix. Killing only the parent
/// can leave a model/helper subprocess holding stdout or stderr open, so
/// the reader-thread joins never finish and the UI never receives Ended.
fn terminate_forge_process(child: &mut Child) {
    #[cfg(unix)]
    {
        let group = -(child.id() as libc::pid_t);
        // SAFETY: a negative pid targets the process group created for this
        // child immediately before spawn; SIGKILL is used only on Cancel or
        // when the child supervisor cannot poll the process.
        let result = unsafe { libc::kill(group, libc::SIGKILL) };
        if result != 0 {
            let _ = child.kill();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill();
    }
}

/// The supervisor the two run readers share: poll the child until it
/// exits, clean the process group, and answer the exit code. The exit is
/// observed WITHOUT reaping - on Unix, `waitpid` with `WNOWAIT` leaves
/// the exited leader a zombie, its pid unreusable - so the group cleanup
/// that follows (a descendant can still own the pipes) always runs while
/// the leader still exists, and `kill(-pid)` can never land on a
/// recycled process group the way a post-reap kill could.
fn supervise_forge_child(
    mut child: Child,
    kill: Arc<AtomicBool>,
) -> Option<std::process::ExitStatus> {
    loop {
        #[cfg(unix)]
        {
            let pid = child.id() as libc::pid_t;
            // SAFETY: waitpid with WNOHANG|WNOWAIT only peeks: it reports
            // the exit without reaping, so the pid stays the leader's.
            let mut code: libc::c_int = 0;
            let seen = unsafe { libc::waitpid(pid, &mut code, libc::WNOHANG | libc::WNOWAIT) };
            if seen == pid {
                // The group first, while the leader still owns its pid
                // and the group is certainly this run's: a descendant
                // holding a pipe dies with the rest, and the readers'
                // joins end. THEN the reap, for the real status.
                // SAFETY: negative pid targets the run's own group.
                unsafe { libc::kill(-pid, libc::SIGKILL) };
                break child.wait().ok();
            }
            if seen == -1 {
                // The poll itself failed: fall back to the leader-only
                // stop and reap what is there.
                log::warn!("forge: the child could not be polled");
                terminate_forge_process(&mut child);
                break child.wait().ok();
            }
        }
        #[cfg(not(unix))]
        {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let _ = child.kill();
                    break Some(status);
                }
                Err(_) => {
                    let _ = child.kill();
                    break child.wait().ok();
                }
                Ok(None) => {}
            }
        }
        if kill.load(Ordering::Relaxed) {
            terminate_forge_process(&mut child);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// Reads the run on its own thread: each line of the harness's output is
/// posted as it comes, and the run's exit code is posted after the last.
/// A separate supervisor polls the child, so Cancel is honoured even
/// while the model call is silent; the posts that follow report the exit.
fn read_the_run(mut child: Child, kill: Arc<AtomicBool>) {
    // Drain both pipes independently: a full stderr pipe must never block
    // stdout, and cancellation must never depend on the child printing.
    let out = child.stdout.take();
    let err = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        if let Some(pipe) = out {
            // A line that is not UTF-8 is skipped, not the end of the
            // log: stopping at the first bad byte would silently
            // truncate everything the harness said after it.
            for line in BufReader::new(pipe).lines() {
                let Ok(line) = line else { continue };
                crate::host::on_ui(move |studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::Line(line)));
                });
            }
        }
    });
    let stderr_reader = std::thread::spawn(move || {
        if let Some(pipe) = err {
            // A line that is not UTF-8 is skipped, not the end of the
            // log: stopping at the first bad byte would silently
            // truncate everything the harness said after it.
            for line in BufReader::new(pipe).lines() {
                let Ok(line) = line else { continue };
                crate::host::on_ui(move |studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::Line(line)));
                });
            }
        }
    });

    // This supervisor polls the process while the reader threads drain output.
    // A quiet model call can take minutes without writing a line; Cancel must
    // still terminate it promptly instead of waiting for another stdout event.
    std::thread::spawn(move || {
        let status = supervise_forge_child(child, kill);
        let _ = stdout_reader.join();
        let _ = stderr_reader.join();
        let code = status.and_then(|status| status.code()).unwrap_or(-1);
        crate::host::on_ui(move |studio, _, _| {
            studio.handle(Msg::Forge(ForgeMsg::Ended(code)));
        });
    });
}

/// One voiceover clip's trip from the source timeline to the sibling
/// project: the identity the narration played with. The sibling is
/// rebuilt from this mapping, never from a sort of the distinct files -
/// a sort would drop the gaps, the trims, the speeds and the levels the
/// take was performed with.
#[derive(Clone, Debug, serde::Serialize)]
struct VoiceoverSegment {
    /// The clip the narration played on the source timeline.
    source_clip_id: String,
    /// The clip that plays it on the sibling's voiceover lane.
    sibling_clip_id: String,
    /// The sibling file the clip plays - the copy the run repairs and
    /// transcribes - so a reader of the mapping can name the take a
    /// segment's words came from.
    take: String,
    /// Seconds into the timeline - the source's own start, so the gaps
    /// and overlaps between segments survive the handoff exactly.
    start: f64,
    /// Where in the source file the clip begins, in source seconds.
    source_in: f64,
    /// Where it ends, in source seconds: in plus the span it plays.
    source_out: f64,
    /// Playback rate, exactly as the source clip played it.
    speed: f64,
    /// Linear gain, 1 being unity.
    volume: f64,
    /// The (fade-in, fade-out) ramps, in seconds.
    fades: (f64, f64),
    /// The source lane the clip came from.
    source_track: String,
    /// The sibling lane the clip sits on.
    track: String,
}

/// What a voiceover run builds in: the sibling project the director
/// edits, the voiceover files copied into it, and the per-clip mapping
/// that reconstructed its timeline.
struct VoiceoverSource {
    /// The new project's folder, ready for `project_open`.
    project: String,
    /// The voiceover files, absolute paths, in the order they play.
    files: Vec<String>,
    /// One record per voiceover clip: source clip to sibling clip.
    segments: Vec<VoiceoverSegment>,
}

/// Names a copied voiceover without allowing equal basenames from
/// different source folders to overwrite one another.
fn voiceover_copy_name(index: usize, source: &std::path::Path) -> String {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("voiceover.wav");
    format!("voiceover-{index:03}-{name}")
}

/// A carried picture's copy, in the sibling's footage folder: the same
/// no-overwrite spelling the voiceover copies use.
fn footage_copy_name(index: usize, source: &std::path::Path) -> String {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("footage.mp4");
    format!("footage-{index:03}-{name}")
}

/// The same, in the AUTO EDIT's own spelling: the file the timeline's
/// clips play is the TAKE, and the repair stage's clean pass lands on
/// it, overwriting this pristine copy with the repaired chain, exactly
/// as the watcher's "the timeline carries the repaired take" promises.
fn take_copy_name(index: usize, source: &std::path::Path) -> String {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("voiceover.wav");
    format!("take-{index:03}-{name}")
}

/// The enhanced intermediate's name, from the take's own spelling, so
/// the repair chain's two files sit beside each other in the folder.
fn enhanced_copy_name(index: usize, source: &std::path::Path) -> String {
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("voiceover.wav");
    format!("enhanced-{index:03}-{name}")
}

/// One voiceover file's fixed paths inside the sibling, as the AUTO
/// EDIT's program is written from them: the take the timeline plays, and
/// the enhanced stepping stone the repair writes before the clean.
/// One voiceover file's fixed paths inside the sibling, as the AUTO
/// EDIT's program is written from them: the take the timeline plays, and
/// the enhanced stepping stone the repair writes before the clean.
#[derive(Clone)]
struct TakePaths {
    /// The file the sibling's voiceover clips play: the pristine copy
    /// until the repair's clean pass overwrites it with the enhanced,
    /// cleaned chain.
    take: String,
    /// The enhanced intermediate: the repair's first pass writes it, its
    /// second reads it. A repair that fails leaves the take as recorded.
    enhanced: String,
    /// The sibling path of the carried video whose audio the take was
    /// extracted from, when the narration came out of a talking-head
    /// video: the tighten cuts both in lockstep. `None` for a plain
    /// audio take.
    aroll: Option<String>,
}

/// One placement of a take on the sibling timeline: what the verify's
/// expected side is made of - the file the clip plays, where on the
/// timeline it starts, the span of the file it plays and the rate it
/// plays at, so the take's transcript words can be placed where the
/// export must carry them.
#[derive(Clone)]
struct TakePlacement {
    /// The sibling file the clip plays.
    take: String,
    /// Where the clip starts on the timeline.
    start: f64,
    /// Where the clip begins in the file, in file seconds.
    source_in: f64,
    /// Where it ends in the file, in file seconds.
    source_out: f64,
    /// Playback rate, 1 being unity.
    speed: f64,
}

/// What an AUTO EDIT run needs beside the forge run's handoff: the
/// fixed paths its program is written from, all inside the sibling it
/// edits.
#[derive(Clone)]
struct AutoEditSource {
    /// The sibling project the run edits.
    project: String,
    /// One row per distinct voiceover file, in first-play order.
    takes: Vec<TakePaths>,
    /// One row per voiceover clip, in first-play order: the placement
    /// the verify's expected transcript is rebuilt from (F-04).
    placements: Vec<TakePlacement>,
    /// Where the export lands, inside the sibling's own export folder.
    export: String,
    /// The contact sheet's name, relative to the project: the window's
    /// server refuses absolute write paths, and the sheet is written
    /// through it.
    sheet: String,
    /// Why the Director did NOT take this edit, when the fixed spine ran
    /// instead: `None` means the Director did. The toast says it once;
    /// the report beside the edit says it forever, so a run that shipped
    /// a lesser edit never gets mistaken for the Director's.
    director: Option<String>,
    /// The script the button carried, when the Forge page's script box
    /// had text: the authoritative wording the voiceover was read from.
    /// The run hands it to the voice reading, which aligns the transcript
    /// against it - misheard words corrected to the author's spelling,
    /// measured timings kept - so the captions and the queries spend the
    /// author's words. `None` when the box was empty: the transcript
    /// then stands as it was transcribed.
    script: Option<String>,
    /// The licence word the pressed switch carried: a monetized edit is
    /// refused the non-commercial beds - the music model's, the free
    /// shelves - and one that is not may spend them. It rides the source
    /// so the Director's prompt, the spine's program and the retry all
    /// answer the one choice, and the plan stage that spends it is the
    /// one a resume skips - the brief the prior run wrote is the one
    /// this run ships.
    monetized: bool,
    /// The frame the sibling was born in, when the build knew it: the
    /// handover prompt names it in numbers, not only in vocabulary - an
    /// agent handed a vertical reel should never have to read the
    /// project to learn which shape it edits.
    frame: Option<(u32, u32)>,
}

/// What the sibling build collected, as one unit: a copy that fails or
/// a document that cannot be written removes the half-populated sibling
/// rather than leaving any of it behind.
struct BuiltSibling {
    /// The voiceover files, absolute paths, in the order they play.
    files: Vec<String>,
    /// One record per voiceover clip: source clip to sibling clip.
    segments: Vec<VoiceoverSegment>,
    /// The AUTO EDIT's take rows, one per distinct file - empty for a
    /// forge run's sibling, which names its copies its own way.
    takes: Vec<TakePaths>,
}

/// Makes the sibling project a voiceover run edits: the open project's
/// audio clips are found on the timeline, copied into a fresh `<name>
/// edit` folder beside it (the window holds the source, and an agent
/// cannot open what the window holds), and the new folder is what the
/// director opens. Every clip is reconstructed on a dedicated voiceover
/// lane with its own start, in/out points, speed, volume and fades, so
/// the narration arrives in the sibling exactly as it played in the
/// source - gaps and overlaps included - and `voiceover-mapping.json`
/// in the sibling's root records the clip-to-clip mapping. The source
/// project is never touched.
fn voiceover_project(
    studio: &Studio,
    for_auto_edit: bool,
) -> Result<(VoiceoverSource, Option<AutoEditSource>), String> {
    let Some(session) = studio.session.as_ref() else {
        return Err(t("forge.needProject"));
    };
    // An agent run's sibling is always born in the source project's own
    // frame: the orientation pick is AUTO EDIT's alone.
    build_voiceover_sibling(
        studio.project(),
        std::path::Path::new(session.path()),
        for_auto_edit,
        None,
    )
}

/// Whether a timeline clip is narration: audio, with media behind it,
/// that is not the project's music bed. The bed is `bgm_import`'s own
/// folder - `<project>/bgm/` - and a clip of it is BGM however the
/// timeline is drawn; counting it as narration armed AUTO EDIT on a
/// music-only project and transcribed the music as speech (F-01).
fn is_narration(
    clip: &concat_project::model::Clip,
    project: &concat_project::model::Project,
    source_root: &std::path::Path,
) -> bool {
    clip.kind == concat_project::model::ClipKind::Audio
        && project
            .media_by_id(&clip.media_id)
            .is_some_and(|item| !item.path.is_empty() && !is_bed(item, source_root))
}

/// A talking-head candidate: a video clip whose own file carries sound.
/// A project with no narration audio and one of these still has a
/// voiceover - it is just welded to a face - so the AUTO EDIT extracts
/// the sound, places the video as A-roll, and edits that. The button
/// enables for it instead of demanding the user detach the audio by
/// hand.
fn is_talking_head(
    clip: &concat_project::model::Clip,
    project: &concat_project::model::Project,
    source_root: &std::path::Path,
) -> bool {
    clip.kind == concat_project::model::ClipKind::Video
        && project.media_by_id(&clip.media_id).is_some_and(|item| {
            !item.path.is_empty() && item.has_audio && !is_bed(item, source_root)
        })
}

/// The open timeline's first narration file: the take the signal lab
/// reads. The same test the sibling build makes - an audio clip that is
/// not the bed - answered with the file's own path, in first-play order.
fn voiceover_file(studio: &Studio) -> Option<String> {
    let session = studio.session.as_ref()?;
    let root = std::path::Path::new(session.path());
    let project = studio.project();
    let timeline = project.active();
    let mut clips: Vec<&concat_project::model::Clip> = timeline
        .clips
        .iter()
        .map(|clip| clip.as_ref())
        .filter(|clip| is_narration(clip, project, root))
        .collect();
    clips.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let first = clips.first()?;
    project
        .media_by_id(&first.media_id)
        .map(|item| item.path.clone())
        .filter(|path| !path.is_empty())
}

/// One signal button's whole read: the window's own server, the same
/// toolbox an agent's call means, and the one named signal - transcribed
/// first when the take never was, exactly the way the AUTO EDIT's own
/// program would have it. Ok is the reading's title and body; Err is
/// the sentence that says why there is none.
fn read_one_signal(audio: &str, name: &str) -> Result<(String, String), String> {
    let mut backend = concat_mcp::backend::open(concat_mcp::backend::Mode::Window)?;
    let toolbox = match std::panic::catch_unwind(concat_mcp::tools::Toolbox::new) {
        Ok(toolbox) => std::sync::Arc::new(toolbox),
        // The refusal lands as the reading card's whole body, so it is the
        // lab's own wording - a localized line, the same sentence the
        // AUTO EDIT's toolbox guard shows.
        Err(_) => return Err(t("forge.signalsToolboxFailed")),
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    // The one reading, taken once: the reply the profile's own extract
    // builds, `requested` block and all. A refusal is the tool's own
    // sentence, never a guess at what it might have meant.
    let extract = |backend: &mut dyn concat_mcp::Backend,
                   arguments: serde_json::Value|
     -> Result<serde_json::Value, String> {
        let outcome =
            concat_mcp::sense::voice_signals(arguments, backend).map_err(|error| error.message)?;
        if outcome.is_error {
            // The tool's own sentence rides as it came; the fallback for a
            // refusal that said nothing is the lab's wording, so it is a
            // localized line rather than an English fragment in the card.
            let sentence = outcome
                .content
                .iter()
                .find_map(|block| block["text"].as_str().map(str::to_owned))
                .unwrap_or_else(|| t("forge.signalsRefused"));
            return Err(sentence);
        }
        let reply = outcome.structured.unwrap_or_else(|| serde_json::json!({}));
        if reply.get("ok") == Some(&serde_json::json!(false)) {
            return Err(reply["error"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| t("forge.signalsRefused")));
        }
        Ok(reply)
    };
    let arguments = serde_json::json!({ "audio": audio, "signals": [name] });
    let reply = match extract(backend.as_mut(), arguments.clone()) {
        Ok(reply) => reply,
        Err(why) if why.contains("no lines") => {
            // The take was never transcribed: transcribe it and wait, the
            // same two calls the AUTO EDIT's words stage makes, then take
            // the reading again - the registry now holds the words. The
            // caller is scoped to the two calls, so its hold on the
            // backend ends before the reading takes it back.
            {
                let mut caller = concat_mcp::director::BackendToolCaller::new(
                    toolbox.as_ref(),
                    backend.as_mut(),
                    &cancelled,
                );
                caller
                    .call(&concat_mcp::director::ToolCall {
                        tool: "transcribe".to_owned(),
                        arguments: serde_json::json!({ "path": audio }),
                    })
                    .map_err(|error| error.message)?;
                caller
                    .call(&concat_mcp::director::ToolCall {
                        tool: "analysis_wait".to_owned(),
                        arguments: serde_json::json!({
                            "path": audio,
                            "timeoutSeconds": 590,
                        }),
                    })
                    .map_err(|error| error.message)?;
            }
            extract(backend.as_mut(), arguments)?
        }
        Err(why) => return Err(why),
    };
    format_signal_reading(&reply, name)
        // The reply named no reading for the button that asked: the lab's
        // own wording for that, as a localized line the card can show.
        .ok_or_else(|| t("forge.signalsNoRequestedBlock"))
}

/// The catalogue as the lab holds it: the families in the catalogue's
/// own order, each with its signals' names (what extraction asks for)
/// and labels (what the button says).
fn signal_families_of(reply: &serde_json::Value) -> Vec<SignalFamily> {
    reply["families"]
        .as_array()
        .map(|labels| {
            labels
                .iter()
                .filter_map(|label| {
                    let label = label.as_str()?;
                    let signals = reply["signals"]
                        .as_array()?
                        .iter()
                        .filter(|signal| signal["family"].as_str() == Some(label))
                        .filter_map(|signal| {
                            Some((
                                signal["name"].as_str()?.to_owned(),
                                signal["label"].as_str()?.to_owned(),
                            ))
                        })
                        .collect();
                    Some(SignalFamily {
                        label: label.to_owned(),
                        signals,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The reading's card: the signal's own label over a body that says
/// what it measures, why the edit wants it, the ear that answered, the
/// confidence it carries, and the values themselves. `None` when the
/// reply's requested block never named the signal - the caller says so
/// rather than showing an empty card.
fn format_signal_reading(reply: &serde_json::Value, name: &str) -> Option<(String, String)> {
    let row = reply["requested"]
        .as_array()?
        .iter()
        .find(|row| row["name"].as_str() == Some(name))?;
    let label = row["label"].as_str().unwrap_or(name).to_owned();
    let mut body = String::new();
    if let Some(reads) = row["reads"].as_str() {
        body.push_str(&format!("{}: {reads}\n", t("forge.signalsMeasures")));
    }
    if let Some(why) = row["why"].as_str() {
        body.push_str(&format!("{}: {why}", t("forge.signalsWhy")));
    }
    if let Some(source) = row["source"].as_str() {
        body.push_str(&format!("\n{}: {source}", t("forge.signalsEar")));
    }
    if let Some(confidence) = row["confidence"].as_f64() {
        body.push_str(&format!(
            "\n{}: {confidence:.2}",
            t("forge.signalsConfidence")
        ));
    }
    let values = format_signal_values(&row["value"]);
    if !values.is_empty() {
        body.push_str("\n\n");
        body.push_str(&values);
    }
    Some((label, body))
}

/// The reading's values, as a card can hold them: a per-line reading is
/// one row a line (where it sits, what it read, the words), a take-wide
/// reading is its own JSON, compact, truncated where a curve would
/// flood the card.
fn format_signal_values(value: &serde_json::Value) -> String {
    let compact = |value: &serde_json::Value| -> String {
        let whole = serde_json::to_string(value).unwrap_or_default();
        let cut: String = whole.chars().take(600).collect();
        if whole.chars().count() > 600 {
            format!("{cut}…")
        } else {
            cut
        }
    };
    match value {
        serde_json::Value::Array(rows)
            if !rows.is_empty()
                && rows
                    .iter()
                    .all(|row| row.get("line").is_some() && row.get("value").is_some()) =>
        {
            let mut lines: Vec<String> = rows
                .iter()
                .take(12)
                .map(|row| {
                    let at = match (row["start"].as_f64(), row["end"].as_f64()) {
                        (Some(start), Some(end)) => format!("{start:.1}-{end:.1}s"),
                        _ => "untimed".to_owned(),
                    };
                    let text = row["text"].as_str().unwrap_or_default();
                    format!("{at}  {}  {text}", compact(&row["value"]))
                })
                .collect();
            if rows.len() > 12 {
                lines.push(format!("… {}", rows.len()));
            }
            lines.join("\n")
        }
        other => {
            let reading = compact(other);
            if reading == "null" || reading.is_empty() {
                String::new()
            } else {
                reading
            }
        }
    }
}

/// Whether a media item is the edit's music bed: a file `bgm_import`
/// copied into `<project>/bgm/`, the folder the bed's own tooling owns.
fn is_bed(item: &concat_project::model::MediaItem, source_root: &std::path::Path) -> bool {
    std::path::Path::new(&item.path).starts_with(source_root.join("bgm"))
}

/// The refusal when the *other* run is already going, or `None` when this
/// entry point may start. The Forge page offers two ways to edit the same
/// project - the agent run (Start) and the unattended AUTO EDIT - and both
/// write one sibling through the same server; letting them overlap
/// interleaves two edits into a timeline neither planned, so the second is
/// refused and told which run to stop. `starting_start` is true for the
/// Start button and false for the AUTO EDIT button: a Start pressed while
/// the auto edit runs is told to stop the AUTO EDIT, and the other way
/// round. The labels are written as `t(...)` calls here, not returned as
/// keys, so the locale inventory sees them used.
fn other_run_refusal(other_running: bool, starting_start: bool) -> Option<String> {
    if !other_running {
        return None;
    }
    Some(if starting_start {
        t("forge.autoEditBusy")
    } else {
        t("forge.startBusy")
    })
}

/// The frame an orientation pick maps to: horizontal 1920x1080, vertical
/// 1080x1920, square 1080x1080, each with the source project's own size
/// tier - a 4K project picked vertical is born 2160x3840, not 1080x1920,
/// because the pick names the shape and the tier is the project's own to
/// keep. The rate and the colour are never the pick's to name - the
/// sibling keeps the source project's own. `None` for no pick, or a word
/// the table does not know: the sibling is born in the source's own
/// frame, exactly as the handoff always built it.
fn orientation_frame(orientation: Option<&str>, tier: (u32, u32)) -> Option<(u32, u32)> {
    // The tier is the pick's own short edge: the source's smaller side,
    // clamped to the ladder a shelf or a platform actually serves - so a
    // 2160-wide project picked vertical is born 2160x3840, a 720 one
    // 720x1280, and a pick over the source's own tier never downscales
    // what the project already paid for.
    let short_edge = tier.0.min(tier.1).max(360);
    match orientation.map(str::trim) {
        Some("horizontal") => Some((short_edge * 16 / 9, short_edge)),
        Some("vertical") => Some((short_edge, short_edge * 16 / 9)),
        Some("square") => Some((short_edge, short_edge)),
        _ => None,
    }
}

/// The handoff itself, apart from the studio so a test can drive it with
/// a plain project model: [`voiceover_project`] supplies the window's
/// two arguments and nothing else. With `for_auto_edit`, the copied
/// files take the AUTO EDIT's own names and the sibling's fixed paths -
/// the takes, the export, the sheet - come back beside the handoff, for
/// the program that run is written from.
fn build_voiceover_sibling(
    source: &concat_project::model::Project,
    source_root: &std::path::Path,
    for_auto_edit: bool,
    orientation: Option<&str>,
) -> Result<(VoiceoverSource, Option<AutoEditSource>), String> {
    use concat_project::model::ClipKind;

    let timeline = source.active();
    // The voiceover is every NARRATION clip on the timeline, each carrying
    // the identity it plays with - not the distinct files once each. A take
    // split into pieces is several clips of one file; an edit that trims,
    // retimes or levels the narration lives in those clip fields, not in
    // the file. The bed is not the voiceover: `bgm_import` lands its music
    // in `<project>/bgm/`, and a clip of that folder is BGM, not speech to
    // transcribe (F-01).
    let mut audio: Vec<&concat_project::model::Clip> = timeline
        .clips
        .iter()
        .map(|clip| clip.as_ref())
        .filter(|clip| clip.kind == ClipKind::Audio)
        .collect();
    audio.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut voiced: Vec<(
        &concat_project::model::Clip,
        &concat_project::model::MediaItem,
    )> = Vec::new();
    for clip in audio {
        // The bin lives on the project, not the timeline: a clip names its
        // media by id, and the project holds what that id is. A clip whose
        // media is gone or pathless cannot be copied - it is skipped, not
        // guessed at.
        let Some(item) = source.media_by_id(&clip.media_id) else {
            continue;
        };
        if !item.path.is_empty() && !is_bed(item, source_root) {
            voiced.push((clip, item));
        }
    }
    // The picture the source already owns: every visual clip - a video
    // cut, a placed still - whose media is real, in first-play order.
    // These are carried into the sibling as A-roll: the one-click edit
    // edits the user's own footage instead of dropping it on the floor
    // and shopping stock over the hole. The bed is never picture.
    let mut visual: Vec<(
        &concat_project::model::Clip,
        &concat_project::model::MediaItem,
    )> = Vec::new();
    let mut visual_sorted: Vec<&concat_project::model::Clip> = timeline
        .clips
        .iter()
        .map(|clip| clip.as_ref())
        .filter(|clip| matches!(clip.kind, ClipKind::Video | ClipKind::Image))
        .collect();
    visual_sorted.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for clip in visual_sorted {
        let Some(item) = source.media_by_id(&clip.media_id) else {
            continue;
        };
        if !item.path.is_empty() && !is_bed(item, source_root) {
            visual.push((clip, item));
        }
    }
    // A project with no narration audio and one face that talks: the
    // talking-head door. The video's own sound is the voiceover, so the
    // sibling extracts it as the take, places the video as A-roll with
    // its audio silenced (the take is the sound), and the tighten cuts
    // picture and words in lockstep. Without this the button was greyed
    // until the user detached the audio by hand - and the audit's very
    // first finding was that the footage was then still dropped.
    let talking_head: Option<(
        &concat_project::model::Clip,
        &concat_project::model::MediaItem,
    )> = if voiced.is_empty() {
        visual
            .iter()
            .find(|(clip, item)| clip.kind == ClipKind::Video && item.has_audio)
            .map(|(clip, item)| (*clip, *item))
    } else {
        None
    };
    if voiced.is_empty() && talking_head.is_none() {
        return Err(t("forge.noVoiceover"));
    }
    // The distinct files, in first-play order - what gets copied in.
    // A talking-head's narration is the extraction, not a copy: its
    // "file" is created below, beside the copies.
    let mut files: Vec<&concat_project::model::MediaItem> = Vec::new();
    for (_, item) in &voiced {
        if !files.iter().any(|known| known.path == item.path) {
            files.push(item);
        }
    }
    // The sibling folder: `<name> edit`, then `edit 2`, `edit 3`, ... so
    // every run gets a fresh project and an earlier edit is never
    // overwritten. The canvas matches the source project's, unless the
    // orientation pick named a frame for the AUTO EDIT to be built for:
    // a sibling born in the picked frame needs no frame-set call, so one
    // early decision at the birth adapts everything downstream - the
    // stock the plan shops, the caption band, the motion canvas, the
    // plan's own aspect - with no downstream change at all.
    // The tier the pick inherits: the source project's own short edge,
    // so a 4K project picked vertical is born a 4K reel and a 720 one
    // is born 720 - the pick names the shape, never the price.
    let tier = (timeline.video.width, timeline.video.height);
    let video = match orientation_frame(orientation, tier) {
        Some((width, height)) => concat_project::model::VideoSettings {
            width,
            height,
            ..timeline.video
        },
        None => timeline.video,
    };
    let folder = source_root
        .parent()
        .map(std::path::Path::to_path_buf)
        .ok_or_else(|| t("forge.needProject"))?;
    let stem = source_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("funcut")
        .to_owned();
    let mut made = None;
    for attempt in 0..50 {
        let name = if attempt == 0 {
            format!("{stem} edit")
        } else {
            format!("{stem} edit {}", attempt + 1)
        };
        match concat_host::projects::create(
            &folder.to_string_lossy(),
            &name,
            video.width,
            video.height,
            video.rate_num,
            video.rate_den,
        ) {
            Ok(info) => {
                made = Some(info);
                break;
            }
            // A folder that already holds a project is the next attempt's
            // business, not a failure.
            Err(error) if attempt < 49 && error.contains("already") => continue,
            Err(error) => return Err(error),
        }
    }
    let Some(info) = made else {
        return Err(t("forge.projectFailed"));
    };
    // Copy the files in and rebuild the narration's clips, as one unit:
    // a copy that fails or a document that cannot be written removes the
    // half-populated sibling rather than leaving it behind.
    let built = (|| -> Result<BuiltSibling, String> {
        // The voiceover files are copied in, so the edit stands on its own
        // and the source project can be moved or deleted without breaking
        // it. The AUTO EDIT's copies take the take- spelling: the repair
        // stage's clean pass lands on them, overwriting the pristine copy
        // with the repaired chain the timeline then plays.
        let into = std::path::Path::new(&info.path).join("voiceover");
        std::fs::create_dir_all(&into).map_err(|error| error.to_string())?;
        let roll_dir = std::path::Path::new(&info.path).join("footage");
        let mut mint = concat_project::commands::IdMint::default();
        let mut copied: Vec<String> = Vec::with_capacity(files.len());
        let mut takes: Vec<TakePaths> = Vec::with_capacity(files.len());
        let mut media_id_of: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        // The carried picture's own bookkeeping: which media copied to
        // which sibling id, and the talking-head's video clip + measured
        // span once its extraction lands.
        let mut roll_media_of: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut roll_copied: Vec<String> = Vec::new();
        let mut talking_segment: Option<(&concat_project::model::Clip, f64)> = None;
        // The sibling file each source take copied into - the take a
        // segment's clip plays, named where the mapping's reader wants it.
        let mut take_of: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut sibling_media: Vec<concat_project::model::MediaItem> = Vec::new();
        for item in &files {
            let source_file = std::path::Path::new(&item.path);
            let name = if for_auto_edit {
                take_copy_name(copied.len(), source_file)
            } else {
                voiceover_copy_name(copied.len(), source_file)
            };
            let target = into.join(&name);
            // The refusal is the pane's own wording and lands as the
            // toast itself - the same kind of sentence the sibling's
            // other refusals are - so it is a localized line, with the
            // file and the file system's own reason riding its places.
            std::fs::copy(&item.path, &target)
                .map_err(|error| tf("forge.voiceoverCopyFailed", &[&item.path, &error]))?;
            let path = target.to_string_lossy().into_owned();
            take_of.insert(item.path.clone(), path.clone());
            if for_auto_edit {
                takes.push(TakePaths {
                    take: path.clone(),
                    enhanced: into
                        .join(enhanced_copy_name(copied.len(), source_file))
                        .to_string_lossy()
                        .into_owned(),
                    aroll: None,
                });
            }
            // The bin item carries the source's own probe metadata - the
            // duration and kind its clips were cut against - with the
            // copied file's path and a fresh id of its own.
            let mut entry = (*item).clone();
            entry.id = mint.next("m");
            entry.path = path.clone();
            media_id_of.insert(item.path.clone(), entry.id.clone());
            copied.push(path);
            sibling_media.push(entry);
        }
        // The talking-head's extraction: the video's own sound, written
        // as the take the whole pipeline reads. The mix engine decodes
        // the file's first audio stream into a WAV of its whole length,
        // so the take's file clock is the video's - the lockstep the
        // tighten cuts both by. The extraction runs here, on the UI
        // thread beside the copies: it is one decode of one file, the
        // same order of work the copy loop already does.
        let mut visual_files: Vec<&concat_project::model::MediaItem> = Vec::new();
        for (_, item) in &visual {
            if !visual_files.iter().any(|known| known.path == item.path) {
                visual_files.push(item);
            }
        }
        if let Some((clip, item)) = talking_head {
            let source_file = std::path::Path::new(&item.path);
            // The footage copy's index is the media's own position in
            // the carried list, so the link below names the exact file
            // the footage loop will copy.
            let roll_index = visual_files
                .iter()
                .position(|known| known.path == item.path)
                .unwrap_or(0);
            let take_name = take_copy_name(0, source_file);
            let take_target = into.join(&take_name);
            let span = item
                .duration
                .filter(|duration| *duration > 0.5)
                .unwrap_or(clip.source_start + clip.duration * clip.speed);
            let mix = concat_media::audio::mix_to_file(
                &[concat_media::audio::AudioClip {
                    path: source_file.to_path_buf(),
                    stream: None,
                    start: 0.0,
                    duration: span,
                    source_start: 0.0,
                    speed: 1.0,
                    preserve_pitch: true,
                    volume: 1.0,
                    volume_curve: Default::default(),
                    fade_in: 0.0,
                    fade_out: 0.0,
                    filter_chain: String::new(),
                }],
                span,
                &take_target,
                None,
            );
            // The same pane-authored refusal, localized the same way:
            // the video the sound was pulled from and the engine's own
            // reason ride the two places.
            mix.map_err(|error| tf("forge.narrationExtractFailed", &[&item.path, &error]))?;
            let take_path = take_target.to_string_lossy().into_owned();
            // The footage copy is made below with the rest of the
            // picture; the take's a-roll link is its sibling path, named
            // the same way the footage loop names it.
            let roll_name = footage_copy_name(roll_index, source_file);
            let aroll_path = roll_dir.join(&roll_name).to_string_lossy().into_owned();
            takes.push(TakePaths {
                enhanced: into
                    .join(enhanced_copy_name(0, source_file))
                    .to_string_lossy()
                    .into_owned(),
                take: take_path.clone(),
                aroll: Some(aroll_path),
            });
            let entry = concat_project::model::MediaItem {
                id: mint.next("m"),
                path: take_path,
                name: take_name,
                duration: Some(span),
                kind: concat_project::model::MediaKind::Audio,
                width: None,
                height: None,
                frame_rate: None,
                frame_rate_fraction: None,
                video_codec: None,
                audio_codec: item.audio_codec.clone(),
                has_audio: true,
                audio_tracks: Vec::new(),
                origin: Some(concat_project::model::MediaOrigin::Processed),
                placeholder: false,
                color_range: None,
                color_space: item.color_space,
                stock: None,
                extra: Default::default(),
            };
            sibling_media.push(entry);
            talking_segment = Some((clip, span));
        }
        // The picture, carried: the source's own visual media, copied
        // into the sibling's footage folder so the edit stands on its own
        // and the source can move without breaking it. The talking-head's
        // video is one of these - its lane clip is muted below, because
        // the take is its sound now.
        std::fs::create_dir_all(&roll_dir).map_err(|error| error.to_string())?;
        for item in &visual_files {
            let source_file = std::path::Path::new(&item.path);
            let name = footage_copy_name(roll_copied.len(), source_file);
            let target = roll_dir.join(&name);
            // The carried picture's copy refuses in the pane's own
            // words too, so it reads as the localized toast the
            // voiceover copy's refusal does.
            std::fs::copy(&item.path, &target)
                .map_err(|error| tf("forge.footageCopyFailed", &[&item.path, &error]))?;
            let path = target.to_string_lossy().into_owned();
            let mut entry = (*item).clone();
            entry.id = mint.next("m");
            entry.path = path.clone();
            roll_media_of.insert(item.path.clone(), entry.id.clone());
            roll_copied.push(path);
            sibling_media.push(entry);
        }
        // One sibling lane per source lane, at the bottom of the stack in
        // first-play order, so takes recorded across lanes keep their
        // arrangement - and their overlap, which a single lane cannot
        // hold. The picture's lanes sit above the voiceover's and below
        // the stock the gather later places.
        let mut lanes: Vec<(String, String)> = Vec::new();
        for (clip, _) in &voiced {
            if !lanes.iter().any(|(from, _)| from == &clip.track_id) {
                lanes.push((clip.track_id.clone(), mint.next("t")));
            }
        }
        // A talking-head's take plays on the lane its video played on,
        // so the words sit exactly where the face was.
        if let Some((clip, _)) = talking_segment
            && !lanes.iter().any(|(from, _)| from == &clip.track_id)
        {
            lanes.push((clip.track_id.clone(), mint.next("t")));
        }
        let mut roll_lanes: Vec<(String, String)> = Vec::new();
        for (clip, _) in &visual {
            if !roll_lanes.iter().any(|(from, _)| from == &clip.track_id) {
                roll_lanes.push((clip.track_id.clone(), mint.next("t")));
            }
        }
        // The sibling document: the copied bin, the voiceover lanes, and
        // every clip restored to the position, in-point, speed, volume,
        // fades and stream it played with. The audio filter chain rides
        // along too - the narration's sound is the edit's, not the file's.
        let mut sibling = concat_project::model::Project::with_video(video);
        sibling.media = sibling_media;
        sibling.active_mut().tracks.splice(
            0..0,
            lanes
                .iter()
                .chain(roll_lanes.iter())
                .map(|(_, id)| concat_project::model::Track {
                    id: id.clone(),
                    ..Default::default()
                })
                .collect::<Vec<_>>(),
        );
        let mut segments: Vec<VoiceoverSegment> = Vec::with_capacity(voiced.len());
        for (clip, item) in &voiced {
            let Some((_, lane)) = lanes.iter().find(|(from, _)| from == &clip.track_id) else {
                continue;
            };
            let sibling_clip_id = mint.next("c");
            let mut restored = concat_project::model::Clip::blank(
                sibling_clip_id.clone(),
                lane.clone(),
                ClipKind::Audio,
                clip.name.clone(),
                clip.start,
                clip.duration,
            );
            restored.media_id = media_id_of[&item.path].clone();
            restored.source_start = clip.source_start;
            restored.speed = clip.speed;
            restored.speed_curve = clip.speed_curve.clone();
            restored.volume = clip.volume;
            restored.fade_in = clip.fade_in;
            restored.fade_out = clip.fade_out;
            restored.preserve_pitch = clip.preserve_pitch;
            restored.audio_stream = clip.audio_stream;
            restored.filters = clip.filters.clone();
            sibling.active_mut().clips.push(Arc::new(restored));
            segments.push(VoiceoverSegment {
                source_clip_id: clip.id.clone(),
                sibling_clip_id,
                take: take_of[&item.path].clone(),
                start: clip.start,
                source_in: clip.source_start,
                source_out: clip.source_start + clip.duration * clip.speed,
                speed: clip.speed,
                volume: clip.volume,
                fades: (clip.fade_in, clip.fade_out),
                source_track: clip.track_id.clone(),
                track: lane.clone(),
            });
        }
        // The carried picture: each source visual clip restored on its
        // own lane, at its own position, in-point, speed and look. The
        // talking-head's video is silenced - the take is its sound now -
        // and every other clip keeps the volume it played with.
        for (clip, item) in &visual {
            let Some((_, lane)) = roll_lanes.iter().find(|(from, _)| from == &clip.track_id) else {
                continue;
            };
            let sibling_clip_id = mint.next("c");
            let mut restored = concat_project::model::Clip::blank(
                sibling_clip_id.clone(),
                lane.clone(),
                clip.kind,
                clip.name.clone(),
                clip.start,
                clip.duration,
            );
            restored.media_id = roll_media_of[&item.path].clone();
            restored.source_start = clip.source_start;
            restored.speed = clip.speed;
            restored.speed_curve = clip.speed_curve.clone();
            restored.volume = clip.volume;
            restored.fade_in = clip.fade_in;
            restored.fade_out = clip.fade_out;
            restored.opacity = clip.opacity;
            restored.scale = clip.scale;
            restored.offset_x = clip.offset_x;
            restored.offset_y = clip.offset_y;
            restored.rotation = clip.rotation;
            restored.stretch_x = clip.stretch_x;
            restored.stretch_y = clip.stretch_y;
            restored.flip_h = clip.flip_h;
            restored.flip_v = clip.flip_v;
            restored.blend = clip.blend.clone();
            restored.filters = clip.filters.clone();
            restored.video_effects = clip.video_effects.clone();
            restored.crop = clip.crop;
            restored.mask = clip.mask;
            restored.transition_in = clip.transition_in.clone();
            // The talking-head's own video: the take clip (placed below)
            // is its sound, so the picture plays muted beside it - the
            // extraction IS this clip's audio, placed where it played.
            if talking_head.is_some_and(|(spoken, _)| spoken.id == clip.id) {
                restored.muted = Some(true);
            }
            sibling.active_mut().clips.push(Arc::new(restored));
        }
        // The talking-head's take clip, after its picture: the words
        // under the face, on the face's own lane position, playing the
        // extracted file from the face's own in-point.
        if let Some((clip, span)) = talking_segment {
            let take_clip_id = mint.next("c");
            let lane = lanes
                .iter()
                .find(|(from, _)| from == &clip.track_id)
                .map(|(_, lane)| lane.clone())
                .unwrap_or_else(|| mint.next("t"));
            let take_path = takes
                .first()
                .map(|take| take.take.clone())
                .unwrap_or_default();
            let take_media = sibling
                .media
                .iter()
                .find(|item| item.path == take_path)
                .map(|item| item.id.clone());
            if let Some(take_media) = take_media {
                let mut restored = concat_project::model::Clip::blank(
                    take_clip_id.clone(),
                    lane.clone(),
                    ClipKind::Audio,
                    clip.name.clone(),
                    clip.start,
                    clip.duration,
                );
                restored.media_id = take_media;
                restored.source_start = clip.source_start;
                restored.speed = clip.speed;
                restored.speed_curve = clip.speed_curve.clone();
                restored.volume = clip.volume;
                restored.fade_in = clip.fade_in;
                restored.fade_out = clip.fade_out;
                restored.preserve_pitch = clip.preserve_pitch;
                sibling.active_mut().clips.push(Arc::new(restored));
                segments.push(VoiceoverSegment {
                    source_clip_id: clip.id.clone(),
                    sibling_clip_id: take_clip_id,
                    take: take_path,
                    start: clip.start,
                    source_in: clip.source_start,
                    source_out: (clip.source_start + clip.duration * clip.speed).min(span),
                    speed: clip.speed,
                    volume: clip.volume,
                    fades: (clip.fade_in, clip.fade_out),
                    source_track: clip.track_id.clone(),
                    track: lane,
                });
            }
        }
        let settings = concat_project::doc::DocumentSettings {
            name: info.name.clone(),
            width: video.width,
            height: video.height,
            rate_num: video.rate_num,
            rate_den: video.rate_den,
        };
        // The sibling's own document refusing to save is the pane's
        // toast, not a lower layer's sentence, so it is a localized
        // line with the document layer's own reason riding its place.
        let document = concat_project::doc::to_document(&settings, &sibling)
            .map_err(|error| tf("forge.voiceoverSaveFailed", &[&error]))?;
        concat_host::projects::save(&info.path, &document)?;
        // The mapping is the provenance record: which sibling clip plays
        // which source clip, with everything the reconstruction kept.
        let mut mapping = serde_json::Map::new();
        mapping.insert(
            "schema".to_owned(),
            serde_json::json!("funcut.voiceover-mapping/1"),
        );
        // The version as its own number, beside the schema's own string:
        // a consumer that wants to branch on the number should not have to
        // parse it out of the name. Constant 1 - the version the schema
        // string already carries - and absent from the mappings the first
        // builds wrote, so a reader takes missing as 1.
        mapping.insert("schemaVersion".to_owned(), serde_json::json!(1));
        // The mapping's refusals - serializing its segments, pretty-
        // printing it, writing the file - are the pane's own words and
        // land as the toast itself, so they read as one localized line:
        // to the reader the three are one sentence either way.
        mapping.insert(
            "segments".to_owned(),
            serde_json::to_value(&segments)
                .map_err(|error| tf("forge.voiceoverMappingFailed", &[&error]))?,
        );
        let bytes = serde_json::to_vec_pretty(&mapping)
            .map_err(|error| tf("forge.voiceoverMappingFailed", &[&error]))?;
        std::fs::write(
            std::path::Path::new(&info.path).join("voiceover-mapping.json"),
            bytes,
        )
        .map_err(|error| tf("forge.voiceoverMappingFailed", &[&error]))?;
        Ok(BuiltSibling {
            files: copied,
            segments,
            takes,
        })
    })();
    match built {
        Ok(BuiltSibling {
            files: copied,
            segments,
            takes,
        }) => {
            // The AUTO EDIT's fixed paths: the export folder is made now,
            // so the run's first write into it is the export itself.
            let auto = if for_auto_edit {
                let export_dir = std::path::Path::new(&info.path).join("export");
                if let Err(error) = std::fs::create_dir_all(&export_dir) {
                    let cleanup = std::fs::remove_dir_all(&info.path);
                    if let Err(cleanup_error) = cleanup {
                        log::warn!(
                            "could not remove incomplete AUTO EDIT sibling {}: {cleanup_error}",
                            info.path
                        );
                    }
                    // The refusal is the pane's own wording, and it lands
                    // as the toast itself - the same kind of sentence
                    // `forge.projectFailed` is - so it is a localized
                    // line, with the folder and the file system's own
                    // reason riding its two places.
                    return Err(tf(
                        "forge.autoEditExportFolderFailed",
                        &[&export_dir.display(), &error],
                    ));
                }
                Some(AutoEditSource {
                    project: info.path.clone(),
                    frame: Some((video.width, video.height)),
                    takes,
                    placements: segments
                        .iter()
                        .map(|segment| TakePlacement {
                            take: segment.take.clone(),
                            start: segment.start,
                            source_in: segment.source_in,
                            source_out: segment.source_out,
                            speed: segment.speed,
                        })
                        .collect(),
                    export: export_dir
                        .join("auto-edit.mp4")
                        .to_string_lossy()
                        .into_owned(),
                    sheet: "auto-edit-sheet.png".to_owned(),
                    director: None,
                    // The button's own script lands here after the sibling
                    // is built (auto_edit_start knows it; the builder does
                    // not), so the construction names the honest default.
                    script: None,
                    // The same honesty for the licence word: the builder
                    // does not know the pressed switch, so the sibling is
                    // born under the licence-safe default and the run that
                    // knows the press overwrites it - never the other way.
                    monetized: true,
                })
            } else {
                None
            };
            Ok((
                VoiceoverSource {
                    project: info.path,
                    files: copied,
                    segments,
                },
                auto,
            ))
        }
        Err(error) => {
            // This sibling project was created solely for this run; do not
            // leave a half-populated project behind after a failed build.
            if let Err(cleanup) = std::fs::remove_dir_all(&info.path) {
                log::warn!(
                    "could not remove incomplete Forge project {}: {cleanup}",
                    info.path
                );
            }
            Err(error)
        }
    }
}

/// Whether a value from the environment is actually configured. An
/// exported-but-empty variable is common in shell profiles and is not a
/// usable endpoint or credential.
fn configured_value(value: Option<String>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

fn env_configured(name: &str) -> bool {
    configured_value(std::env::var(name).ok())
}

/// Whether a vision critic answers without any setup: the environment the
/// editor was started with, a key the Forge page keeps (handed to the
/// spawned harness and to the in-process AUTO EDIT's tool calls alike), or
/// the saved `vlm.json` the `local_vlm {action:"configure"}` tool writes.
/// Blank exported variables do not count as a critic: treating them as
/// configured skips the local setup and silently leaves craft_critique on
/// its heuristic tier.
fn hosted_critic_ready(studio: &Studio) -> bool {
    if env_configured("FUNCUT_VLM_ENDPOINT") || env_configured("FUNCUT_VLM_API_KEY") {
        return true;
    }
    // A provider key the page keeps, or one already exported, is a hosted
    // critic the run can reach: the craft critique's strong tier resolves
    // exactly these spellings.
    let forge = &studio.prefs.forge;
    let provider_keys = [
        (&forge.groq, "GROQ_API_KEY"),
        (&forge.gemini, "GEMINI_API_KEY"),
        (&forge.zai, "ZAI_API_KEY"),
        (&forge.openrouter, "OPENROUTER_API_KEY"),
    ];
    if provider_keys
        .iter()
        .any(|(field, name)| !field.trim().is_empty() || env_configured(name))
    {
        return true;
    }
    // The saved provider config, written by the local_vlm tool. A key or
    // an endpoint in it means a critic is configured; the masked replies
    // the tool gives never carry the whole key, but the file does.
    let path = studio.host.dirs.data.join("vlm").join("vlm.json");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return false;
    };
    let Ok(saved) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    let key = saved
        .get("apiKey")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|key| !key.trim().is_empty());
    let endpoint = saved
        .get("endpoint")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|endpoint| !endpoint.trim().is_empty());
    key || endpoint
}

/// The one prompt the pane sends. It used to be a single sentence of
/// instructions ("verify with preview_frame, then export") with no creative
/// intent and no way for the agent to judge its own work. Now it carries the
/// project's persisted creative brief and demands a measured
/// render -> inspect -> revise loop before the run may claim success.
///
/// A model that cannot see images cannot do the second half, which is why
/// `FUNCUT_FORGE_MODEL` should name a vision-capable model.
fn director_run_prompt(
    script: &str,
    voiceover: Option<&VoiceoverSource>,
    voice: &str,
    niche: &str,
    mood: &str,
    studio: &Studio,
) -> String {
    let mut brief = String::new();
    if let Some(session) = studio.session.as_ref() {
        let path = std::path::Path::new(session.path()).join("creative-brief.json");
        let saved = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
        if let Some(value) = saved {
            let get = |key: &str| {
                value
                    .get(key)
                    .map(|field| field.to_string())
                    .unwrap_or_else(|| "(unset)".to_owned())
            };
            brief = format!(
                "PERSISTED CREATIVE BRIEF (authoritative; update it with creative_brief if the script changed it):\n\
                     audience={audience}\nplatform={platform}\ngoal={goal}\nthesis={thesis}\n\
                     curiosityGap={gap}\npayoff={payoff}\nemotionalArc={arc}\n\
                     referenceStyle={reference}\ntone={tone}\navoid={avoid}\n\n",
                audience = get("audience"),
                platform = get("platform"),
                goal = get("goal"),
                thesis = get("thesis"),
                gap = get("curiosityGap"),
                payoff = get("payoff"),
                arc = get("emotionalArc"),
                reference = get("referenceStyle"),
                tone = get("tone"),
                avoid = get("avoid"),
            );
        }
    }
    if brief.is_empty() {
        brief = "NO CREATIVE BRIEF IS SAVED. Your FIRST tool call must be creative_brief \
                 {action:\"set\"} with audience, platform, goal, thesis, curiosityGap, \
                 payoff, emotionalArc (3+ distinct beats) and referenceStyle. Invent a \
                 sharp angle from the script; do not proceed without one.\n\n"
            .to_owned();
    }
    let critic = if hosted_critic_ready(studio) {
        "A vision critic is configured. Prefer craft_critique with autoSetup:true \
         so a model, not a heuristic, grades the frames.\n"
            .to_owned()
    } else {
        "If no local vision critic is running, call local_vlm {action:\"setup\"} once \
         (0.5B, ~546 MB, CPU-only, no system install) so craft_critique can see the \
         frames rather than only measure pixels - or local_vlm {action:\"configure\"} \
         with a free-tier provider (groq, gemini, zai, openrouter) for a stronger critic.\n"
            .to_owned()
    };
    // How the narration arrives and what the run builds around. A
    // voiceover run has its material already: the files are named, the
    // TTS is off, and the edit is the whole job. A niche chosen as the
    // director's own is a brief to decide, not a playbook to apply.
    let (material, tail) = if let Some(source) = voiceover {
        let files = source
            .files
            .iter()
            .map(|file| format!("  - {file}\n"))
            .collect::<String>();
        let lanes = source
            .segments
            .iter()
            .map(|segment| segment.track.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        let material = format!(
            "THE VOICEOVER (already recorded - do NOT run tts_generate, do NOT write a script):\n\
             {files}\
             Open the project folder '{project}' (project_open). The narration is ALREADY on \
             the timeline, reconstructed clip for clip on the voiceover lane(s) {lanes} with \
             every clip's own start, in/out points, speed, volume and fades - the gaps and \
             overlaps it played with are the edit's timing, and voiceover-mapping.json in the \
             project root maps each sibling clip to its source. Do NOT re-import, re-place, \
             retime or ripple the voiceover clips. Direct the WHOLE edit around that \
             pre-placed narration: repair a rough take before analysing it (Phase 2), tighten \
             only the dead air (Phase 7 - a dramatic hold is the performance, keep it), then \
             the full pipeline: emotion and voice-signal reads, plan, B-roll, captions, \
             motion comps, ONE grade, the watch/critique loop, export.\n\n",
            project = source.project,
        );
        let tail = if script.trim().is_empty() {
            String::new()
        } else {
            format!("EXTRA DIRECTIONS FROM THE USER:\n{script}")
        };
        (material, tail)
    } else {
        (
            format!(
                "The narration does not exist yet: write it from the script below and read it \
                 with tts_generate ({voice}, a Kokoro voice).\n\n"
            ),
            format!("THE SCRIPT:\n{script}"),
        )
    };
    let look = if niche == UNIVERSAL {
        format!(
            "PRODUCTION CHOICES: yours to make - derive the look from the brief and the \
             material (prompts/niche_playbooks.md is a reference of looks, not a rail to \
             stay on). Music for the mood '{mood}', measured word-synced captions, ONE \
             consistent grade, and silence removed from the voiceover.\n\n"
        )
    } else {
        format!(
            "PRODUCTION CHOICES: the {niche} playbook as a starting point (the brief and the \
             material win where they disagree), music for the mood '{mood}', measured \
             word-synced captions, ONE consistent grade, and silence removed from the \
             voiceover.\n\n"
        )
    };
    format!(
        "Use the funcut-director skill and follow its 25 phases IN ORDER. You are directing a \
         video, not assembling clips. The skill rides with two judgment shelves you MUST \
         consult, not decorate with: maestro (design, motion and film judgment - its shot \
         cards, its 157-shot vocabulary, its sound-design method) and clip-skills (the \
         editing craft - where to cut, the audio spine, B-roll matching, pacing, grading, \
         voice processing, export numbers). Phase 9 draws on clip-skills' audio-driven \
         cutting module and maestro's shot cards for scale, motion and register; Phase 13 \
         draws on clip-skills' B-roll matching module; name the module each decision used \
         in the plan's signalLog notes. An edit that never opened a shelf is an edit that \
         guessed.\n\n{brief}{material}\
         {look}\
         MANDATORY QUALITY LOOP (do not skip, do not just describe it):\n\
         1. plan_validate must return BOTH valid:true AND creativeReady:true. Fix every \
            creativeBlockers entry before cutting. A score below 7 means rewrite the plan.\n\
         2. Never choose B-roll from caption text. Shop the licensed live shelves \
            (stock_search; the Pexels/Pixabay keys are in your environment and this edit \
            is monetized - OpenVid has no licence record and its import is refused), \
            fetch, then stock_contact_sheet with the fetched paths and LOOK at the \
            returned image; use cropHint when fitting square or landscape clips into a \
            vertical frame.\n\
         3. motion_frame returns an inline image - check legibility, contrast and safe \
            margins before every motion_bake.\n\
         4. After the edit, call watch_render (the WHOLE cut as one contact sheet image) \
            then craft_critique. {critic}\
         5. Act on at least one revision from craft_critique and call watch_render AGAIN. \
            Compare the two sheets and their craft numbers. Repeat until the overall score \
            is >= 7, with no black frames, letterbox bars or long static runs.\n\
         6. Export, then call watch_render {{path: <the exported file>, audio:true}} and \
            craft_critique one final time. Report the honest score and what you changed; \
            never claim success on an unmeasured render.\n\n\
         A file PATH to a PNG is not a picture you can see. Only the inline image content \
         blocks from these tools are. If a tool returns an image, look at it.\n\n\
         {tail}"
    )
}

/// Whether a model key is saved for the harness: one of the eight LLM
/// provider keys the pane hands the harness for thinking. The HuggingFace
/// key is not among them - it only feeds the emotion sidecar - and a bare
/// `FUNCUT_FORGE_MODEL` names a model without paying for one, so neither
/// can carry a Director run on its own. A harness with no key cannot
/// think, so a Director run without one would only fail after the sibling
/// was built.
fn a_model_key_is_saved(studio: &Studio) -> bool {
    let forge = &studio.prefs.forge;
    [
        &forge.groq,
        &forge.gemini,
        &forge.zai,
        &forge.openrouter,
        &forge.mistral,
        &forge.cerebras,
        &forge.sambanova,
        &forge.nim,
    ]
    .iter()
    .any(|key| !key.trim().is_empty())
}

/// Whether the AUTO EDIT can hand its edit to the Director: the harness
/// is on PATH, the Forge page has wired it to this editor, the skill is
/// installed, and a model key is saved. Every one of those is a thing the
/// user can see on the page, so a run that cannot hand over says which is
/// missing instead of quietly running the fixed spine and calling it the
/// same edit.
///
/// The wiring answers come in as arguments, read from the pane's own
/// fields: while `update` runs, the studio's copy of the pane is a blank
/// `mem::take` left behind, so reading `studio.forge` here would answer
/// "not connected" on every call and the handover could never happen.
///
/// The refusals are the pane's own words for the user - the retry path
/// shows one as the toast itself, and the first run embeds one inside
/// `forge.autoEditDirectorFallback`'s {0} - so each is a localized line a
/// locale can carry, never an English sentence baked into the code.
fn director_ready(
    connected: Option<bool>,
    installed: Option<bool>,
    studio: &Studio,
) -> Result<(), String> {
    // A saved key is enough to know the user means to use a model; the
    // harness itself still has to exist and be wired, which the page's
    // own probe answers.
    if !a_model_key_is_saved(studio) {
        return Err(t("forge.autoEditDirectorNoModelKey"));
    }
    if connected != Some(true) {
        return Err(t("forge.autoEditDirectorNotConnected"));
    }
    if installed != Some(true) {
        return Err(t("forge.autoEditDirectorSkillMissing"));
    }
    // The surface, not just the wiring. A run told to verify its export
    // meets a refusal mid-edit if the server it talks to does not serve
    // the call - and the edit it already made stays on the sibling. So
    // the tools the run cannot finish without are checked here, against
    // the same environment the harness's server reads, and a missing one
    // stops the handover before anything is cut.
    let unserved = concat_mcp::protocol::unserved(concat_mcp::protocol::DIRECTOR_REQUIRED);
    if !unserved.is_empty() {
        // The tool names ride the {0} as they are - names, not words - and
        // the line itself dodges the one-or-many pronoun the old English
        // picked between, so one localized line serves both counts.
        return Err(tf(
            "forge.autoEditDirectorSurfaceUnserved",
            &[&unserved.join(", ")],
        ));
    }
    Ok(())
}

/// The prompt an AUTO EDIT hands the Director: the same skill, the same
/// shelves and the same quality loop a Start Forge run gets, plus the
/// facts only this button knows - the sibling it built with the narration
/// placed, and the export the run should land. The measurements are NOT
/// done for it: the Director runs the reads itself, in the skill's order,
/// and the prompt says so, so the pane never presents a read as already
/// spent.
fn auto_director_prompt(source: &AutoEditSource, studio: &Studio) -> String {
    // The vision critic the quality loop stands on: the same readiness the
    // Start prompt reads, so an AUTO EDIT's craft_critique is graded by a
    // model that can see, and an unattended run is told to set one up
    // rather than quietly grading its own pixels.
    let critic = if hosted_critic_ready(studio) {
        "A vision critic is configured. Prefer craft_critique with autoSetup:true \
         so a model, not a heuristic, grades the frames.\n\n"
            .to_owned()
    } else {
        "Before the quality loop, call local_vlm {action:\"setup\"} once (0.5B, \
         ~546 MB, CPU-only, no system install) so craft_critique can SEE the frames \
         rather than only measure pixels - or local_vlm {action:\"configure\"} with a \
         free-tier provider (groq, gemini, zai, openrouter) for a stronger critic. \
         A critique from a model that cannot see is a guess wearing a score.\n\n"
            .to_owned()
    };
    // The frame in numbers, said once at the top: the vocabulary below
    // says the shapes in words, and a Director handed a vertical reel
    // should never have to read the project to learn which number it is.
    let frame = source.frame.map(|(width, height)| {
        let shape = if height > width {
            "a vertical reel"
        } else if width > height {
            "a horizontal cut"
        } else {
            "a square"
        };
        format!(
            "THE FRAME: {width}x{height} ({shape}). The timeline is already this shape: \
             do not set it, spend it - a 16:9 clip unreframed in a vertical reel is a \
             letterboxed frame, not a stylistic choice.\n\n"
        )
    });
    let frame = frame.unwrap_or_default();
    // The licence word the press carried, said to the agent in the one
    // place it decides shelves and beds: a monetized edit is refused
    // OpenVid's unrecorded licence and the music model's non-commercial
    // beds, and one that is not may spend them. The word rides the
    // source, so this prompt and the spine's program answer one choice
    // - and the brief the Director writes carries it, the way the
    // spine's own brief would.
    let licence = if source.monetized {
        "THE LICENCE: this edit is monetized. OpenVid has no licence record \
         and its import is refused; the music model's non-commercial beds \
         are refused with it. Set the brief's monetized flag accordingly.\n\n"
            .to_owned()
    } else {
        "THE LICENCE: this edit is not monetized. OpenVid's unrecorded \
         shelf and the music model's non-commercial beds may be spent. \
         Set the brief's monetized flag accordingly.\n\n"
            .to_owned()
    };
    format!(
        "Use the funcut-director skill and follow its 25 phases IN ORDER. This is an \
         UNATTENDED one-click edit: nobody is watching to answer a question, so make every \
         creative decision yourself and record it.\n\n\
         {frame}{licence}\
         READ THE DEEP PLAYBOOKS BEFORE YOU DECIDE: read .claude/skills/funcut-director/SKILL.md, \
         prompts/creative_direction.md, prompts/style_bible.md, prompts/ai_media.md, prompts/edit_plan.md with templates/edit_plan.schema.json \
         and the example, prompts/motion_comps.md with .claude/skills/motion/SKILL.md, \
         prompts/quality_loop.md, prompts/power_moves.md, prompts/signal_matrix.md, \
         prompts/signal_recipes.md, prompts/emotion_to_edit.md, prompts/token_budget.md \
         and prompts/tool_reference.md. The skill is the pipeline; the playbooks are the \
         craft. Do not plan from memory.\n\n\
         ALL-SIGNAL CONTRACT: voice_signals exposes 71 named readings across 14 families. Read \
         prompts/signal_matrix.md and spend the readings, not just the family names: use the \
         accepted signalLog family alias from the matrix, name the exact catalogue reading in \
         its evidence note, and record confidence plus the decision it changed; a reading that \
         was absent is named unmeasured, never cited at confidence 0. A silent family is a broken promise, not a neutral choice. Then use \
         prompts/signal_recipes.md: score the seam, set the cut density, choose the cut type, \
         and fall back conservatively when confidence is low.\n\n\
         EMOTION IS A RECIPE: prompts/emotion_to_edit.md turns each of the seven labels and \
         its intensity into pace, cut types, captions, sound, grade, motion and duck. The act \
         wins the structure; the label wins the texture; log both.\n\n\
         TOKEN BUDGET: prompts/token_budget.md is the 10-minute estimate (250-500k typical, \
         800k hard stop) and the reduction rules. Read each tool result once, keep a compact \
         per-section note, contact-sheet before metadata, cap candidates at three, one plan, \
         one critique per pass, one revision before the re-watch. Never cut the first three \
         seconds, the turn, the voice read, the contact sheet, the frame proof or the whole-cut \
         watch to save tokens.\n\n\
         MULTI-AGENT, WHEN THE HARNESS EXPOSES IT: if spawn_agents or /workflow is available, \
         fan out the independent read-only work - one agent per stock slot, one per motion beat, \
         one for sound, one for the story map, and a fresh-context critic. Subagents return \
         JSON, paths and reasons, never edits. YOU are the only writer: only this thread calls \
         edit_apply or project_save. If no subagent tool exists, parallelise \
         independent tool calls and keep every write sequential; do not pretend a fan-out.\n\n\
         THE PROJECT: open '{project}' (project_open). The narration is ALREADY on the \
         timeline, reconstructed clip for clip with its own starts, in/out points, speed, \
         volume and fades; voiceover-mapping.json maps each clip to its source. Do NOT \
         re-import, re-place or otherwise retime the narration clips - with ONE \
         exception: you MAY tighten measured dead air with removeSpans on the narration \
         clips (ripple on), which the skill's own Phase 7 asks for. The WORDS must \
         survive the tighten: never cut inside a word, and verify with emotion \
         {{action:\"verify\"}} on the exported file before you claim success.\n\n\
         {critic}\
         WHAT IS ALREADY DONE, AND WHAT IS NOT: the sibling exists and the narration is \
         placed; nothing has been MEASURED yet. The repair, the transcription, \
         voice_signals and emotion are YOURS to run on this sibling, in the skill's order - \
         do not skip them and do not assume them. Every creative decision must name the \
         signal that drove it in the plan's signalLog, with the confidence the profile \
         rated it at and a one-sentence basis - an edit that never read the voice is \
         an edit that guessed, and an unavailable signal must be reported as missing rather \
         than claimed.\n\n\
         READ THE PROFILE WHOLE, THEN SPEND IT. voice_signals returns more than highlights: \
         read `decisionPolicy` (which signals drive cut, caption, graphic, bgm and \
         transition), `signalConfidence.perSignal` (what was measured, how strongly, and on \
         what basis), `directorHints` (cutStyle, bgm.recommendation, speakerChanges, \
         questions, emphasisWords, captionGate, protectedPauses) and `wholeTake` (the \
         forensic numbers, the loudness, the rhythm layers, speechRatio). Spend each family \
         on the decision the policy names for it: pauses and emphasis size the cuts, \
         speechRate.pace sets their length, words and sentence drive captions, entities and \
         claims become graphics, speakerChanges and topics cut the angle or the colour, \
         noiseFloor and the forensic numbers choose and duck the bed, loudness and prosody \
         set the pacing, motifs and rhythmLayers keep the shape. A family the profile rated \
         low (see `signalConfidence`) is spent with less weight and its basis is named; a \
         family at confidence 0 was not measured, so say so and do not cite it. Never dump \
         the profile into a note: every signal you read must land on a choice.\n\n\
         {script}\
         FIRST THE BRIEF: call creative_brief {{action:\"set\", path:'{project}', brief:...}} \
         before any planning tool; the plan carries that brief and a plan without one is not \
         creativeReady. Run a three-concept bake-off first: write three different films that could answer the brief, say what each would make the viewer feel, and choose one for a stated reason - then write the style bible (`style` in the plan: palette, grade, caption system, motion hand, sound world, footage treatment, continuity rules) and the retention contract (`retention`: hook, turnAt, payoffAt, patternBreaks, openLoops). Both are creativeReady blockers when absent. Then write the Phase 5 story map, not a shot list with no argument.\n\n\
         WHAT THE EDIT MUST BE: the whole pipeline around that narration - the plan \
         (plan_validate must return valid:true AND creativeReady:true), B-roll shopped on \
         the licensed live shelves (stock_search with the keys in your environment, \
         stock_fetch, then stock_contact_sheet with the fetched paths) and LOOKED AT \
         before choosing and gated with \
         footage_continuity before anything is imported (a flagged slot gets a different \
         clip or holds). The live shelves in this build are VIDEO-ONLY: do not claim a stock still, and do not use a 16:9 clip unreframed in a 9:16 reel - fill and reframe it, or make a still from a clip with preview_frame, or use a generated image. If a real generator (the Higgsfield CLI) is installed, use prompts/ai_media.md: one nano_banana_2 style key first, then reference it for every still, then seedance_2_0/gemini_omni image-to-video for impossible shots; download into the project and media_import. If it is absent, say so and use footage, stills and Geneva comps - never fake a generated asset. captions cut on the measured words in groups of two to four \
         (punctuation, breath gaps and four words are the seams; the safe-remove fillers \
         never show; the delivery's stressed words take the accent through \
         title_style_words), Geneva motion comps for the beats that earn them \
         (motion_frame to check legibility before motion_bake; the ops vocabulary \
         author - set-markup included - is yours to compose with, not only the presets), \
         transitions drawn from the catalogue's full breadth and never the same move \
         twice running, ONE consistent grade whose intensity follows the acts (hook full, \
         body settled, cta lifted - effect_key_set on the layer), music ducked under the \
         voice with a shape that follows the lines (the bed lifts in the gaps that can \
         hold it), accents on the measured emphasis peaks landed on their peak (the \
         manifest's syncAt says where it is). The first three seconds are the hook: they \
         earn the strongest push-in, the loudest word and an entrance accent whatever the \
         mood. Fit the frame the project actually is - a vertical reel and a horizontal \
         cut are different edits. Use the editor's whole vocabulary; do not settle for a \
         template. Use masks, subject tracking, 3D set-camera moves with depths, bone rigs, template_instantiate, kinetic captions, motion reels and \
         overlays, scene analysis, clip_speed and clip_transform for a designed retime or \
         reframe, tts_generate only for a missing pickup, and the full transition catalogue \
         where the story earns them - not as decoration.\n\n\
         AUTO EDIT'S OWN QUALITY CONTRACT - the button asks for this even when the prompt is short, and the acceptance reads it:\n\
         - STORY ARCHITECTURE: write the five beats before the plan - hook (what the first three seconds show and the question it plants), curiosity gap (the question the viewer needs answered), progression (the argument or feeling turning), payoff (the line or image that answers it) and ending (the short landing after it). Put them in `retention` and the story map, and make every section serve one beat.\n\
         - RETENTION ANALYSIS: before cutting, identify boring stretches (long low-energy spans), repeated explanations (the same idea told twice without a new image) and weak transitions (a cut that keeps the same act, mood and angle). Record them in `retention.analysis` with their times, then move pattern breaks, cuts or a new visual to each finding. A clean cut with no retention pass is not ready.\n\
         - VISUAL VARIETY: no B-roll style, camera movement, caption animation or transition may repeat twice running. Rotate shot scale (wide / medium / close), camera move (push / slide / whip / hold), B-roll category and caption entrance; a repeated choice is only allowed when the motif is the point, and the plan says so. Record the rotation in `visualVariety`.\n\
         - BEAT-AWARE EDITING: match cuts, motion and sound to meaningful speech beats - the measured emphasis peaks, sentence ends, topic boundaries and the music's downbeats - not to arbitrary seconds. A cut on a beat that carries no word is a metronome, not an edit; the signalLog names the beat each one spent.\n\
         - THREE DIRECTIONS: audition three genuinely different films (for example evidence-first documentary, kinetic explainer, cinematic essay), say what each would make the viewer feel, and choose one for a stated reason. The chosen direction sets the style bible, the grade family, the caption hand and the B-roll vocabulary; the other two stay in the plan as rejected alternatives.\n\
         - TOPIC-SPECIFIC DIRECTION: the direction comes from the material, not a default. A finance claim, a history archive, a fitness tutorial and a personal story do not get the same look, pace or stock vocabulary; name the topic evidence that chose the direction.\n\n\
         STORYBOARD AND MOTION CODE: before placement, write the scene/beat/transition map (Phase 6), the storyboard (Phase 10), the motion decision (Phase 11) and the Geneva deep design (Phase 12) for every section. Each section has a designed beat or a \
         stated reason it does not; every number, percentage, comparison, ranking, trend or \
         process becomes a purpose-built comp (ticker, chart, triptych, compare) - never a plain \
         text card. A textCard or titleCard carries a complete visual.storyboard accepted by \
         motion_reel; author the comp/ops document, motion_inspect it, motion_frame the \
         entrance/settled/exit, then motion_bake at the cut size; use motion_overlay for \
         transparent comps and captions_animate for word-timed kinetic captions. The plan's \
         signalLog records the graphic decision and its reason.\n\n\
         LIVE TIMELINE: after every edit_apply batch, timeline_read the project; on the \
         window's own server, timeline_seek the moment just placed so the user sees it arrive. \
         On a headless server, preview_frame each section boundary and watch_render each act. \
         The timeline is the deliverable; a decision that is not on it did not happen.\n\n\
         MANDATORY QUALITY LOOP: watch_render the OPEN timeline, craft_critique it, act on \
         at least one revision, watch_render AGAIN and compare the two craft scores, and \
         repeat until the overall score is >= 7 with no black frames, letterbox bars or long \
         static runs. Then collision_check the timeline and fix every shared lane, and \
         project_save so the edit the acceptance opens is the edit you judged. Then hand off the \
         timeline; do not export it.\n\n\
         INDEPENDENT CRITIC: the editor who made the cut is the worst judge of it. If the \
         harness exposes subagents, send a fresh-context critic the rendered contact sheet, the \
         brief and the plan - not this conversation - and ask for findings, not a rubber stamp. \
         Otherwise run craft_critique inline and say the critique was not independent. Act on \
         the highest-leverage finding, then watch_render the whole cut again. Below 7 is not \
         ready.\n\n\
         PERSIST THE PLAN: write the exact canonical plan object you validated to \
         '{project}/edit-plan.json' before you save - the run's acceptance gate reads that \
         file as its story evidence, and an edit without it lands unproven whatever its \
         craft score.\n\n\
         THE DELIVERABLE IS THE TIMELINE, NOT A RENDER: do NOT call export_start or \
         export_wait. The user wants a finished timeline they can customize. Finish with \
         project_save, then prove the timeline: watch_render the OPEN project with audio:true, \
         craft_critique its sheet, collision_check the project, plan_validate the saved plan, and \
         timeline_read the project. The pane's independent acceptance runs the same timeline \
         checks; a skipped words/LUFS/export gate is honest, not a pass. Report the measured \
         craft score, the gates, and what you changed. Exporting is the user's next click, not \
         yours; if you rendered by mistake, leave the timeline as the deliverable and say so. A \
         file PATH to a PNG is not a picture you can see: only inline image blocks are.\n\n\
         {stock}\n\n         GENERATIVE MEDIA: {generator}",
        project = source.project,
        critic = critic,
        script = source.script.as_deref().map_or_else(String::new, |script| {
            format!(
                "THE SCRIPT (the author's own wording, authoritative): pass it as the \
                 `script` argument of every voice_signals call. The reading aligns the \
                 transcript against it - misheard words are corrected to this spelling \
                 while the measured timings stay - and its reply carries a \
                 `scriptAlignment` verdict. Captions, entity cards and B-roll queries \
                 spend these words, never the recogniser's guesses. The script:\n{script}\n\n"
            )
        }),
        stock = if stock_shelves_ready(studio) {
            "The stock shelves are keyed (Settings > Stock APIs): shop them for B-roll.".to_owned()
        } else {
            "NO stock key is saved (Settings > Stock APIs), so stock_search will refuse: \
             carry the picture with the project's own footage, Geneva motion comps and \
             typography over the narration instead - do not leave unpictured spans."
                .to_owned()
        },
        generator = if ai_generator_ready() {
            "A real generator is installed: the Higgsfield CLI answered. Read prompts/ai_media.md, \
             generate one style key first, then reference it for every still and image-to-video, \
             download into the project and media_import. Record model, prompt and licence."
                .to_owned()
        } else {
            "NO generative media CLI is installed (higgsfield was not found). Do not invent a \
             generated asset: use the project's footage, licensed video stock, stills from \
             preview_frame, Geneva comps and typography, and say which one you used."
                .to_owned()
        },
    )
}

/// Whether a live stock shelf can be shopped at all: a Pexels or Pixabay
/// key saved on the page or already in the environment. The AUTO EDIT's
/// preflight says this to the user BEFORE the run spends minutes
/// discovering it one refused search at a time, and the Director's prompt
/// says it so the agent plans typography instead of begging a locked
/// shelf.
fn stock_shelves_ready(studio: &Studio) -> bool {
    let stock = &studio.prefs.stock;
    [
        (&stock.pexels, "PEXELS_API_KEY"),
        (&stock.pixabay, "PIXABAY_API_KEY"),
    ]
    .iter()
    .any(|(field, name)| {
        !field.trim().is_empty() || std::env::var(name).is_ok_and(|set| !set.trim().is_empty())
    })
}

/// Whether a real generative-media CLI is installed. The Higgsfield shelf
/// is the supported door (images, video, 3D, audio); the prompt reads this
/// answer so an unattended run either uses a generator that exists or says
/// plainly that none does, instead of pretending an asset was made.
fn ai_generator_ready() -> bool {
    Command::new("higgsfield")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Starts the Director over the AUTO EDIT's sibling: the same harness a
/// Start Forge run spawns, given the sibling this run already built and
/// measured. The pane shows the harness's words as they come, and the
/// run's end opens the finished edit in the window the way a Start Forge
/// run does.
fn launch_auto_director(studio: &Studio, source: &AutoEditSource, kill: Arc<AtomicBool>) {
    let prompt = auto_director_prompt(source, studio);
    let mut command = Command::new("forge");
    command.arg("run");
    let model = std::env::var("FUNCUT_FORGE_MODEL")
        .or_else(|_| std::env::var("FORGE_MODEL"))
        .unwrap_or_default();
    if !model.trim().is_empty() {
        command.arg("--model").arg(model.trim());
    }
    command.arg(prompt);
    let forge = &studio.prefs.forge;
    let stock = &studio.prefs.stock;
    for (name, value) in [
        ("GROQ_API_KEY", &forge.groq),
        ("GEMINI_API_KEY", &forge.gemini),
        ("ZAI_API_KEY", &forge.zai),
        ("OPENROUTER_API_KEY", &forge.openrouter),
        ("MISTRAL_API_KEY", &forge.mistral),
        ("CEREBRAS_API_KEY", &forge.cerebras),
        ("SAMBANOVA_API_KEY", &forge.sambanova),
        ("NVIDIA_API_KEY", &forge.nim),
        ("FUNCUT_EMOTION_API_KEY", &forge.hf),
        ("HF_TOKEN", &forge.hf),
        ("PEXELS_API_KEY", &stock.pexels),
        ("PIXABAY_API_KEY", &stock.pixabay),
    ] {
        if !value.trim().is_empty() {
            command.env(name, value.trim());
        }
    }
    configure_forge_process_group(&mut command);
    let spawned = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    match spawned {
        Ok(child) => {
            crate::host::on_ui(|studio, _, _| {
                studio.handle(Msg::Forge(ForgeMsg::Line(t(
                    "forge.autoEditDirectorHasEdit",
                ))));
            });
            read_the_director_run(child, kill);
        }
        Err(error) => {
            crate::host::on_ui(move |studio, _, _| {
                studio.handle(Msg::Forge(ForgeMsg::Line(tf(
                    "forge.autoEditDirectorStartFailed",
                    &[&error],
                ))));
                studio.handle(Msg::Forge(ForgeMsg::AutoDirectorEnded(-1)));
            });
        }
    }
}

/// The Director child's pipes, drained into the pane's log, and its exit
/// reported. The same shape as [`read_the_run`] - one line at a time, a
/// kill flag the supervisor polls - because a Director run is the same
/// kind of process.
fn read_the_director_run(mut child: Child, kill: Arc<AtomicBool>) {
    // The handed-over run has no runner to post a tick. Without this
    // clock the pane's elapsed field only moved when the harness printed
    // a line, so a model thinking for minutes showed 0s and looked dead.
    let tick_still = Arc::new(AtomicBool::new(true));
    let tick_flag = Arc::clone(&tick_still);
    std::thread::spawn(move || {
        while tick_flag.load(Ordering::Relaxed) {
            crate::host::on_ui(|studio, _, _| {
                studio.handle(Msg::Forge(ForgeMsg::AutoEditTick));
            });
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });
    let out = child.stdout.take();
    let err = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        if let Some(pipe) = out {
            // A line that is not UTF-8 is skipped, not the end of the
            // log: stopping at the first bad byte would silently
            // truncate everything the harness said after it.
            for line in BufReader::new(pipe).lines() {
                let Ok(line) = line else { continue };
                crate::host::on_ui(move |studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::Line(line)));
                });
            }
        }
    });
    let stderr_reader = std::thread::spawn(move || {
        if let Some(pipe) = err {
            // A line that is not UTF-8 is skipped, not the end of the
            // log: stopping at the first bad byte would silently
            // truncate everything the harness said after it.
            for line in BufReader::new(pipe).lines() {
                let Ok(line) = line else { continue };
                crate::host::on_ui(move |studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::Line(line)));
                });
            }
        }
    });
    std::thread::spawn(move || {
        let status = supervise_forge_child(child, kill);
        let _ = stdout_reader.join();
        let _ = stderr_reader.join();
        let code = status.and_then(|status| status.code()).unwrap_or(-1);
        tick_still.store(false, Ordering::Relaxed);
        crate::host::on_ui(move |studio, _, _| {
            studio.handle(Msg::Forge(ForgeMsg::AutoDirectorEnded(code)));
        });
    });
}

/// The director skill's phases, in the order it runs them, with the
/// words its own headings and tool calls carry. A handed-over AUTO EDIT
/// has no runner to name its stage, so the pane reads the harness's
/// output instead and advances only when a phase is actually named -
/// the twenty-five headings the skill prints (1 through 25), which is
/// the same sequence a Start Forge run walks.
///
/// The words are the *fallback*: the skill numbers every heading, so the
/// numbered match in `director_phase` decides almost every line, and a
/// word here only places a line the harness emitted without its number.
const DIRECTOR_PHASES: [&[&str]; 25] = [
    &["creative direction", "creative_brief"],
    &["understand the material"],
    &["emotion"],
    &["voice_signals", "voice signal"],
    &["story map", "shot budget"],
    &["scene", "beat map", "transition map"],
    &["tighten", "clip_cut_spans"],
    &["voice edit proof"],
    &["plan_validate", "the edit plan"],
    &["storyboard", "shot list"],
    &["motion design code", "motion_inspect"],
    &["geneva deep", "comp graph", "motion system"],
    &["gather", "stock_search", "b-roll"],
    &["stock review", "footage_continuity", "continuity"],
    &["place", "rough cut"],
    &["motion_bake", "motion design", "geneva"],
    &["caption choreography", "captions_animate", "captions_add"],
    &["sound design", "sounds_add", "duck"],
    &["grade (required)", "the look", "effect_layer"],
    &["grade proof", "preview_frame"],
    &["watch_render", "craft_critique"],
    &["independent critique", "revise"],
    &[
        "project_save",
        "timeline ready",
        "hand off",
        "export_start",
        "export_wait",
    ],
    &[
        "final watch",
        "motion_probe",
        "measure_loudness",
        "timeline acceptance",
    ],
    &["delivery", "receipt", "wow bar"],
];

/// The skill's numbered ladder: the number each heading prints, the
/// numbers 1 through 25, and the pane's phase index for each one. A
/// number the ladder does not carry - a "phase 99" an older prompt might
/// still echo - matches nothing by number, so it can never be read as
/// "phase 1".
const DIRECTOR_PHASE_LADDER: [(u32, &str, usize); 25] = [
    (1, "", 0),
    (2, "", 1),
    (3, "", 2),
    (4, "", 3),
    (5, "", 4),
    (6, "", 5),
    (7, "", 6),
    (8, "", 7),
    (9, "", 8),
    (10, "", 9),
    (11, "", 10),
    (12, "", 11),
    (13, "", 12),
    (14, "", 13),
    (15, "", 14),
    (16, "", 15),
    (17, "", 16),
    (18, "", 17),
    (19, "", 18),
    (20, "", 19),
    (21, "", 20),
    (22, "", 21),
    (23, "", 22),
    (24, "", 23),
    (25, "", 24),
];

/// The stage the pane shows for a phase. The words are spelled out here
/// rather than read from the table above so that every key is a literal
/// the locale inventory can see: a key built from data is a key the
/// checker cannot prove exists.
fn director_phase_stage(index: usize) -> String {
    match index {
        0 => t("forge.directorPhaseBrief"),
        1 => t("forge.directorPhaseUnderstand"),
        2 => t("forge.directorPhaseAudio"),
        3 => t("forge.directorPhaseSignals"),
        4 => t("forge.directorPhasePlan"),
        5 => t("forge.directorPhasePlan"),
        6 => t("forge.directorPhaseTighten"),
        7 => t("forge.directorPhaseTighten"),
        8 => t("forge.directorPhasePlan"),
        9 => t("forge.directorPhasePlan"),
        10 => t("forge.directorPhaseMotion"),
        11 => t("forge.directorPhaseMotion"),
        12 => t("forge.directorPhaseFootage"),
        13 => t("forge.directorPhaseFootage"),
        14 => t("forge.directorPhaseCut"),
        15 => t("forge.directorPhaseMotion"),
        16 => t("forge.directorPhaseCut"),
        17 => t("forge.directorPhaseAudio"),
        18 => t("forge.directorPhaseGrade"),
        19 => t("forge.directorPhaseGrade"),
        20 => t("forge.directorPhaseReview"),
        21 => t("forge.directorPhaseReview"),
        22 => t("forge.directorPhaseExport"),
        23 => t("forge.directorPhaseExport"),
        _ => t("forge.directorPhaseExport"),
    }
}

/// The phase a line of the harness's output names, and how far through
/// the skill's twenty-five phases that is. A phase sits halfway through
/// its own slot while it runs - the last phase is 24.5/25, never 1.0, which
/// only the accepted end of the run may show. A line that mentions two
/// phases is a sentence about them, not a stage, so only the earliest
/// match is taken and the fraction is that phase's own position.
fn director_phase(line: &str) -> Option<(usize, f32)> {
    let lower = line.to_ascii_lowercase();
    let total = DIRECTOR_PHASES.len() as f32;
    let at = |index: usize| (index, (index as f32 + 0.5) / total);
    // A numbered phase is matched by its number: the skill prints
    // "Phase 1" through "Phase 25", so a plain "phase 1" scan must not
    // swallow "phase 15", a greedy digit read must not turn "phase 12"
    // into "phase 1", and a number the ladder does not carry matches
    // nothing by number at all.
    let mut cursor = 0;
    while let Some(found) = lower[cursor..].find("phase ") {
        let rest = &lower[cursor + found + "phase ".len()..];
        let digits: usize = rest
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .map(|c| c.len_utf8())
            .sum();
        let after_digits = &rest[digits..];
        let letters: usize = after_digits
            .chars()
            .take_while(|c| c.is_ascii_lowercase())
            .map(|c| c.len_utf8())
            .sum();
        let after = &after_digits[letters..];
        if digits > 0
            && !after.chars().next().is_some_and(|c| c.is_alphanumeric())
            && let Ok(number) = rest[..digits].parse::<u32>()
            && let Some((_, _, index)) = DIRECTOR_PHASE_LADDER
                .iter()
                .find(|(known, suffix, _)| *known == number && *suffix == &after_digits[..letters])
        {
            return Some(at(*index));
        }
        cursor += found + "phase ".len();
    }
    // A phase the harness names by a tool or a word instead of a number.
    DIRECTOR_PHASES
        .iter()
        .enumerate()
        .find(|(_, words)| words.iter().any(|word| lower.contains(word)))
        .map(|(index, _)| at(index))
}

/// Whether a failed or unaccepted AI Director run still has the exact
/// sibling required for a safe retry. Never rebuild over a missing mapping
/// or missing source take: acceptance would otherwise be unverifiable.
fn director_retry_available(source: &AutoEditSource) -> bool {
    !source.takes.is_empty()
        && source
            .takes
            .iter()
            .all(|take| std::path::Path::new(&take.take).is_file())
        && std::path::Path::new(&source.project)
            .join("concat.json")
            .is_file()
        && std::path::Path::new(&source.project)
            .join("voiceover-mapping.json")
            .is_file()
        && std::path::Path::new(&source.export)
            .parent()
            .is_some_and(|parent| parent.is_dir())
}

/// Whether either editing entry point must stay locked while AUTO EDIT
/// is running or its Director export is being checked.
fn auto_edit_busy(running: bool, checking: bool) -> bool {
    running || checking
}

/// The licence word the page's own switch carries, read at the moment a
/// press reads its script box: the choice belongs to the press, and
/// travels with it. The reading runs where the press does - the window's
/// own thread, the one place the page is reachable - and a window that
/// is gone (or a pane under test, where no shell was ever installed)
/// keeps the licence-safe default: a monetized edit is the one that may
/// not spend the non-commercial beds, so the default never reaches for
/// material it may not use.
fn monetized_choice() -> bool {
    let mut monetized = true;
    crate::host::Shell::with(|_, app| {
        monetized = app.global::<crate::ui::Forge>().get_monetized();
    });
    monetized
}

/// The AI phase is not complete until the independent export gates accept.
fn director_acceptance_fraction(accepted: bool) -> f32 {
    if accepted { 1.0 } else { 0.98 }
}

/// Whether the harness is wired to Concat and the skill is where it
/// reads: `forge mcp get funcut` says the one, `forge commands` lists the
/// other. Neither is a reason to fail: the buttons say what is missing.
fn director_skill_listed(code: i32, words: &str) -> bool {
    code == 0 && words.to_ascii_lowercase().contains("funcut-director")
}

fn probe_setup() -> (Result<bool, String>, Result<bool, String>) {
    let connected = match forge(&["mcp", "get", "funcut"]) {
        Ok((code, _)) => Ok(code == 0),
        Err(error) => Err(error),
    };
    let installed = match forge(&["commands"]) {
        // Supporting shelves do not replace the required Director entry
        // point. A partial install must not pass preflight.
        Ok((code, words)) => Ok(director_skill_listed(code, &words)),
        Err(error) => Err(error),
    };
    (connected, installed)
}

/// Connects the harness to Concat: the add, run only when the server is
/// not wired already. Ok(true) is wired, Ok(false) is not; Err is why it
/// could not be asked.
fn connect_forge() -> Result<bool, String> {
    let (code, _) = forge(&["mcp", "get", "funcut"])?;
    if code == 0 {
        return Ok(true);
    }
    let (code, words) = forge(&[
        "mcp", "add", "funcut", "concat", "--", "--mcp", "window", "-s", "user",
    ])?;
    if code == 0 {
        Ok(true)
    } else {
        // The harness's own first line rides as it comes; the fallback for
        // a harness that said nothing is the pane's own word, so it is a
        // localized line rather than an English fragment riding the
        // toast's {0}.
        Err(first_line(&words).unwrap_or_else(|| t("forge.commandFailed")))
    }
}

/// Installs this repository's skill into the harness.
fn install_skill() -> Result<bool, String> {
    let (code, words) = forge(&["skill", "install", "coreselfmastery/FUNCUT"])?;
    if code == 0 {
        Ok(true)
    } else {
        // The same fallback, for the same reason: the harness's own words
        // as they come, the pane's own word localized when there were
        // none.
        Err(first_line(&words).unwrap_or_else(|| t("forge.commandFailed")))
    }
}

/// Runs `forge` with `args`, capturing what it said: its exit code, and
/// its output - standard error folded in, last words first for the caller
/// that reads the tail. A harness that is not there is Err, in the
/// harness's own spelling.
fn forge(args: &[&str]) -> Result<(i32, String), String> {
    // Bounded: the button's first press asks the harness synchronously
    // (below), and a harness wrapper that starts but never answers - a
    // slow npx-style launcher, a wedged CLI - must not hang the window
    // on the one thread that draws it. Thirty seconds holds a warm
    // skill install; past it the harness is killed and named, and the
    // answer is remembered as a failure rather than awaited forever.
    let mut child = Command::new("forge")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("forge did not answer within 30 s".to_owned());
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    let run = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    let mut words = String::from(String::from_utf8_lossy(&run.stdout));
    words.push('\n');
    words.push_str(&String::from_utf8_lossy(&run.stderr));
    Ok((run.status.code().unwrap_or(-1), words))
}

/// The first line that says anything, of a command's words.
fn first_line(words: &str) -> Option<String> {
    words
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

/// A ToolCaller that carries a waiting job's own reading of its progress
/// out of the reply the wait drew: a status-shaped reply - the export's
/// frames, a model's arrival - reports `fraction` (or `frame` against
/// `total`), and the reading is stored as a fraction of a thousand for
/// the tick to ride through the stage's own slot. The stage the wait
/// runs in is the one the bar is creeping through, so this is the one
/// piece of a run that knows how long its middle is; a reply that
/// carries no reading is not a job's report and moves nothing.
struct JobReportingCaller<C> {
    /// The caller every call is forwarded to, unchanged.
    caller: C,
    /// Where a job's reading is stored, shared with the tick that reads
    /// it: a fraction of a thousand, zero meaning "nothing to ride".
    job: Arc<AtomicU32>,
}

impl<C: ToolCaller> ToolCaller for JobReportingCaller<C> {
    fn call(&mut self, call: &ToolCall) -> Result<serde_json::Value, ToolError> {
        let reply = self.caller.call(call)?;
        // The reading, as the wait itself spells it: a fraction, else the
        // frames against the whole. Anything else on the reply is not a
        // job's report.
        let reading = reply
            .get("fraction")
            .and_then(serde_json::Value::as_f64)
            .map(|fraction| fraction.clamp(0.0, 1.0))
            .or_else(|| {
                let frame = reply.get("frame").and_then(serde_json::Value::as_f64)?;
                let total = reply.get("total").and_then(serde_json::Value::as_f64)?;
                (total > 0.0).then(|| (frame / total).clamp(0.0, 1.0))
            });
        if let Some(reading) = reading {
            self.job
                .store((reading * 1000.0).round() as u32, Ordering::Relaxed);
        }
        Ok(reply)
    }
}

/// The AUTO EDIT's program: the deterministic spine of the watcher's
/// pipeline, written from paths the pane fixed before the run began -
/// which is what makes the program static, and the runner's the right
/// shape for it. The stages are the runner's own register, in the
/// watcher's order:
///
/// - the handoff the pane itself made, opened and read back;
/// - the repair, warn-policy: a take that cannot be repaired ships as
///   recorded, the watcher's own degradation, and the clean pass lands
///   on the take the timeline plays;
/// - the readings: the words, the delivery, the music gate;
/// - the sanity reads before any render;
/// - the save: the project saved and read back, the editable timeline
///   the user opens - and the export renders the run's own MP4 of the
///   same cut, waited to done;
/// - the verify: the saved timeline's lanes checked for clips that
///   share one, the timeline read back clean;
/// - the watch, warn-policy: the cut seen as a contact sheet and scored,
///   and a low score is a flaw recorded - the yellow toast - not a run
///   thrown away.
///
/// The creative layer - B-roll, captions, the grade - is the Director
/// agent's; those stages are simply not in the program, and the progress
/// fraction counts only the stages that are.
/// Runs the auto-edit on its own thread: the toolbox and the backend are
/// the adapter's ground, the runner drives them, and the pane hears every
/// move through the messages the threads post. A ticker beside the run
/// posts the clock and the stage when either moves; a drain thread reads
/// the runner's event stream into the pane a batch at a time, off the
/// window's thread; the run's own end posts the terminal state.
fn run_auto_edit(
    runner: Runner,
    // The runner reads and writes the program it runs - the fan lanes and
    // the derived steps land in it - so the run's own copy is mutable
    // through to the `run_with_lanes` call inside its thread.
    mut program: EditProgram,
    backend: Box<dyn concat_mcp::Backend>,
    tool_cancel: Arc<AtomicBool>,
    // The runner's event stream, the receiving end the pane kept beside
    // the runner: the drain thread below reads it, and a retryable end
    // keeps it with the runner for the resume - one channel per runner,
    // whichever run is draining it now.
    events: AutoEditEvents,
    // The waiting job's own reading of its progress, as a fraction of a
    // thousand: the caller the run drives reports into it when a reply
    // carries one, and the tick rides the bar through the stage's slot
    // on it - the one piece of a run that knows how long its middle is.
    job: Arc<AtomicU32>,
) {
    let still = Arc::new(AtomicBool::new(true));
    // The ticker: the pane's fields reread the runner's last event, so
    // the tick is only ever "look again" - a quarter-second eye on the
    // stage, a one-second eye on the clock.
    let tick_runner = runner.clone();
    let tick_still = Arc::clone(&still);
    std::thread::spawn(move || {
        let mut last = None;
        let mut since = Instant::now();
        while tick_still.load(Ordering::Relaxed) {
            let reading = tick_runner.progress();
            if reading != last || since.elapsed() >= Duration::from_secs(1) {
                last = reading;
                since = Instant::now();
                crate::host::on_ui(|studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::AutoEditTick));
                });
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
    // The drain: the runner's events, read off the window's thread and
    // posted as one batch per look - a fan's lanes answering at once are
    // one wakeup, not five. The lock is only ever held for the taking,
    // never across the sleep: the resume's own clearing of a prior run's
    // unread tail must never wait on this thread's nap.
    let drain_still = Arc::clone(&still);
    std::thread::spawn(move || {
        while drain_still.load(Ordering::Relaxed) {
            let mut batch = Vec::new();
            {
                let receiver = events
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                while let Ok(event) = receiver.try_recv() {
                    batch.push(event);
                }
            }
            if batch.is_empty() {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            crate::host::on_ui(move |studio, _, _| {
                studio.handle(Msg::Forge(ForgeMsg::AutoEditEvents(batch)));
            });
        }
    });
    std::thread::spawn(move || {
        // The same toolbox the protocol serves, so a call the runner makes
        // means exactly what the same call from an agent would. Built
        // under catch_unwind so a build-time inconsistency is a failed run
        // with a sentence, not a thread that dies quietly and leaves the
        // button saying "running" forever.
        let toolbox = match std::panic::catch_unwind(concat_mcp::tools::Toolbox::new) {
            Ok(toolbox) => std::sync::Arc::new(toolbox),
            Err(_) => {
                still.store(false, Ordering::Relaxed);
                crate::host::on_ui(|studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::AutoEditEnded(Box::new(
                        AutoEditState::Failed {
                            stage: Stage::Plan,
                            error: AutoEditError::new(
                                Stage::Plan,
                                None,
                                ToolError {
                                    // The sentence is the pane's own
                                    // wording, and it lands raw inside
                                    // `forge.autoEditFailed`'s {0}, so it
                                    // is a localized line - the lower
                                    // layers' own words, when there are
                                    // any, ride the toast as they come.
                                    message: t("forge.autoEditToolboxFailed"),
                                    retryable: Some(false),
                                    retry_after: None,
                                    code: None,
                                },
                            ),
                            attempt: 1,
                            // No receipt: the toolbox never built, so no
                            // call ever answered and there is no evidence
                            // a receipt could name.
                            receipt: None,
                        },
                    ))));
                });
                return;
            }
        };
        let mut backend = backend;
        let cancelled = tool_cancel;
        // The caller the runner drives, wrapped so a waiting job's own
        // reading of its progress reaches the bar: the export's frames, a
        // model's arrival - whatever a reply reports as a fraction,
        // stored as a fraction of a thousand for the tick to ride. The
        // call itself is forwarded unchanged; only the reading is taken.
        let mut caller = JobReportingCaller {
            caller: concat_mcp::director::BackendToolCaller::new(
                &toolbox,
                backend.as_mut(),
                &cancelled,
            ),
            job: Arc::clone(&job),
        };
        // The lanes a fan step runs its groups on: every lane its own
        // connection to the same server, over the one shared toolbox,
        // stopped by the one shared flag. A run without fan steps never
        // asks the factory for a lane.
        let lanes = concat_mcp::director::LaneOpener::new(
            concat_mcp::backend::Mode::Window,
            Arc::clone(&toolbox),
            Arc::clone(&cancelled),
        );
        // The run itself, under the same guard the toolbox was built
        // under: a panic in the runner's own frame - between the calls
        // its per-call guard already catches - must not leave the
        // button saying "running" forever. The runner's own RunGuard
        // clears its one-run latch on unwind; this guard is the pane's
        // half: the ticker stops and the end still posts.
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            runner.run_with_lanes(&mut program, &mut caller, &lanes)
        }));
        still.store(false, Ordering::Relaxed);
        let state = match made {
            Ok(_) => runner.state(),
            Err(panic) => {
                let what = if let Some(text) = panic.downcast_ref::<&str>() {
                    (*text).to_owned()
                } else if let Some(text) = panic.downcast_ref::<String>() {
                    text.clone()
                } else {
                    // The pane's own words for a payload that carried
                    // none - a sentence the toast shows, so a localized
                    // one.
                    t("forge.autoEditPanickedWithoutMessage")
                };
                // The state is built here, not read back from the
                // runner: a panic mid-emit can leave the runner's own
                // locks poisoned, and reading them would only panic
                // again. The stage is the run's own, the way the
                // toolbox guard names it.
                AutoEditState::Failed {
                    stage: Stage::Plan,
                    error: AutoEditError::new(
                        Stage::Plan,
                        None,
                        ToolError {
                            // The pane's own prefix around whatever the
                            // panic carried rides `forge.autoEditFailed`'s
                            // {0} raw, so it is a localized line and only
                            // the panicking code's own words ride as
                            // they came.
                            message: tf("forge.autoEditPanicked", &[&what]),
                            retryable: Some(false),
                            retry_after: None,
                            code: None,
                        },
                    ),
                    attempt: 1,
                    // No receipt: the state is built here rather than
                    // read back from the runner - the runner's own locks
                    // may be poisoned - so the partial receipt it held
                    // dies with the panic. `None` is the honest answer,
                    // not a silent half of one.
                    receipt: None,
                }
            }
        };
        crate::host::on_ui(move |studio, _, _| {
            studio.handle(Msg::Forge(ForgeMsg::AutoEditEnded(Box::new(state))));
        });
    });
}

/// The stage's own word, localized: the register is the runner's, the
/// word is the page's. Each call spells its key out loud, the way the
/// locale inventory reads them.
fn stage_label(stage: &Stage) -> String {
    match stage {
        Stage::Preflight => t("forge.stagePreflight"),
        Stage::HandoffVoiceover => t("forge.stageHandoffVoiceover"),
        Stage::Transcribe => t("forge.stageTranscribe"),
        Stage::AudioRepair => t("forge.stageAudioRepair"),
        Stage::VoiceSignals => t("forge.stageVoiceSignals"),
        Stage::Understand => t("forge.stageUnderstand"),
        Stage::Plan => t("forge.stagePlan"),
        Stage::Gather => t("forge.stageGather"),
        Stage::Motion => t("forge.stageMotion"),
        Stage::Timeline => t("forge.stageTimeline"),
        Stage::Captions => t("forge.stageCaptions"),
        Stage::Music => t("forge.stageMusic"),
        Stage::Sfx => t("forge.stageSfx"),
        Stage::Preview => t("forge.stagePreview"),
        Stage::TimelineReady => t("forge.stageTimelineReady"),
        Stage::Export => t("forge.stageExport"),
        Stage::Watch => t("forge.stageWatch"),
        Stage::Revise => t("forge.stageRevise"),
        Stage::Verify => t("forge.stageVerify"),
    }
}

/// How long a run has taken, as a person reads it: "42s", then "1m 04s".
fn elapsed_text(duration: Duration) -> String {
    let seconds = duration.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}

/// The bar's reading of one event: a stage's own beginning sits its slot
/// halfway - the same place the tick's reread puts a stage in progress -
/// and each of its calls beyond creeps further through the slot,
/// approaching the stage's end without ever claiming it: 100% of the
/// bar is the run's own accepted end's alone to show, and a stage that
/// merely looks busy must not promise the stage after it.
fn event_fraction(stage_index: usize, total_stages: usize, stage_events: usize) -> f32 {
    let total = total_stages.max(1) as f32;
    // The creep halves the distance that is left with each event: 0.5,
    // 0.75, 0.833... - a curve that always rises, never settles, and
    // never arrives.
    let through = 0.5 + 0.5 * (stage_events as f32 / (stage_events as f32 + 1.0));
    (stage_index as f32 + through) / total
}

/// One event's line in the log: the stage's own word, then what the
/// event says is happening inside it - the call the runner is making,
/// the lane it is making it on, the stage's own beginning. The stage's
/// word is the page's (localized); the detail is the runner's, exactly
/// as it was written.
fn auto_edit_event_line(progress: &AutoEditProgress) -> String {
    format!("{}: {}", stage_label(&progress.stage), progress.detail)
}

#[cfg(test)]
mod tests {
    use super::auto_edit_busy;
    use super::configured_value;
    use super::director_acceptance_fraction;
    use super::director_phase;
    use super::director_retry_available;
    use super::director_skill_listed;
    use super::other_run_refusal;
    use super::voiceover_copy_name;
    use super::{AutoEditSource, TakePaths};
    use std::path::Path;
    use std::time::Duration;

    #[test]
    fn blank_environment_values_do_not_claim_a_configured_vision_critic() {
        assert!(!configured_value(None));
        assert!(!configured_value(Some(String::new())));
        assert!(!configured_value(Some("  \t\n".to_owned())));
        assert!(configured_value(Some(
            "https://critic.example/v1".to_owned()
        )));
        assert!(configured_value(Some("api-key".to_owned())));
    }

    #[test]
    fn director_progress_reaches_one_hundred_percent_only_after_acceptance() {
        assert_eq!(director_acceptance_fraction(false), 0.98);
        assert_eq!(director_acceptance_fraction(true), 1.0);
    }

    #[test]
    fn director_retry_reuses_only_a_complete_sibling_handoff() {
        let root = std::env::temp_dir().join(format!(
            "funcut-director-retry-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let voiceover = root.join("voiceover");
        let export = root.join("export");
        std::fs::create_dir_all(&voiceover).unwrap();
        std::fs::create_dir_all(&export).unwrap();
        std::fs::write(root.join("concat.json"), b"{}").unwrap();
        std::fs::write(root.join("voiceover-mapping.json"), b"{}").unwrap();
        let take = voiceover.join("take.wav");
        std::fs::write(&take, b"voice").unwrap();
        let source = AutoEditSource {
            project: root.to_string_lossy().into_owned(),
            frame: None,
            takes: vec![TakePaths {
                take: take.to_string_lossy().into_owned(),
                enhanced: voiceover
                    .join("enhanced.wav")
                    .to_string_lossy()
                    .into_owned(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: export.join("auto-edit.mp4").to_string_lossy().into_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            director: None,
            script: None,
            // The licence-safe default, the word a sibling is born under:
            // the builder does not know the pressed switch.
            monetized: true,
        };
        assert!(director_retry_available(&source));

        std::fs::remove_file(root.join("voiceover-mapping.json")).unwrap();
        assert!(
            !director_retry_available(&source),
            "mapping is required for word acceptance"
        );
        std::fs::write(root.join("voiceover-mapping.json"), b"{}").unwrap();
        std::fs::remove_file(&take).unwrap();
        assert!(
            !director_retry_available(&source),
            "all source takes must still exist"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn acceptance_check_keeps_both_edit_entry_points_locked() {
        assert!(!auto_edit_busy(false, false));
        assert!(auto_edit_busy(true, false));
        assert!(auto_edit_busy(false, true));
        assert!(auto_edit_busy(true, true));
    }

    #[test]
    fn only_the_director_skill_satisfies_forge_preflight() {
        assert!(director_skill_listed(
            0,
            "funcut-director\nmaestro\nclip-skills"
        ));
        assert!(!director_skill_listed(0, "maestro\nclip-skills"));
        assert!(!director_skill_listed(1, "funcut-director"));
    }

    /// The handed-over run's progress is read from the harness's own
    /// words, and only where a phase is actually named: a line about
    /// nothing in particular moves no bar, the ladder is the one the
    /// skill really prints (1 through 25 - tested
    /// against the skill's own heading strings, not a paraphrase of
    /// them), and no phase - not the last - ever shows a finished bar,
    /// which only the accepted end of a run may do.
    #[test]
    fn the_director_stage_is_read_from_the_harness_words() {
        assert_eq!(director_phase("just thinking"), None);
        assert_eq!(director_phase(""), None);

        let (first, at_first) =
            director_phase("Phase 1 — Creative direction (do this FIRST)").expect("phase one");
        assert_eq!(first, 0);
        assert!(at_first > 0.0 && at_first < 0.2);

        // Every heading the skill prints maps to its own rung, 1 through 25.
        for (number, index) in (1usize..=25).map(|number| (number, number - 1)) {
            let heading = format!("Phase {number} — the skill's own heading");
            assert_eq!(
                director_phase(&heading).map(|(at, _)| at),
                Some(index),
                "{heading}"
            );
        }

        // A greedy digit read must not turn "phase 15" into "phase 1",
        // and a number the ladder does not carry matches nothing.
        let (ten, _) = director_phase("Phase 10 — Storyboard and shot list").expect("phase ten");
        assert_eq!(ten, 9);
        let (twenty, at_twenty) =
            director_phase("Phase 25 — Delivery, customization and the wow bar")
                .expect("phase twenty-five");
        assert_eq!(twenty, 24);
        assert!(at_twenty < 1.0 && at_twenty > 0.8);
        assert_eq!(director_phase("Phase 99 — not on the ladder"), None);

        // The watch phase is where the Director reviews its own cut: the
        // tool name alone is enough to place it.
        let (review, _) = director_phase("craft_critique the sheet").expect("review");
        assert_eq!(review, 20);

        // A named phase advances; the one before it is behind it.
        let (place, early) =
            director_phase("Phase 15 — Place (sequential, batched)").expect("phase fifteen");
        assert_eq!(place, 14);
        assert!(early > at_first);

        // 16 is motion design, its own rung after Place.
        let (motion, _) = director_phase("Phase 16 — Motion design").expect("phase sixteen");
        assert_eq!(motion, 15);

        // The grade phase the skill requires is visible to the bar too.
        let (grade, _) = director_phase("Phase 19 — Grade (required)").expect("phase nineteen");
        assert_eq!(grade, 18);

        // The final phase never completes the bar: 1.0 is the accepted
        // end's alone.
        let (save, at_save) =
            director_phase("Phase 23 — Save and hand off the timeline (no export).")
                .expect("phase twenty-three");
        assert_eq!(save, 22);
        assert!(at_save < 1.0 && at_save > 0.8);
    }

    /// One run at a time on the shared project: the Start is refused while
    /// the auto edit goes, the auto edit is refused while the Start goes,
    /// and each refusal names the run to stop. Two writers on one sibling
    /// interleave their edits, so the guard is the page's contract.
    #[test]
    fn the_two_runs_refuse_to_overlap_and_name_the_one_to_stop() {
        assert!(
            other_run_refusal(false, true).is_none(),
            "Start alone may go"
        );
        assert!(
            other_run_refusal(false, false).is_none(),
            "auto edit alone may go"
        );
        let start_refused = other_run_refusal(true, true).expect("Start is refused");
        let auto_refused = other_run_refusal(true, false).expect("auto edit is refused");
        assert_eq!(
            start_refused,
            crate::i18n::t("forge.autoEditBusy"),
            "Start while the auto edit runs names the auto edit"
        );
        assert_eq!(
            auto_refused,
            crate::i18n::t("forge.startBusy"),
            "auto edit while the Start runs names the agent run"
        );
        assert_ne!(
            start_refused, auto_refused,
            "each refusal names its own run, not one shared sentence"
        );
    }

    #[test]
    fn voiceover_copies_with_the_same_basename_keep_distinct_names() {
        let first = voiceover_copy_name(0, Path::new("/first/take.wav"));
        let second = voiceover_copy_name(1, Path::new("/second/take.wav"));
        assert_ne!(first, second);
        assert!(first.ends_with("take.wav"));
        assert!(second.ends_with("take.wav"));
    }

    #[test]
    fn voiceover_copy_name_falls_back_for_a_path_without_a_filename() {
        assert_eq!(
            voiceover_copy_name(2, Path::new("/")),
            "voiceover-002-voiceover.wav"
        );
    }

    /// The handoff's contract (PR: fix/voiceover-handoff-preservation): a
    /// two-segment voiceover with a 1.5 s gap and one clip at 1.2x must
    /// round-trip into the sibling project with the same starts, the same
    /// gap, and the same speed - reconstructed from the per-clip mapping,
    /// never from a sort of the distinct files.
    #[test]
    fn a_two_segment_voiceover_round_trips_with_its_gap_and_speed() {
        use concat_project::model::{Clip, ClipKind, MediaItem, MediaKind, Project, VideoSettings};

        // A source project: two temp voiceover takes on one lane.
        let dir = std::env::temp_dir().join(format!("funcut-voiceover-{}", std::process::id()));
        let source_root = dir.join("Narration");
        let takes = dir.join("takes");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir_all(&takes).unwrap();
        let take_one = takes.join("take-1.wav");
        let take_two = takes.join("take-2.wav");
        std::fs::write(&take_one, b"take one").unwrap();
        std::fs::write(&take_two, b"take two").unwrap();

        let mut source = Project::with_video(VideoSettings {
            width: 1920,
            height: 1080,
            rate_num: 30,
            rate_den: 1,
            ..Default::default()
        });
        let media = |id: &str, path: &std::path::Path| MediaItem {
            id: id.to_owned(),
            path: path.to_string_lossy().into_owned(),
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("voiceover.wav")
                .to_owned(),
            duration: Some(6.0),
            kind: MediaKind::Audio,
            has_audio: true,
            ..Default::default()
        };
        source.media.push(media("m1", &take_one));
        source.media.push(media("m2", &take_two));
        let clip =
            |id: &str, media: &str, start: f64, duration: f64, source_start: f64, speed: f64| {
                let mut clip = Clip::blank(id, "T1", ClipKind::Audio, id, start, duration);
                clip.media_id = media.to_owned();
                clip.source_start = source_start;
                clip.speed = speed;
                clip.volume = 0.9;
                clip.fade_in = 0.05;
                clip.fade_out = 0.1;
                clip
            };
        // Segment one: 0.0 to 2.0. Segment two: starts at 3.5 - a 1.5 s
        // gap - trimmed 0.5 s into its file and played at 1.2x.
        {
            let timeline = source.active_mut();
            timeline
                .clips
                .push(std::sync::Arc::new(clip("c1", "m1", 0.0, 2.0, 0.0, 1.0)));
            timeline
                .clips
                .push(std::sync::Arc::new(clip("c2", "m2", 3.5, 2.4, 0.5, 1.2)));
        }

        // The handoff.
        let (built, auto) =
            super::build_voiceover_sibling(&source, &source_root, false, None).unwrap();
        assert_eq!(built.segments.len(), 2);
        // The forge run's handoff carries no AUTO EDIT paths.
        assert!(auto.is_none());

        // The sibling reopens as a real project, and its voiceover clips
        // carry the source's timing: same starts, same gap, same speed.
        let document =
            std::fs::read_to_string(std::path::Path::new(&built.project).join("concat.json"))
                .unwrap();
        let sibling = concat_project::doc::from_document(
            &serde_json::from_str::<serde_json::Value>(&document).unwrap(),
        )
        .unwrap();
        let timeline = sibling.active();
        let first = timeline
            .clips
            .iter()
            .find(|clip| clip.id == built.segments[0].sibling_clip_id)
            .unwrap();
        let second = timeline
            .clips
            .iter()
            .find(|clip| clip.id == built.segments[1].sibling_clip_id)
            .unwrap();
        assert_eq!(first.start, 0.0);
        assert_eq!(second.start, 3.5);
        assert_eq!(second.start - (first.start + first.duration), 1.5);
        assert_eq!(first.speed, 1.0);
        assert_eq!(second.speed, 1.2);
        assert_eq!(second.source_start, 0.5);
        assert_eq!(second.duration, 2.4);
        assert_eq!(second.volume, 0.9);
        assert_eq!(second.fade_in, 0.05);
        assert_eq!(second.fade_out, 0.1);
        // Both on one dedicated lane at the bottom of the stack, playing
        // the copies inside the sibling rather than the source's files.
        assert_eq!(first.track_id, second.track_id);
        assert_eq!(timeline.tracks.first().unwrap().id, first.track_id);
        let played = sibling.media_by_id(&second.media_id).unwrap();
        assert!(played.path.contains("voiceover"));
        assert!(Path::new(&played.path).exists());
        assert_ne!(played.path, take_two.to_string_lossy().into_owned());

        // The mapping file agrees with the timeline it wrote.
        let mapping = std::fs::read_to_string(
            std::path::Path::new(&built.project).join("voiceover-mapping.json"),
        )
        .unwrap();
        let mapping: serde_json::Value = serde_json::from_str(&mapping).unwrap();
        assert_eq!(mapping["schema"], "funcut.voiceover-mapping/1");
        // The version stands beside the schema string as its own number,
        // so a consumer branches on 1 without parsing the name.
        assert_eq!(mapping["schemaVersion"], 1);
        let segments = mapping["segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1]["source_clip_id"], "c2");
        assert_eq!(segments[1]["sibling_clip_id"], second.id);
        assert_eq!(segments[1]["source_in"], 0.5);
        assert_eq!(segments[1]["source_out"], 0.5 + 2.4 * 1.2);
        assert_eq!(segments[1]["track"], second.track_id);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The AUTO EDIT's own sibling: the takes carry the spelling the
    /// repair chain lands on, the export folder is made, and the fixed
    /// paths all sit inside the sibling.
    #[test]
    fn an_auto_edit_sibling_names_its_takes_and_makes_its_export_folder() {
        use concat_project::model::{Clip, ClipKind, MediaItem, MediaKind, Project, VideoSettings};

        let dir = std::env::temp_dir().join(format!("funcut-auto-edit-{}", std::process::id()));
        let source_root = dir.join("Narration");
        let takes = dir.join("takes");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir_all(&takes).unwrap();
        let take = takes.join("take-1.wav");
        std::fs::write(&take, b"take one").unwrap();

        let mut source = Project::with_video(VideoSettings {
            width: 1080,
            height: 1920,
            rate_num: 30,
            rate_den: 1,
            ..Default::default()
        });
        source.media.push(MediaItem {
            id: "m1".to_owned(),
            path: take.to_string_lossy().into_owned(),
            name: "take-1.wav".to_owned(),
            duration: Some(6.0),
            kind: MediaKind::Audio,
            has_audio: true,
            ..Default::default()
        });
        let mut clip = Clip::blank("c1", "T1", ClipKind::Audio, "c1", 0.0, 6.0);
        clip.media_id = "m1".to_owned();
        source.active_mut().clips.push(std::sync::Arc::new(clip));

        let (_, auto) = super::build_voiceover_sibling(&source, &source_root, true, None).unwrap();
        let auto = auto.expect("the AUTO EDIT's paths");
        assert_eq!(auto.takes.len(), 1);
        let row = &auto.takes[0];
        // The take is inside the sibling's voiceover folder, under the
        // take- spelling, and the enhanced stepping stone sits beside it.
        assert!(row.take.contains("voiceover"));
        assert!(Path::new(&row.take).is_file(), "the pristine copy exists");
        assert!(row.take.contains("take-000"));
        assert!(row.enhanced.contains("enhanced-000"));
        assert!(
            row.enhanced
                .starts_with(Path::new(&row.take).parent().unwrap().to_str().unwrap())
        );
        // The export folder is made, and both fixed outputs live in it.
        assert!(Path::new(&auto.project).join("export").is_dir());
        assert!(auto.export.ends_with("export/auto-edit.mp4"));
        assert!(auto.export.starts_with(&auto.project));
        assert_eq!(auto.sheet, "auto-edit-sheet.png");
        // The licence word is the licence-safe default until the press
        // that knows the switch overwrites it: a monetized edit is the
        // one that may not spend the non-commercial beds, so the default
        // never reaches for material it may not use.
        assert!(auto.monetized);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The orientation pick births the sibling in the picked frame: the
    /// AUTO EDIT's canvas turns while its rate and everything else stay
    /// the source project's, and no pick - or a word the table does not
    /// know - keeps the source's own canvas, exactly as the handoff
    /// always built it.
    #[test]
    fn an_orientation_pick_births_the_sibling_in_the_picked_frame() {
        use concat_project::model::{Clip, ClipKind, MediaItem, MediaKind, Project, VideoSettings};

        let dir = std::env::temp_dir().join(format!(
            "funcut-orientation-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let source_root = dir.join("Narration");
        let takes = dir.join("takes");
        std::fs::create_dir_all(&source_root).unwrap();
        std::fs::create_dir_all(&takes).unwrap();
        let take = takes.join("take-1.wav");
        std::fs::write(&take, b"take one").unwrap();

        let mut source = Project::with_video(VideoSettings {
            width: 1920,
            height: 1080,
            rate_num: 30000,
            rate_den: 1001,
            ..Default::default()
        });
        source.media.push(MediaItem {
            id: "m1".to_owned(),
            path: take.to_string_lossy().into_owned(),
            name: "take-1.wav".to_owned(),
            duration: Some(6.0),
            kind: MediaKind::Audio,
            has_audio: true,
            ..Default::default()
        });
        let mut clip = Clip::blank("c1", "T1", ClipKind::Audio, "c1", 0.0, 6.0);
        clip.media_id = "m1".to_owned();
        source.active_mut().clips.push(std::sync::Arc::new(clip));

        let frame_of = |orientation: Option<&str>| {
            let (_, auto) =
                super::build_voiceover_sibling(&source, &source_root, true, orientation).unwrap();
            let auto = auto.expect("the AUTO EDIT's paths");
            let document =
                std::fs::read_to_string(std::path::Path::new(&auto.project).join("concat.json"))
                    .unwrap();
            let document: serde_json::Value = serde_json::from_str(&document).unwrap();
            (
                document["video"]["width"].as_u64().unwrap(),
                document["video"]["height"].as_u64().unwrap(),
                document["video"]["rateNum"].as_i64().unwrap(),
                document["video"]["rateDen"].as_i64().unwrap(),
            )
        };
        // A vertical pick turns the canvas and keeps the NTSC rate.
        assert_eq!(frame_of(Some("vertical")), (1080, 1920, 30000, 1001));
        // The pick keeps the project's own size tier: a 4K source picked
        // vertical is born a 4K reel, not a downscaled 1080 one - the
        // pick names the shape, never the price.
        let mut four_k = Project::with_video(VideoSettings {
            width: 3840,
            height: 2160,
            rate_num: 30000,
            rate_den: 1001,
            ..Default::default()
        });
        four_k.media = source.media.clone();
        let mut clip = Clip::blank("c1", "T1", ClipKind::Audio, "c1", 0.0, 6.0);
        clip.media_id = "m1".to_owned();
        four_k.active_mut().clips.push(std::sync::Arc::new(clip));
        let (_, auto) =
            super::build_voiceover_sibling(&four_k, &source_root, true, Some("vertical")).unwrap();
        let auto = auto.expect("the AUTO EDIT's paths");
        let document =
            std::fs::read_to_string(std::path::Path::new(&auto.project).join("concat.json"))
                .unwrap();
        let document: serde_json::Value = serde_json::from_str(&document).unwrap();
        assert_eq!(document["video"]["width"].as_u64().unwrap(), 2160);
        assert_eq!(document["video"]["height"].as_u64().unwrap(), 3840);
        // No pick - and a word the table does not know - keeps the
        // source's own canvas.
        assert_eq!(frame_of(None), (1920, 1080, 30000, 1001));
        assert_eq!(frame_of(Some("widescreen")), (1920, 1080, 30000, 1001));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The button's own gate: NARRATION on the timeline is what AUTO EDIT
    /// stands on, and nothing else is - not a video clip, and not the
    /// music bed either, which is audio but lives in `<project>/bgm/`
    /// (F-01: a BGM-only project must not arm the button).
    #[test]
    fn the_auto_edit_button_needs_voiceover_clips() {
        use concat_project::model::{Clip, ClipKind, MediaItem, MediaKind, Project, VideoSettings};

        let mut with_voice = Project::with_video(VideoSettings::default());
        with_voice.media.push(MediaItem {
            id: "m1".to_owned(),
            path: "/takes/a.wav".to_owned(),
            name: "a.wav".to_owned(),
            duration: Some(2.0),
            kind: MediaKind::Audio,
            has_audio: true,
            ..Default::default()
        });
        let mut clip = Clip::blank("c1", "T1", ClipKind::Audio, "c1", 0.0, 2.0);
        clip.media_id = "m1".to_owned();
        with_voice
            .active_mut()
            .clips
            .push(std::sync::Arc::new(clip));

        let mut without = Project::with_video(VideoSettings::default());
        let mut video_clip = Clip::blank("v1", "T1", ClipKind::Video, "v1", 0.0, 2.0);
        video_clip.media_id = "m1".to_owned();
        without
            .active_mut()
            .clips
            .push(std::sync::Arc::new(video_clip));

        // The bed: bgm_import's own folder under the project. A timeline
        // that carries nothing else is a music edit, not a narration one.
        let root = std::env::temp_dir().join(format!(
            "concat-bed-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bgm")).unwrap();
        std::fs::create_dir_all(root.join("takes")).unwrap();
        std::fs::write(root.join("takes/a.wav"), b"take").unwrap();
        std::fs::write(root.join("bgm/lofi-track.mp3"), b"music").unwrap();
        let take_path = root.join("takes/a.wav").to_string_lossy().into_owned();
        let bed_path = root
            .join("bgm/lofi-track.mp3")
            .to_string_lossy()
            .into_owned();

        let mut bed_only = Project::with_video(VideoSettings::default());
        bed_only.media.push(MediaItem {
            id: "b1".to_owned(),
            path: bed_path.clone(),
            name: "lofi-track.mp3".to_owned(),
            duration: Some(90.0),
            kind: MediaKind::Audio,
            has_audio: true,
            ..Default::default()
        });
        let mut bed_clip = Clip::blank("g1", "T1", ClipKind::Audio, "g1", 0.0, 90.0);
        bed_clip.media_id = "b1".to_owned();
        bed_only
            .active_mut()
            .clips
            .push(std::sync::Arc::new(bed_clip));

        // The same bed beside a real take: the take is narration, the bed
        // stays out of the voiceover lane.
        let mut mixed = with_voice.clone();
        mixed.media[0].path = take_path;
        mixed.media.push(bed_only.media[0].clone());
        let mut bed_clip = Clip::blank("g1", "T1", ClipKind::Audio, "g1", 0.0, 90.0);
        bed_clip.media_id = "b1".to_owned();
        mixed.active_mut().clips.push(std::sync::Arc::new(bed_clip));

        let narrated = |project: &concat_project::model::Project| {
            project
                .active()
                .clips
                .iter()
                .any(|clip| super::is_narration(clip, project, &root))
        };
        assert!(narrated(&with_voice));
        assert!(!narrated(&without));
        assert!(
            !narrated(&bed_only),
            "a BGM-only project does not arm AUTO EDIT"
        );
        assert!(narrated(&mixed), "the bed does not unarm a real take");
        // And the sibling build takes the same side: the bed is not a
        // segment of the voiceover.
        let (built, _) = super::build_voiceover_sibling(&mixed, &root, false, None).unwrap();
        assert_eq!(built.segments.len(), 1, "only the take is the voiceover");
        // A project whose only audio is the bed refuses the handoff with
        // the no-voiceover notice, not a sibling of its music.
        assert!(super::build_voiceover_sibling(&bed_only, &root, false, None).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The clock, as the page spells it.
    #[test]
    fn the_elapsed_line_reads_as_a_person_does() {
        assert_eq!(super::elapsed_text(Duration::from_secs(0)), "0s");
        assert_eq!(super::elapsed_text(Duration::from_secs(42)), "42s");
        assert_eq!(super::elapsed_text(Duration::from_secs(64)), "1m 04s");
        assert_eq!(super::elapsed_text(Duration::from_secs(601)), "10m 01s");
    }

    /// The event stream's reading of the bar: a stage's own beginning
    /// sits its slot halfway - the same place the tick's reread puts a
    /// stage in progress - each call beyond creeps further through the
    /// slot, and no number of calls ever claims the stage's end: 100% of
    /// the bar is the run's own accepted end's alone to show.
    #[test]
    fn the_events_creep_the_bar_through_a_stage_without_claiming_its_end() {
        // The stage's own beginning, halfway through the first slot.
        assert_eq!(super::event_fraction(0, 4, 0), 0.125);
        // Each call beyond the beginning creeps further through.
        assert!(super::event_fraction(0, 4, 1) > super::event_fraction(0, 4, 0));
        assert!(super::event_fraction(0, 4, 2) > super::event_fraction(0, 4, 1));
        // However long a stage runs, its slot's end stays unclaimed.
        for events in 0..100 {
            assert!(super::event_fraction(2, 4, events) < 0.75);
            assert!(super::event_fraction(2, 4, events) >= 0.625);
        }
        // The last stage is the same: a busy final stage is not a
        // finished run.
        for events in 0..100 {
            assert!(super::event_fraction(3, 4, events) < 1.0);
        }
        // A later stage's beginning is beyond an earlier stage's creep,
        // so the bar only ever advances on evidence.
        assert!(super::event_fraction(1, 4, 0) > super::event_fraction(0, 4, 99));
        // A program of no stages is not divided by zero.
        assert!(super::event_fraction(0, 0, 3) > 0.0);
    }

    /// A waiting job's own reading of its progress rides out of the
    /// reply the wait drew - the export's frames, a model's arrival -
    /// and nothing else does: a reply that carries no reading is not a
    /// job's report, and the bar does not move on it.
    #[test]
    fn a_waiting_jobs_reading_rides_out_of_the_reply_that_carries_it() {
        use concat_mcp::director::{ToolCall, ToolCaller as _};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        // The caller under the test's own: whatever reply the test armed
        // it with, verbatim.
        struct Says(serde_json::Value);
        impl super::ToolCaller for Says {
            fn call(&mut self, _call: &ToolCall) -> Result<serde_json::Value, super::ToolError> {
                Ok(self.0.clone())
            }
        }
        let asked = |reply: serde_json::Value| {
            let job = Arc::new(AtomicU32::new(0));
            let mut caller = super::JobReportingCaller {
                caller: Says(reply),
                job: Arc::clone(&job),
            };
            caller
                .call(&ToolCall {
                    tool: "analysis_wait".to_owned(),
                    arguments: serde_json::json!({ "path": "take.wav" }),
                })
                .expect("the armed reply is the answer");
            job.load(Ordering::Relaxed)
        };
        // A fraction, as the wait itself spells it.
        assert_eq!(
            asked(serde_json::json!({ "state": "running", "fraction": 0.25 })),
            250
        );
        // The frames against the whole, when the fraction is absent.
        assert_eq!(
            asked(serde_json::json!({ "state": "running", "frame": 60, "total": 240 })),
            250
        );
        // A settled job reports whole - the stage's slot is its own to
        // claim, the run's accepted end still the only thing that
        // finishes the bar.
        assert_eq!(
            asked(serde_json::json!({ "state": "done", "fraction": 1.0 })),
            1000
        );
        // A reading past the whole is clamped to it.
        assert_eq!(
            asked(serde_json::json!({ "state": "running", "fraction": 2.0 })),
            1000
        );
        // A reply that carries no reading moves nothing.
        assert_eq!(asked(serde_json::json!({ "ok": true })), 0);
        // A fraction that is not a number is not a reading.
        assert_eq!(asked(serde_json::json!({ "fraction": "soon" })), 0);
        // A total of zero is not a promise of instant completion.
        assert_eq!(asked(serde_json::json!({ "frame": 0, "total": 0 })), 0);
    }
}
