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
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
/// The niches the page's select offers: the playbooks the skill carries.
const NICHES: [&str; 10] = [
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
    /// Start: the script and the three choices.
    Start {
        /// The words the voiceover will say.
        script: String,
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
    /// How the child is stopped. The pane never holds the child: the
    /// reader thread owns it, and a Cancel only raises the flag it is
    /// checked against, one line away.
    kill: Arc<AtomicBool>,
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
                voice,
                niche,
                mood,
            } => self.run(script, voice, niche, mood, studio),
            ForgeMsg::Cancel => {
                if self.running {
                    self.kill.store(true, Ordering::Relaxed);
                    self.set_status(t("forge.stopping"));
                }
            }
            ForgeMsg::Line(line) => {
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
    fn run(&mut self, script: String, voice: i32, niche: i32, mood: i32, studio: &mut Studio) {
        if self.running || script.trim().is_empty() {
            return;
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
        let prompt = director_run_prompt(&script, voice, niche, mood, studio);
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
        for (name, value) in [
            ("GROQ_API_KEY", &forge.groq),
            ("GEMINI_API_KEY", &forge.gemini),
            ("OPENROUTER_API_KEY", &forge.openrouter),
            ("MISTRAL_API_KEY", &forge.mistral),
            ("CEREBRAS_API_KEY", &forge.cerebras),
            ("SAMBANOVA_API_KEY", &forge.sambanova),
            ("NVIDIA_API_KEY", &forge.nim),
            ("FUNCUT_EMOTION_API_KEY", &forge.hf),
            ("HF_TOKEN", &forge.hf),
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
}

/// Reads the run on its own thread: each line of the harness's output is
/// posted as it comes, and the run's exit code is posted after the last.
/// A Cancel is honoured at the next line by killing the child; the posts
/// that follow say what happened, not that it was read to the end.
fn read_the_run(mut child: Child, kill: Arc<AtomicBool>) {
    let out = child.stdout.take();
    let err = child.stderr.take();
    let err_kill = Arc::clone(&kill);
    let reader = std::thread::spawn(move || {
        let Some(pipe) = err else {
            return;
        };
        for line in BufReader::new(pipe).lines().map_while(Result::ok) {
            if err_kill.load(Ordering::Relaxed) {
                break;
            }
            crate::host::on_ui(move |studio, _, _| {
                studio.handle(Msg::Forge(ForgeMsg::Line(line)));
            });
        }
    });
    std::thread::spawn(move || {
        if let Some(pipe) = out {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if kill.load(Ordering::Relaxed) {
                    let _ = child.kill();
                    break;
                }
                crate::host::on_ui(move |studio, _, _| {
                    studio.handle(Msg::Forge(ForgeMsg::Line(line)));
                });
            }
        }
        let _ = reader.join();
        let code = child.wait().ok().and_then(|status| status.code());
        crate::host::on_ui(move |studio, _, _| {
            studio.handle(Msg::Forge(ForgeMsg::Ended(code.unwrap_or(-1))));
        });
    });
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
    let critic = if std::env::var("FUNCUT_VLM_ENDPOINT").is_ok()
        || std::env::var("FUNCUT_VLM_API_KEY").is_ok()
    {
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
    format!(
        "Use the funcut-director skill and follow its phases IN ORDER. You are directing a \
         video, not assembling clips.\n\n{brief}\
         PRODUCTION CHOICES: the {niche} playbook, {voice} narration (a Kokoro voice), \
         music for the mood '{mood}', measured word-synced captions, ONE consistent grade, \
         and silence removed from the voiceover.\n\n\
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
         THE SCRIPT:\n{script}"
    )
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
        Ok((_, words)) => Ok(words.to_ascii_lowercase().contains("funcut-director")),
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
