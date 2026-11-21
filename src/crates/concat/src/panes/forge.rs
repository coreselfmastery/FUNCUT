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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use concat_mcp::director::{
    AutoEditError, AutoEditState, EditProgram, Runner, Stage, ToolError, auto_edit_program,
};
use slint::SharedString;

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
    /// cancellation with rollback and the retries.
    AutoEditStart,
    /// AUTO EDIT: stop at the next stage boundary; the edits the run
    /// applied are rolled back, one `edit_undo` each.
    AutoEditCancel,
    /// The auto-edit's clock moved, or its stage did: reread the
    /// runner's last event into the page's fields.
    AutoEditTick,
    /// The auto-edit ended. The state says which of the four ends the
    /// run met.
    AutoEditEnded(Box<AutoEditState>),
    /// The Director run AUTO EDIT hands the edit to has ended, by its
    /// exit code.
    AutoDirectorEnded(i32),
    /// The check the pane runs on a finished Director run came back: the
    /// receipt is the verdict, and the exit code is not.
    DirectorChecked(Box<concat_mcp::acceptance::DirectorAcceptance>),
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
}

/// The AUTO EDIT's standing on the page: the fields the progress pane
/// is filled from, and the handles that reach the run.
#[derive(Default)]
pub struct AutoEditPane {
    /// A run is going.
    pub running: bool,
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
                if on && self.connected.is_none() {
                    self.probe();
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
                // the fraction of its twelve phases they sit at. A line
                // that names no phase moves nothing, so the bar only ever
                // advances on evidence.
                if self.auto.running
                    && self.auto.runner.is_none()
                    && let Some((phase, fraction)) = director_phase(&line)
                {
                    self.auto.stage = director_phase_stage(phase);
                    self.auto.fraction = fraction;
                }
                self.log.push(line.into());
                let extra = self.log.len().saturating_sub(LOG_KEPT);
                self.log.drain(0..extra);
            }
            ForgeMsg::Ended(code) => {
                self.running = false;
                if self.kill.load(Ordering::Relaxed) {
                    self.set_status(t("forge.stopped"));
                } else if code == 0 {
                    self.set_status(t("forge.done"));
                    // A voiceover run's whole point is the edit landing on
                    // the timeline the user is looking at: the run edited
                    // a sibling project (the window holds this one, and an
                    // agent cannot open what the window holds), so the
                    // finished edit opens here, in the window, now. The
                    // voiceover project itself is untouched on disk.
                    if let Some(path) = self.result_path.take() {
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
                }
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
            }
            ForgeMsg::Connected(Ok(false)) => {
                self.connected = Some(false);
                self.set_status(t("forge.notConnected"));
            }
            ForgeMsg::Connected(Err(error)) => {
                self.connected = Some(false);
                self.set_status(t("forge.notConnected"));
                studio.notify(&tf("forge.connectFailed", &[&error]), true);
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
            }
            ForgeMsg::Installed(Ok(false)) => {
                self.set_status(t("forge.installFailed"));
            }
            ForgeMsg::Installed(Err(error)) => {
                self.set_status(t("forge.installFailed"));
                studio.notify(&tf("forge.installFailedDetail", &[&error]), true);
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
            ForgeMsg::AutoEditStart => self.auto_edit_start(studio),
            ForgeMsg::AutoEditCancel => {
                if self.auto.running {
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
                    self.set_status(t("forge.autoEditStopping"));
                }
            }
            ForgeMsg::AutoEditTick => self.auto_edit_tick(),
            ForgeMsg::AutoEditEnded(state) => self.auto_edit_ended(*state, studio),
            ForgeMsg::AutoDirectorEnded(code) => self.auto_director_ended(code, studio),
            ForgeMsg::DirectorChecked(checked) => self.auto_director_checked(*checked, studio),
        }
    }

