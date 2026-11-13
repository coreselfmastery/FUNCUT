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
        if !from_timeline && script.trim().is_empty() {
            return;
        }
        let mut voiceover = None;
        if from_timeline {
            match voiceover_project(studio) {
                Ok(made) => voiceover = Some(made),
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
fn voiceover_project(studio: &Studio) -> Result<VoiceoverSource, String> {
    let Some(session) = studio.session.as_ref() else {
        return Err(t("forge.needProject"));
    };
    build_voiceover_sibling(studio.project(), std::path::Path::new(session.path()))
}

/// The handoff itself, apart from the studio so a test can drive it with
/// a plain project model: [`voiceover_project`] supplies the window's
/// two arguments and nothing else.
fn build_voiceover_sibling(
    source: &concat_project::model::Project,
    source_root: &std::path::Path,
) -> Result<VoiceoverSource, String> {
    use concat_project::model::ClipKind;

    let timeline = source.active();
    // The voiceover is EVERY audio clip on the timeline, each carrying the
    // identity it plays with - not the distinct files once each. A take
    // split into pieces is several clips of one file; an edit that trims,
    // retimes or levels the narration lives in those clip fields, not in
    // the file.
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
        if !item.path.is_empty() {
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
    let built = (|| -> Result<(Vec<String>, Vec<VoiceoverSegment>), String> {
        // The voiceover files are copied in, so the edit stands on its own
        // and the source project can be moved or deleted without breaking
        // it.
        let into = std::path::Path::new(&info.path).join("voiceover");
        std::fs::create_dir_all(&into).map_err(|error| error.to_string())?;
        let mut mint = concat_project::commands::IdMint::default();
        let mut copied: Vec<String> = Vec::with_capacity(files.len());
        let mut media_id_of: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut sibling_media: Vec<concat_project::model::MediaItem> = Vec::new();
        for item in &files {
            let target = into.join(voiceover_copy_name(
                copied.len(),
                std::path::Path::new(&item.path),
            ));
            std::fs::copy(&item.path, &target)
                .map_err(|error| format!("could not copy voiceover {}: {error}", item.path))?;
            let path = target.to_string_lossy().into_owned();
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
        Ok((copied, segments))
    })();
    match built {
        Ok((copied, segments)) => Ok(VoiceoverSource {
            project: info.path,
            files: copied,
            segments,
        }),
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
         video, not assembling clips.\n\n{brief}{material}\
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
#[cfg(test)]
mod tests {
    use super::voiceover_copy_name;
    use std::path::Path;

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
        let built = super::build_voiceover_sibling(&source, &source_root).unwrap();
        assert_eq!(built.segments.len(), 2);

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
        let segments = mapping["segments"].as_array().unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1]["source_clip_id"], "c2");
        assert_eq!(segments[1]["sibling_clip_id"], second.id);
        assert_eq!(segments[1]["source_in"], 0.5);
        assert_eq!(segments[1]["source_out"], 0.5 + 2.4 * 1.2);
        assert_eq!(segments[1]["track"], second.track_id);

        std::fs::remove_dir_all(&dir).ok();
    }
}