    /// A quiet look at the setup, on the first time the page is shown:
    /// is the harness wired, is the skill where it reads.
    fn probe(&mut self) {
        self.set_status(t("forge.probing"));
        crate::host::spawn(probe_setup, |studio, _, _, (connected, installed)| {
            studio.handle(Msg::Forge(ForgeMsg::Connected(connected)));
            studio.handle(Msg::Forge(ForgeMsg::Installed(installed)));
        });
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
        if let Some(refusal) = other_run_refusal(self.auto.running, true) {
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
        let prompt = director_run_prompt(&script, voiceover.as_ref(), voice, niche, mood, studio);
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
        project
            .active()
            .clips
            .iter()
            .any(|clip| is_narration(clip, project, root))
    }

    /// Starts an AUTO EDIT: the sibling is built exactly as a forge run's
    /// is - the same handoff, the same voiceover lane reconstruction -
    /// and then the deterministic spine of the pipeline is run by the
    /// concat-director runner over the window's own server: the repair,
    /// readings, tighten, grade, licensed live-stock B-roll, captions,
    /// title entrances, music, preview, revise, export, verify and watch.
    /// The plan's creative brief, the eye-picked stock and the baked motion
    /// comps remain the Director agent's; this is the unattended voiceover
    /// edit.
    fn auto_edit_start(&mut self, studio: &mut Studio) {
        if self.auto.running {
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
        // A retryable end kept its sibling, its source and its runner:
        // the resume picks up after the stages the failed run completed
        // (IDEM), instead of building a fresh sibling and paying for the
        // repair, the transcription and every reading again.
        if let (Some(runner), Some(source)) = (self.auto.runner.clone(), self.auto.source.clone())
            && source
                .takes
                .iter()
                .all(|take| std::path::Path::new(&take.take).exists())
            && std::path::Path::new(&source.project)
                .join("concat.json")
                .exists()
        {
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
            });
            let tool_cancel = Arc::new(AtomicBool::new(false));
            let project = source.project.clone();
            self.auto = AutoEditPane {
                running: true,
                stage: t("forge.autoEditStarting"),
                elapsed: elapsed_text(Duration::ZERO),
                fraction: 0.0,
                visible: true,
                outcome: 0,
                runner: Some(runner.clone()),
                tool_cancel: Some(Arc::clone(&tool_cancel)),
                kill: None,
                started: Some(Instant::now()),
                project: Some(project),
                source: Some(source),
            };
            self.set_status(t("forge.autoEditRunning"));
            run_auto_edit(runner, program, backend, tool_cancel);
            return;
        }
        let Some(session) = studio.session.as_ref() else {
            studio.notify(&t("forge.needProject"), true);
            return;
        };
        let built =
            build_voiceover_sibling(studio.project(), std::path::Path::new(session.path()), true);
        let (_, auto) = match built {
            Ok(built) => built,
            Err(error) => {
                studio.notify(&error, true);
                return;
            }
        };
        let Some(auto) = auto else {
            return;
        };
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
        });
        // The Director gets the edit whenever the page has a model to run
        // it: AUTO EDIT is the one-click form of Start Forge, so the
        // creative decisions belong to the same agent, over the same
        // sibling. Only when the harness, its wiring, the skill or a key
        // is missing does the run fall back to the fixed spine - and the
        // pane says which one is missing, so a lesser edit is never
        // mistaken for the Director's.
        let handed_over = match director_ready(studio) {
            Ok(()) => {
                let kill = Arc::new(AtomicBool::new(false));
                self.auto = AutoEditPane {
                    running: true,
                    stage: t("forge.autoEditDirectorStage"),
                    elapsed: elapsed_text(Duration::ZERO),
                    fraction: 0.0,
                    visible: true,
                    outcome: 0,
                    runner: None,
                    tool_cancel: None,
                    started: Some(Instant::now()),
                    project: Some(auto.project.clone()),
                    source: Some(auto.clone()),
                    kill: Some(Arc::clone(&kill)),
                };
                self.set_status(t("forge.autoEditDirectorStage"));
                launch_auto_director(studio, &auto, kill);
                true
            }
            Err(why) => {
                studio.notify(
                    &format!(
                        "the Director is not available, so this run uses the fixed edit: {why}"
                    ),
                    true,
                );
                false
            }
        };
        if handed_over {
            return;
        }
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
        let (runner, _events) = Runner::new(PathBuf::from(&auto.project));
        let tool_cancel = Arc::new(AtomicBool::new(false));
        self.auto = AutoEditPane {
            running: true,
            stage: t("forge.autoEditStarting"),
            elapsed: elapsed_text(Duration::ZERO),
            fraction: 0.0,
            visible: true,
            outcome: 0,
            runner: Some(runner.clone()),
            tool_cancel: Some(Arc::clone(&tool_cancel)),
            started: Some(Instant::now()),
            project: Some(auto.project.clone()),
            source: Some(auto),
            kill: None,
        };
        self.set_status(t("forge.autoEditRunning"));
        run_auto_edit(runner, program, backend, tool_cancel);
    }

    /// The Director run's end. A zero exit is the harness saying the
    /// agent finished; the edit it made is opened here in the window, the
    /// same landing a Start Forge run has, and the pane reports the run
    /// rather than a measured export - the Director's own craft_critique
    /// and its final watch_render are what judged the file, and the pane
    /// does not invent a score of its own.
    fn auto_director_ended(&mut self, code: i32, studio: &mut Studio) {
        self.auto.running = false;
        self.auto.started = None;
        self.auto.kill = None;
        self.auto.fraction = 1.0;
        if code == 0 {
            // The harness exiting zero says the agent stopped, not that the
            // edit exists. So the run is held to the same four questions the
            // fixed spine asks of its own export - is the file readable, is
            // the mix on target, what does the critic score it, does it say
            // the words the edit placed - and only a receipt the gates
            // accept is reported as a finished edit. The check runs on a
            // worker: it probes and measures a real file.
            self.auto.stage = t("forge.autoEditCheckingStage");
            self.set_status(t("forge.autoEditCheckingStage"));
            let (Some(project), Some(export)) = (
                self.auto.project.clone(),
                self.auto.source.as_ref().map(|s| s.export.clone()),
            ) else {
                self.auto.outcome = 2;
                self.auto.stage = t("forge.autoEditDirectorFailedStage");
                self.set_status(t("forge.autoEditDirectorFailedStage"));
                studio.notify(
                    "the Director finished but the run has no project and export to check",
                    true,
                );
                return;
            };
            crate::host::spawn_detached(move || {
                let checked = match concat_mcp::backend::open(concat_mcp::backend::Mode::Window) {
                    Ok(mut backend) => {
                        let toolbox = concat_mcp::tools::Toolbox::new();
                        let cancelled = AtomicBool::new(false);
                        let check = concat_mcp::acceptance::DirectorCheck {
                            project: &project,
                            export: &export,
                        };
                        concat_mcp::acceptance::check(&check, &toolbox, &mut *backend, &cancelled)
                    }
                    // The window's server is how the check reaches the
                    // tools; without it there is no measurement, and an
                    // unmeasured run is not a finished one.
                    Err(error) => {
                        crate::host::on_ui(move |studio, _, _| {
                            studio.notify(&format!("the edit cannot be checked: {error}"), true);
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
        self.auto.outcome = 1;
        self.auto.stage = t("forge.autoEditDirectorFailedStage");
        self.set_status(t("forge.autoEditDirectorFailedStage"));
        studio.notify(
            &tf("forge.autoEditDirectorFailed", &[&code.to_string()]),
            true,
        );
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
        let accepted = checked.accepted();
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
        self.log.push(format!("acceptance: {summary}").into());
        let extra = self.log.len().saturating_sub(LOG_KEPT);
        self.log.drain(0..extra);
        if accepted {
            self.auto.outcome = 0;
            self.auto.stage = t("forge.autoEditDirectorDoneStage");
            self.set_status(t("forge.autoEditDirectorDoneStage"));
            if let Some(path) = self.auto.project.clone() {
                studio.notify(&tf("forge.autoEditDirectorDone", &[&path]), false);
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
            let total = progress.total_stages.max(1) as f32;
            self.auto.fraction = (progress.stage_index as f32 + 1.0) / total;
        }
        if let Some(started) = self.auto.started {
            self.auto.elapsed = elapsed_text(started.elapsed());
        }
    }

    /// The run's terminal state: the toast it earned, the button it
    /// leaves behind, the report it writes beside the edit - and, when it
    /// shipped, the edit opened here on the timeline, which is the point
    /// of the run.
    fn auto_edit_ended(&mut self, state: AutoEditState, studio: &mut Studio) {
        self.auto.running = false;
        self.auto.started = None;
        // A retryable end keeps the runner and its source for the resume;
        // a completed or hard-failed run has nothing to pick up from.
        let keep_for_retry = matches!(
            &state,
            AutoEditState::Failed { error, .. } if error.retryable
        ) || matches!(&state, AutoEditState::Cancelled { .. });
        if !keep_for_retry {
            self.auto.runner = None;
            self.auto.source = None;
            self.auto.tool_cancel = None;
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
                let exported = export.display().to_string();
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
                // same landing a forge run's edit has. The voiceover
                // source project itself stays untouched on disk.
                if let Some(path) = self.auto.project.clone() {
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
            }
            AutoEditState::Cancelled { rolled_back, .. } => {
                self.auto.stage = t("forge.autoEditCancelledStage");
                self.auto.outcome = 0;
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
                | AutoEditState::CompletedWithWarnings { receipt, .. } => receipt.as_deref(),
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
                studio.handle(Msg::Forge(ForgeMsg::Line(format!("acceptance: {summary}"))));
            }
        }
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
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                crate::host::on_ui(move |studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::Line(line)));
                });
            }
        }
    });
    let stderr_reader = std::thread::spawn(move || {
        if let Some(pipe) = err {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
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
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if kill.load(Ordering::Relaxed) {
                        let _ = child.kill();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => {
                    log::warn!("could not poll Forge child: {error}");
                    let _ = child.kill();
                    break child.wait().ok();
                }
            }
        };
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
    build_voiceover_sibling(
        studio.project(),
        std::path::Path::new(session.path()),
        for_auto_edit,
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
/// Start button and false for the AUTO EDIT button. The labels are written
/// as `t(...)` calls here, not returned as keys, so the locale inventory
/// sees them used.
fn other_run_refusal(other_running: bool, starting_start: bool) -> Option<String> {
    if !other_running {
        return None;
    }
    Some(if starting_start {
        t("forge.startBusy")
    } else {
        t("forge.autoEditBusy")
    })
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
    if voiced.is_empty() {
        return Err(t("forge.noVoiceover"));
    }
    // The distinct files, in first-play order - what gets copied in.
    let mut files: Vec<&concat_project::model::MediaItem> = Vec::new();
    for (_, item) in &voiced {
        if !files.iter().any(|known| known.path == item.path) {
            files.push(item);
        }
    }
    // The sibling folder: `<name> edit`, then `edit 2`, `edit 3`, ... so
    // every run gets a fresh project and an earlier edit is never
    // overwritten. The canvas matches the source project's.
    let video = timeline.video;
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
        let mut mint = concat_project::commands::IdMint::default();
        let mut copied: Vec<String> = Vec::with_capacity(files.len());
        let mut takes: Vec<TakePaths> = Vec::with_capacity(files.len());
        let mut media_id_of: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
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
            std::fs::copy(&item.path, &target)
                .map_err(|error| format!("could not copy voiceover {}: {error}", item.path))?;
            let path = target.to_string_lossy().into_owned();
            take_of.insert(item.path.clone(), path.clone());
            if for_auto_edit {
                takes.push(TakePaths {
                    take: path.clone(),
                    enhanced: into
                        .join(enhanced_copy_name(copied.len(), source_file))
                        .to_string_lossy()
                        .into_owned(),
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
        // One sibling lane per source lane, at the bottom of the stack in
        // first-play order, so takes recorded across lanes keep their
        // arrangement - and their overlap, which a single lane cannot
        // hold.
        let mut lanes: Vec<(String, String)> = Vec::new();
        for (clip, _) in &voiced {
            if !lanes.iter().any(|(from, _)| from == &clip.track_id) {
                lanes.push((clip.track_id.clone(), mint.next("t")));
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
        let settings = concat_project::doc::DocumentSettings {
            name: info.name.clone(),
            width: video.width,
            height: video.height,
            rate_num: video.rate_num,
            rate_den: video.rate_den,
        };
        let document = concat_project::doc::to_document(&settings, &sibling)
            .map_err(|error| format!("the voiceover project cannot be saved: {error}"))?;
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
        mapping.insert(
            "segments".to_owned(),
            serde_json::to_value(&segments)
                .map_err(|error| format!("the voiceover mapping cannot be written: {error}"))?,
        );
        let bytes = serde_json::to_vec_pretty(&mapping)
            .map_err(|error| format!("the voiceover mapping cannot be written: {error}"))?;
        std::fs::write(
            std::path::Path::new(&info.path).join("voiceover-mapping.json"),
            bytes,
        )
        .map_err(|error| format!("could not write the voiceover mapping: {error}"))?;
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
            let auto = for_auto_edit.then(|| {
                let export_dir = std::path::Path::new(&info.path).join("export");
                if let Err(error) = std::fs::create_dir_all(&export_dir) {
                    log::warn!(
                        "could not make the export folder {}: {error}",
                        export_dir.display()
                    );
                }
                AutoEditSource {
                    project: info.path.clone(),
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
                }
            });
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

/// Whether a vision critic answers without any setup: the environment the
/// editor was started with, a key the Forge page keeps (handed to the
/// spawned harness and to the in-process AUTO EDIT's tool calls alike), or
/// the saved `vlm.json` the `local_vlm {action:"configure"}` tool writes.
/// The run prompt used to look only at `FUNCUT_VLM_ENDPOINT` and
/// `FUNCUT_VLM_API_KEY`, so a critic configured the normal way was
/// invisible to it - the agent was told to set one up when one was
/// already active, and the quality loop started a step behind.
fn hosted_critic_ready(studio: &Studio) -> bool {
    if std::env::var("FUNCUT_VLM_ENDPOINT").is_ok() || std::env::var("FUNCUT_VLM_API_KEY").is_ok() {
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
        .any(|(field, name)| !field.trim().is_empty() || std::env::var(name).is_ok())
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
             pre-placed narration: repair a rough take before analysing it (Phase 1), tighten \
             only the dead air (Phase 2 - a dramatic hold is the performance, keep it), then \
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
        "Use the funcut-director skill and follow its phases IN ORDER. You are directing a \
         video, not assembling clips. The skill rides with two judgment shelves you MUST \
         consult, not decorate with: maestro (design, motion and film judgment - its shot \
         cards, its 157-shot vocabulary, its sound-design method) and clip-skills (the \
         editing craft - where to cut, the audio spine, B-roll matching, pacing, grading, \
         voice processing, export numbers). Phase 3 draws on clip-skills' audio-driven \
         cutting module and maestro's shot cards for scale, motion and register; Phase 4 \
         draws on clip-skills' B-roll matching module; name the module each decision used \
         in the plan's signalLog notes. An edit that never opened a shelf is an edit that \
         guessed.\n\n{brief}{material}\
         {look}\
         MANDATORY QUALITY LOOP (do not skip, do not just describe it):\n\
         1. plan_validate must return BOTH valid:true AND creativeReady:true. Fix every \
            creativeBlockers entry before cutting. A score below 7 means rewrite the plan.\n\
         2. Never choose B-roll from caption text. Call stock_contact_sheet with rank:true \
            and LOOK at the returned image; use cropHint when fitting square or landscape \
            clips into a vertical frame.\n\
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

/// Whether a model key is saved for the harness: any provider the Forge
/// page keeps. A harness with no key cannot think, so a Director run
/// without one would only fail after the sibling was built.
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
        &forge.hf,
    ]
    .iter()
    .any(|key| !key.trim().is_empty())
        || std::env::var("FUNCUT_FORGE_MODEL")
            .or_else(|_| std::env::var("FORGE_MODEL"))
            .is_ok_and(|model| !model.trim().is_empty())
}

/// Whether the AUTO EDIT can hand its edit to the Director: the harness
/// is on PATH, the Forge page has wired it to this editor, the skill is
/// installed, and a model key is saved. Every one of those is a thing the
/// user can see on the page, so a run that cannot hand over says which is
/// missing instead of quietly running the fixed spine and calling it the
/// same edit.
fn director_ready(studio: &Studio) -> Result<(), String> {
    // A saved key is enough to know the user means to use a model; the
    // harness itself still has to exist and be wired, which the page's
    // own probe answers.
    if !a_model_key_is_saved(studio) {
        return Err(
            "no model key is saved on the Forge page, so the Director has no model to think with"
                .to_owned(),
        );
    }
    if studio.forge.connected != Some(true) {
        return Err(
            "the Forge harness is not connected to this editor (press Connect on the Forge page)"
                .to_owned(),
        );
    }
    if studio.forge.installed != Some(true) {
        return Err(
            "the funcut-director skill is not installed (press Install on the Forge page)"
                .to_owned(),
        );
    }
    // The surface, not just the wiring. A run told to verify its export
    // meets a refusal mid-edit if the server it talks to does not serve
    // the call - and the edit it already made stays on the sibling. So
    // the tools the run cannot finish without are checked here, against
    // the same environment the harness's server reads, and a missing one
    // stops the handover before anything is cut.
    let unserved = concat_mcp::protocol::unserved(concat_mcp::protocol::DIRECTOR_REQUIRED);
    if !unserved.is_empty() {
        return Err(format!(
            "the editor's MCP surface does not serve {} - an edit that cannot measure itself \
             would report a success nobody checked; name {} on CONCAT_MCP_ALLOW (or serve the \
             whole catalogue) and try again",
            unserved.join(", "),
            if unserved.len() == 1 { "it" } else { "them" }
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
fn auto_director_prompt(source: &AutoEditSource) -> String {
    format!(
        "Use the funcut-director skill and follow its phases IN ORDER. This is an \
         UNATTENDED one-click edit: nobody is watching to answer a question, so make every \
         creative decision yourself and record it.\n\n\
         THE PROJECT: open '{project}' (project_open). The narration is ALREADY on the \
         timeline, reconstructed clip for clip with its own starts, in/out points, speed, \
         volume and fades; voiceover-mapping.json maps each clip to its source. Do NOT \
         re-import, re-place, retime or ripple the narration clips.\n\n\
         WHAT IS ALREADY DONE, AND WHAT IS NOT: the sibling exists and the narration is \
         placed; nothing has been MEASURED yet. The repair, the transcription, \
         voice_signals and emotion are YOURS to run on this sibling, in the skill's order - \
         do not skip them and do not assume them. Every creative decision must name the \
         signal that drove it in the plan's signalLog - an edit that never read the voice is \
         an edit that guessed, and an unavailable signal must be reported as missing rather \
         than claimed.\n\n\
         WHAT THE EDIT MUST BE: the whole pipeline around that narration - the plan \
         (plan_validate must return valid:true AND creativeReady:true), B-roll shopped with \
         stock_contact_sheet rank:true and LOOKED AT before choosing, captions on the \
         measured words, Geneva motion comps for the beats that earn them (motion_frame to \
         check legibility before motion_bake), ONE consistent grade, music ducked under the \
         voice, accents on the measured emphasis peaks. Use the editor's whole \
         vocabulary; do not settle for a template.\n\n\
         MANDATORY QUALITY LOOP: watch_render the OPEN timeline, craft_critique it, act on \
         at least one revision, watch_render AGAIN and compare the two craft scores, and \
         repeat until the overall score is >= 7 with no black frames, letterbox bars or long \
         static runs. Only then export.\n\n\
         THE EXPORT lands at '{export}'. Finish by calling watch_render on the exported file \
         with audio:true and craft_critique once more, then report the honest score and what \
         you changed. Never claim success on an unmeasured render. A file PATH to a PNG is \
         not a picture you can see: only inline image blocks are.\n",
        project = source.project,
        export = source.export,
    )
}

/// Starts the Director over the AUTO EDIT's sibling: the same harness a
/// Start Forge run spawns, given the sibling this run already built and
/// measured. The pane shows the harness's words as they come, and the
/// run's end opens the finished edit in the window the way a Start Forge
/// run does.
fn launch_auto_director(studio: &Studio, source: &AutoEditSource, kill: Arc<AtomicBool>) {
    let prompt = auto_director_prompt(source);
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
    let spawned = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    match spawned {
        Ok(child) => {
            crate::host::on_ui(|studio, _, _| {
                studio.handle(Msg::Forge(ForgeMsg::Line(
                    "the Director has the edit: it reads the measurements, plans, cuts and \
                     reviews its own work"
                        .to_owned(),
                )));
            });
            read_the_director_run(child, kill);
        }
        Err(error) => {
            crate::host::on_ui(move |studio, _, _| {
                studio.handle(Msg::Forge(ForgeMsg::Line(format!(
                    "the Director could not start: {error}"
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
    let out = child.stdout.take();
    let err = child.stderr.take();
    let stdout_reader = std::thread::spawn(move || {
        if let Some(pipe) = out {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                crate::host::on_ui(move |studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::Line(line)));
                });
            }
        }
    });
    let stderr_reader = std::thread::spawn(move || {
        if let Some(pipe) = err {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                crate::host::on_ui(move |studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::Line(line)));
                });
            }
        }
    });
    std::thread::spawn(move || {
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if kill.load(Ordering::Relaxed) {
                        let _ = child.kill();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(_) => {
                    let _ = child.kill();
                    break child.wait().ok();
                }
            }
        };
        let _ = stdout_reader.join();
        let _ = stderr_reader.join();
        let code = status.and_then(|status| status.code()).unwrap_or(-1);
        crate::host::on_ui(move |studio, _, _| {
            studio.handle(Msg::Forge(ForgeMsg::AutoDirectorEnded(code)));
        });
    });
}

/// The director skill's phases, in the order it runs them, with the
/// words its own headings carry. A handed-over AUTO EDIT has no runner
/// to name its stage, so the pane reads the harness's output instead and
/// advances only when a phase is actually named - the twelve the skill
/// prints, which is the same sequence a Start Forge run walks.
const DIRECTOR_PHASES: [&[&str]; 12] = [
    &["phase 1", "creative brief"],
    &["phase 2", "understand"],
    &["phase 3", "voice signal", "emotion"],
    &["phase 4", "transcri"],
    &["phase 5", "plan_validate", "plan"],
    &["phase 6", "b-roll", "stock_"],
    &["phase 7", "rough cut"],
    &["phase 8", "motion_", "geneva"],
    &["phase 9", "caption"],
    &["phase 10", "bgm_", "sounds_", "music"],
    &["phase 11", "watch_render", "craft_critique"],
    &["phase 12", "export_"],
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
        3 => t("forge.directorPhaseTranscribe"),
        4 => t("forge.directorPhasePlan"),
        5 => t("forge.directorPhaseFootage"),
        6 => t("forge.directorPhaseCut"),
        7 => t("forge.directorPhaseMotion"),
        8 => t("forge.directorPhaseCaption"),
        9 => t("forge.directorPhaseSound"),
        10 => t("forge.directorPhaseReview"),
        _ => t("forge.directorPhaseExport"),
    }
}

/// The phase a line of the harness's output names, and how far through
/// the skill's twelve phases that is. The last phase a line matches wins
/// nothing: a line that mentions two phases is a sentence about them,
/// not a stage, so only the earliest match is taken and the fraction is
/// that phase's own position.
fn director_phase(line: &str) -> Option<(usize, f32)> {
    let lower = line.to_ascii_lowercase();
    let total = DIRECTOR_PHASES.len() as f32;
    let at = |index: usize| (index, (index + 1) as f32 / total);
    // A numbered phase is matched by its number with a boundary after it:
    // "phase 1" sits inside "phase 12", so a plain substring scan would
    // read every phase ten and up as phase one.
    for index in 0..DIRECTOR_PHASES.len() {
        let number = format!("phase {}", index + 1);
        if let Some(found) = lower.find(&number)
            && !lower[found + number.len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        {
            return Some(at(index));
        }
    }
    // A phase the harness names by a tool instead of a number.
    DIRECTOR_PHASES
        .iter()
        .enumerate()
        .find(|(_, words)| words.iter().any(|word| lower.contains(word)))
        .map(|(index, _)| at(index))
}

/// Whether the harness is wired to Concat and the skill is where it
/// reads: `forge mcp get funcut` says the one, `forge commands` lists the
/// other. Neither is a reason to fail: the buttons say what is missing.
fn probe_setup() -> (Result<bool, String>, Result<bool, String>) {
    let connected = match forge(&["mcp", "get", "funcut"]) {
        Ok((code, _)) => Ok(code == 0),
        Err(error) => Err(error),
    };
    let installed = match forge(&["commands"]) {
        // Supporting shelves do not replace the required Director entry
        // point. A partial install must not pass preflight.
        Ok((code, words)) => {
            Ok(code == 0 && words.to_ascii_lowercase().contains("funcut-director"))
        }
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
        Err(first_line(&words).unwrap_or_else(|| "the command failed".to_owned()))
    }
}

/// Installs this repository's skill into the harness.
fn install_skill() -> Result<bool, String> {
    let (code, words) = forge(&["skill", "install", "coreselfmastery/FUNCUT"])?;
    if code == 0 {
        Ok(true)
    } else {
        Err(first_line(&words).unwrap_or_else(|| "the command failed".to_owned()))
    }
}

/// Runs `forge` with `args`, capturing what it said: its exit code, and
/// its output - standard error folded in, last words first for the caller
/// that reads the tail. A harness that is not there is Err, in the
/// harness's own spelling.
fn forge(args: &[&str]) -> Result<(i32, String), String> {
    let run = Command::new("forge")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
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
/// - the export, waited on by its project (the job's name is minted at
///   run time, so the wait asks by what it was started on);
/// - the verify: the words re-read from the render, the loudness
///   measured against the short-form target;
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
/// posts the clock and the stage when either moves; the run's own end
/// posts the terminal state.
fn run_auto_edit(
    runner: Runner,
    // The runner reads and writes the program it runs - the fan lanes and
    // the derived steps land in it - so the run's own copy is mutable
    // through to the `run_with_lanes` call inside its thread.
    mut program: EditProgram,
    backend: Box<dyn concat_mcp::Backend>,
    tool_cancel: Arc<AtomicBool>,
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
                                    message: "the tool box could not be built".to_owned(),
                                    retryable: Some(false),
                                    retry_after: None,
                                    code: None,
                                },
                            ),
                            attempt: 1,
                        },
                    ))));
                });
                return;
            }
        };
        let mut backend = backend;
        let cancelled = tool_cancel;
        let mut caller =
            concat_mcp::director::BackendToolCaller::new(&toolbox, backend.as_mut(), &cancelled);
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
                    "without a message".to_owned()
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
                            message: format!("the run panicked: {what}"),
                            retryable: Some(false),
                            retry_after: None,
                            code: None,
                        },
                    ),
                    attempt: 1,
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

#[cfg(test)]
mod tests {
    use super::director_phase;
    use super::other_run_refusal;
    use super::voiceover_copy_name;
    use std::path::Path;
    use std::time::Duration;

    /// The handed-over run's progress is read from the harness's own
    /// words, and only where a phase is actually named: a line about
    /// nothing in particular moves no bar, and the fraction a named phase
    /// earns is its own position in the skill's twelve - so the pane can
    /// never show a phase the Director has not reached.
    #[test]
    fn the_director_stage_is_read_from_the_harness_words() {
        assert_eq!(director_phase("just thinking"), None);
        assert_eq!(director_phase(""), None);

        let (first, at_first) = director_phase("## Phase 1 - Creative Brief").expect("phase one");
        assert_eq!(first, 0);
        assert!(at_first > 0.0 && at_first < 0.2);

        // "phase 1" is inside "phase 12": the numbered match must not
        // read the last phase as the first, which froze the bar at the
        // end of every run.
        let (last, at_last) =
            director_phase("Phase 12: export_start then export_wait").expect("phase twelve");
        assert_eq!(last, 11);
        assert!((at_last - 1.0).abs() < 1e-6);

        // The review phase is where the Director watches its own cut: the
        // tool name alone is enough to place it.
        let (review, _) = director_phase("craft_critique the sheet").expect("review");
        assert_eq!(review, 10);

        // A named phase advances; the one before it is behind it.
        let (plan, early) = director_phase("phase 5 plan_validate").expect("plan");
        assert_eq!(plan, 4);
        assert!(early > at_first);
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
            crate::i18n::t("forge.startBusy"),
            "Start while the auto edit runs names the auto edit"
        );
        assert_eq!(
            auto_refused,
            crate::i18n::t("forge.autoEditBusy"),
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
        let (built, auto) = super::build_voiceover_sibling(&source, &source_root, false).unwrap();
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

        let (_, auto) = super::build_voiceover_sibling(&source, &source_root, true).unwrap();
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
        let (built, _) = super::build_voiceover_sibling(&mixed, &root, false).unwrap();
        assert_eq!(built.segments.len(), 1, "only the take is the voiceover");
        // A project whose only audio is the bed refuses the handoff with
        // the no-voiceover notice, not a sibling of its music.
        assert!(super::build_voiceover_sibling(&bed_only, &root, false).is_err());
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
}
