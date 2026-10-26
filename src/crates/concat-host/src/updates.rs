// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Which release this is, which releases are out, and putting one of them
//! in this one's place.
//!
//! Every release on GitHub carries a `manifest.json` naming each
//! platform's package with its URL, its size and its SHA-256 - the release
//! workflow writes it. That is what a switch reads: the list of releases
//! comes from GitHub's API, the package for this machine from the chosen
//! release's manifest, and the bytes are checked against the manifest's
//! digest before they are put anywhere, the road the models take (see
//! [`crate::models`]).
//!
//! Only releases from [`FIRST_SWITCHABLE`] on are offered. A release before
//! it has no page to come back up by, so a person who went there would be
//! stranded.
//!
//! How a package goes in place is the platform's affair. Windows runs the
//! installer, which replaces the files once the app has closed. macOS
//! copies the app out of the disk image over the bundle this is running
//! from. An AppImage is written over the file it was started from. A .deb,
//! an .rpm or a pacman package is handed to the desktop's installer. The
//! caller closes the app where that is needed and [`relaunch`] starts the
//! new one.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;

use crate::dirs::AppDirs;
use crate::models;

/// The repository the releases are read from: this fork's own, with
/// upstream Concat's as the fallback when the fork has published none -
/// the same preference `install-linux.sh` gives a person at the command
/// line. An in-app update offered from upstream would swap the fork's
/// tuning for a build the fork never made.
pub const REPO: &str = "coreselfmastery/FUNCUT";

/// Upstream Concat, whose releases a fork with none of its own published
/// falls back to.
const UPSTREAM_REPO: &str = "jub0t/Concat";

/// The first release with a Version page: the earliest one a person can
/// come back from, so the earliest one offered.
pub const FIRST_SWITCHABLE: Version = Version::new(0, 2, 5);

/// GitHub's list of a repository's releases, newest first. A hundred
/// covers years of releases at this pace.
fn releases_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/releases?per_page=100")
}

/// What a download that was stopped says.
pub const CANCELLED: &str = "the download was cancelled";

/// A release's number: `0.2.5`. A tag carries a `v` in front of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// The first number.
    pub major: u64,
    /// The second.
    pub minor: u64,
    /// The third.
    pub patch: u64,
    /// This fork's own iteration on that base, from a tag like
    /// `v0.2.8-funcut.5`: the fork re-cuts a base release as `.1`,
    /// `.2`, ... while the workspace's own version - the number the
    /// bundles are named by - stays what it is. `None` for a plain
    /// release, upstream's or this fork's. Ordered last, so a cut of a
    /// base is newer than the base and older than the next base: an
    /// install of `.5` is behind `.6` and ahead of `0.2.8` itself.
    pub funcut: Option<u64>,
}

impl Version {
    /// The version with these three numbers.
    pub const fn new(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
            funcut: None,
        }
    }

    /// `0.2.5`, `v0.2.5`, or this fork's own `v0.2.8-funcut.5`. Anything
    /// else - a pre-release such as `v0.3.0-beta.1`, the `nightly` tag,
    /// two numbers - is `None`: not a release a person is offered. The
    /// fork's tags are the fork's *releases*, not pre-releases: a tag
    /// this cannot read is a tag the updater cannot see, and a fork
    /// whose every release reads as nothing falls back to upstream
    /// Concat - the very thing a person chose this fork over.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        // A `-funcut.N` suffix is the fork's own iteration on the base
        // in front of it; every other suffix - beta, rc, a shelf's tag -
        // stays what it was, not a release.
        let (text, funcut) = match text.split_once('-') {
            Some((base, suffix)) => (base, Some(suffix.strip_prefix("funcut.")?.parse().ok()?)),
            None => (text, None),
        };
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            funcut,
        })
    }

    /// The version this binary was built as: the workspace's, and - when
    /// the release workflow built this from one of the fork's own tags -
    /// the iteration that tag carried, read out of `FUNCUT_ITERATION` at
    /// compile time. Without it a build of `.5` would know itself only
    /// as `0.2.8`, and sit forever behind the `.5` it already is.
    pub fn current() -> Version {
        let version = Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version::new(0, 0, 0));
        match option_env!("FUNCUT_ITERATION") {
            Some(iteration) => Version {
                funcut: iteration.parse().ok(),
                ..version
            },
            None => version,
        }
    }

    /// The release's tag: `v0.2.5`.
    pub fn tag(self) -> String {
        format!("v{self}")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some(iteration) = self.funcut {
            write!(f, "-funcut.{iteration}")?;
        }
        Ok(())
    }
}

/// One release a person can switch to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// Its number.
    pub version: Version,
    /// Its tag on GitHub, `v0.2.5`, which names its download folder.
    pub tag: String,
    /// The repository the release lives in: this fork, or upstream
    /// when the fork has published none and the list fell back to it.
    /// A release's manifest is where its own repository keeps it.
    pub repo: &'static str,
    /// The day it was published, `2026-09-24`; empty when GitHub did not
    /// say.
    pub published: String,
}

/// Reads GitHub's answer to the releases request: the stable releases from
/// [`FIRST_SWITCHABLE`] on, newest first, belonging to `repo`. Drafts and
/// pre-releases are left out, as is anything whose tag is not a plain
/// version.
fn releases_from_json(json: &str, repo: &'static str) -> Result<Vec<Release>, String> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("the release list does not read as JSON: {error}"))?;
    let Some(entries) = value.as_array() else {
        // GitHub answers an object with a message when it refuses: the
        // rate limit, for the most part.
        return Err(format!("GitHub answered: {}", github_message(&value)));
    };
    let mut releases: Vec<Release> = entries
        .iter()
        .filter_map(|entry| {
            let flag = |name: &str| {
                entry
                    .get(name)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            };
            if flag("draft") || flag("prerelease") {
                return None;
            }
            let tag = entry.get("tag_name")?.as_str()?;
            let version = Version::parse(tag)?;
            if version < FIRST_SWITCHABLE {
                return None;
            }
            let published = entry
                .get("published_at")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .chars()
                .take(10)
                .collect();
            Some(Release {
                version,
                tag: tag.to_owned(),
                repo,
                published,
            })
        })
        .collect();
    releases.sort_by(|a, b| b.version.cmp(&a.version));
    releases.dedup_by(|a, b| a.version == b.version);
    Ok(releases)
}

/// The `message` of a GitHub error body, or the body's first line.
fn github_message(value: &serde_json::Value) -> String {
    value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map_or_else(
            || value.to_string().chars().take(200).collect(),
            str::to_owned,
        )
}

/// Asks GitHub for the releases; see [`releases_from_json`]. This fork's
/// own are preferred, and only a build without the fork's own iteration -
/// upstream's own, or a plain workspace build - ever falls back to
/// upstream Concat's list: a build tagged `-funcut.N` whose own list
/// comes back empty (a rate limit, a private flip, a deleted release)
/// is told so, because an in-app update offered from upstream would
/// swap the fork's tuning for a build the fork never made.
pub fn fetch_releases() -> Result<Vec<Release>, String> {
    let own = own_releases();
    match own {
        Ok(releases) if !releases.is_empty() => Ok(releases),
        _ if option_env!("FUNCUT_ITERATION").is_some() => Err(format!(
            "no published releases of {REPO} answered; upstream Concat's are refused, \
             they would replace this fork with a build it never made"
        )),
        _ => releases_from_json(&get_text(&releases_url(UPSTREAM_REPO))?, UPSTREAM_REPO),
    }
}

/// The fork's own releases, paged: a hundred a page covers years of app
/// releases, but the data shelves share the same release list, and a
/// page that comes back short is the last one. Three pages is years of
/// headroom over anything this repo has published so far.
fn own_releases() -> Result<Vec<Release>, String> {
    let mut all = Vec::new();
    for page in 1..=3 {
        let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=100&page={page}");
        let page_releases = releases_from_json(&get_text(&url)?, REPO)?;
        let short = page_releases.len() < 100;
        all.extend(page_releases);
        if short {
            break;
        }
    }
    Ok(all)
}

/// Where this build sits among the releases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// This build is the newest release.
    Latest,
    /// A newer release is out: this one.
    Behind(Version),
    /// This build is newer than any release: a build from the tree.
    Ahead,
    /// No release from [`FIRST_SWITCHABLE`] on is out.
    NoReleases,
}

/// Where `current` sits among `releases`.
pub fn standing(current: Version, releases: &[Release]) -> Standing {
    match releases.iter().map(|release| release.version).max() {
        None => Standing::NoReleases,
        Some(newest) if newest > current => Standing::Behind(newest),
        Some(newest) if newest < current => Standing::Ahead,
        Some(_) => Standing::Latest,
    }
}

/// The kind of package this build was installed from, which is the kind
/// that goes in its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageKind {
    /// A Linux AppImage: one file, started where it lies.
    AppImage,
    /// A Debian package.
    Deb,
    /// An RPM.
    Rpm,
    /// An Arch package.
    Pacman,
}

impl PackageKind {
    /// The key the release manifest files the package under.
    pub fn key(self) -> &'static str {
        match self {
            PackageKind::AppImage => "appimage",
            PackageKind::Deb => "deb",
            PackageKind::Rpm => "rpm",
            PackageKind::Pacman => "pacman",
        }
    }
}

/// Why this build cannot be replaced from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fixed {
    /// A Flatpak: Flatpak keeps it up to date.
    Flatpak,
    /// Not installed from any package this knows - a build from the tree,
    /// a distribution's own package.
    NotFromPackage,
}

/// How this build was installed, or why it cannot be switched from here.
/// Worked out once: on Linux it reads the package databases.
pub fn installed_as() -> Result<PackageKind, Fixed> {
    static KNOWN: OnceLock<Result<PackageKind, Fixed>> = OnceLock::new();
    *KNOWN.get_or_init(|| {
        linux_kind(
            std::env::var_os("APPIMAGE").is_some(),
            std::env::var_os("FLATPAK_ID").is_some(),
            Path::new("/var/lib/dpkg/info/concat.list").exists(),
            pacman_has_concat(),
            rpm_has_concat(),
        )
    })
}

/// A Linux install, from what is on the machine: an AppImage says so in
/// the environment, and a package is in its manager's database.
fn linux_kind(
    appimage: bool,
    flatpak: bool,
    dpkg: bool,
    pacman: bool,
    rpm: bool,
) -> Result<PackageKind, Fixed> {
    if flatpak {
        Err(Fixed::Flatpak)
    } else if appimage {
        Ok(PackageKind::AppImage)
    } else if dpkg {
        Ok(PackageKind::Deb)
    } else if pacman {
        Ok(PackageKind::Pacman)
    } else if rpm {
        Ok(PackageKind::Rpm)
    } else {
        Err(Fixed::NotFromPackage)
    }
}

/// Whether pacman's database lists the `concat` package: a folder named
/// `concat-<version>-<release>` under its local store.
fn pacman_has_concat() -> bool {
    std::fs::read_dir("/var/lib/pacman/local").is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.strip_prefix("concat-")
                .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        })
    })
}

/// Whether the RPM database lists `concat`. Asked of `rpm` itself, and only
/// where there is a database to ask about.
fn rpm_has_concat() -> bool {
    Path::new("/var/lib/rpm").exists()
        && Command::new("rpm")
            .args(["-q", "concat"])
            .output()
            .is_ok_and(|output| output.status.success())
}

/// One platform's package in a release: what to fetch and what it must
/// hash to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    /// Its file name, `Concat-0.2.5-linux-x86_64.AppImage`.
    pub file: String,
    /// Where to fetch it.
    pub url: String,
    /// Its size, for the progress bar before the server has said.
    pub bytes: u64,
    /// Its SHA-256 as lowercase hex.
    pub sha256: String,
}

/// The name the manifest gives this machine's architecture.
pub fn manifest_arch(os: &str, arch: &str) -> String {
    let _ = os;
    arch.to_owned()
}

/// The package of `kind` for `os` on `arch` in a release's manifest.
pub fn package_from_manifest(
    json: &str,
    os: &str,
    arch: &str,
    kind: PackageKind,
) -> Result<Package, String> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|error| format!("the release manifest does not read as JSON: {error}"))?;
    let entry = value
        .get("binaries")
        .and_then(|binaries| binaries.get(os))
        .and_then(|platform| platform.get(manifest_arch(os, arch)))
        .and_then(|machine| machine.get(kind.key()))
        .ok_or_else(|| format!("the release has no {} package for {os} {arch}", kind.key()))?;
    let text = |name: &str| {
        entry
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("the manifest's package has no {name}"))
    };
    let file = text("file")?;
    // The name goes into a path under the app's own folder, and comes off
    // the network: a name with a folder in it is refused, not honoured.
    if file.is_empty() || file.starts_with('.') || file.contains(['/', '\\']) {
        return Err(format!(
            "the manifest names the package {file:?}, which is not a file name"
        ));
    }
    Ok(Package {
        file,
        url: text("url")?,
        bytes: entry
            .get("bytes")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        sha256: text("sha256")?,
    })
}

/// Where a release's manifest is: in the repository the release came
/// from, which is where its packages are too.
pub fn manifest_url(repo: &str, tag: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{tag}/manifest.json")
}

/// The package of `kind` for this machine in `release`, from its manifest.
pub fn fetch_package(release: &Release, kind: PackageKind) -> Result<Package, String> {
    package_from_manifest(
        &get_text(&manifest_url(release.repo, &release.tag))?,
        std::env::consts::OS,
        std::env::consts::ARCH,
        kind,
    )
}

/// Where packages are downloaded to.
pub fn folder(dirs: &AppDirs) -> PathBuf {
    dirs.data.join("updates")
}

/// Fetches `package` into `folder` and checks it against its digest,
/// reporting `(received, total)` bytes as it goes and stopping when
/// `cancel` is set. One package at a time: whatever an earlier switch left
/// in the folder goes first, but a partial of this same package stays and
/// is resumed.
pub fn download_package(
    package: &Package,
    folder: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder)
        .map_err(|error| format!("could not create {}: {error}", folder.display()))?;
    let partial = folder.join(format!("{}.part", package.file));
    let done = folder.join(&package.file);
    if let Ok(entries) = std::fs::read_dir(folder) {
        for path in entries.flatten().map(|entry| entry.path()) {
            if path != partial {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    models::download(
        &package.url,
        &partial,
        package.bytes,
        cancel,
        CANCELLED,
        progress,
    )?;
    if let Err(error) = models::verify(&partial, &package.sha256) {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    std::fs::rename(&partial, &done)
        .map_err(|error| format!("could not write {}: {error}", done.display()))?;
    Ok(done)
}

/// What [`install`] did, which decides what the app does next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installed {
    /// The installer is running: the app closes so it can finish.
    InstallerRunning,
    /// The new build is in this one's place: the app closes and
    /// [`relaunch`] starts it.
    Replaced,
    /// The package is open in the desktop's installer: the person finishes
    /// there and starts the app again.
    Opened,
}

/// Puts `package`, a file of `kind` already checked against its digest, in
/// this build's place.
pub fn install(package: &Path, kind: PackageKind) -> Result<Installed, String> {
    match kind {
        PackageKind::AppImage => replace_appimage(package),
        PackageKind::Deb | PackageKind::Rpm | PackageKind::Pacman => {
            run(Command::new("xdg-open").arg(package))?;
            Ok(Installed::Opened)
        }
    }
}

/// Writes the new AppImage over the file this one was started from, which
/// the environment names; the launcher entries that point at it keep
/// working.
fn replace_appimage(package: &Path) -> Result<Installed, String> {
    let target = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .ok_or_else(|| "this is not running as an AppImage".to_owned())?;
    let mut staged = target.clone().into_os_string();
    staged.push(".new");
    let staged = PathBuf::from(staged);
    std::fs::copy(package, &staged)
        .map_err(|error| format!("could not write {}: {error}", staged.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| format!("could not mark {} executable: {error}", staged.display()))?;
    }
    std::fs::rename(&staged, &target)
        .map_err(|error| format!("could not replace {}: {error}", target.display()))?;
    Ok(Installed::Replaced)
}

/// Starts the build now in place a moment from now, so the caller can quit
/// first.
pub fn relaunch() -> Result<(), String> {
    let target = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .ok_or_else(|| "could not tell what to start".to_owned())?;
    detach(
        Command::new("sh")
            .args(["-c", "sleep 1; exec \"$0\""])
            .arg(target),
    )
}

/// Runs a command to its end and reads its failure off standard error.
fn run(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let output = command
        .output()
        .map_err(|error| format!("could not run {program}: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let said = said.trim();
    Err(if said.is_empty() {
        format!("{program} failed ({})", output.status)
    } else {
        format!("{program} failed: {said}")
    })
}

/// Starts a command and lets it go, with no pipes back to this process.
fn detach(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not start {program}: {error}"))
}

// ── the automatic check ───────────────────────────────────────────────
//
// The check on the Version page is the one a person asks for. This is the
// one the app asks for itself, no more often than every three days, and
// the whole of it is honest about what it will not do: it never installs
// anything a person did not choose (an AppImage release is downloaded and
// left waiting in the updates folder, a package release is only said
// about, because apt and dnf are the ones that install), it never falls
// back to upstream (fetch_releases' own line, which a `-funcut.N` build
// cannot cross), and it leaves a marker so a restart restarts nothing -
// the three days are counted from the last check that ran, not from the
// process that happens to be open.

/// How long the app leaves between automatic checks: three days.
pub const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(72 * 60 * 60);

/// The file under the data folder that remembers when a check last ran -
/// automatic or the Version page's own, which both reset the same clock -
/// so a launch does not ask again what a launch just asked.
fn checked_marker(dirs: &AppDirs) -> PathBuf {
    dirs.data.join("update-check")
}

/// When the last check ran, as the marker wrote it; `None` is never.
pub fn last_checked(dirs: &AppDirs) -> Option<std::time::SystemTime> {
    let seconds = std::fs::read_to_string(checked_marker(dirs))
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()?;
    std::time::SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(seconds))
}

/// Writes `when` as the time of the last check.
pub fn record_checked(dirs: &AppDirs, when: std::time::SystemTime) {
    if let Ok(seconds) = when.duration_since(std::time::SystemTime::UNIX_EPOCH) {
        let _ = std::fs::create_dir_all(&dirs.data);
        let _ = std::fs::write(checked_marker(dirs), seconds.as_secs().to_string());
    }
}

/// Whether a check is due at `now`: never having run is due, and so is a
/// last check older than [`CHECK_INTERVAL`]. A marker from a clock set
/// backwards - a last check "in the future" - reads as due rather than
/// holding the app off until that future arrives, because a machine whose
/// clock was wrong is a machine whose marker is wrong too.
pub fn check_due(last: Option<std::time::SystemTime>, now: std::time::SystemTime) -> bool {
    match last {
        None => true,
        Some(last) => match now.duration_since(last) {
            Ok(elapsed) => elapsed >= CHECK_INTERVAL,
            Err(_) => true,
        },
    }
}

/// What the automatic check decides, in the order a log line or a toast
/// can say it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutoAction {
    /// Nothing to do, with the reason.
    Skip {
        /// The reason, phrased for a log line.
        why: String,
    },
    /// A newer release is out and this machine's package is one a person
    /// installs (a .deb, an RPM, an Arch package): it is named, and the
    /// installing is theirs.
    Announce {
        /// The release that is out.
        version: Version,
    },
    /// A newer release is out; its AppImage is downloaded and
    /// digest-checked, waiting at `file` for the Version page's switch.
    Downloaded {
        /// The release that was fetched.
        version: Version,
        /// Where the checked package landed.
        file: PathBuf,
    },
    /// A newer release is out and its AppImage was the thing to fetch, but
    /// the fetch or the digest failed: the reason is carried, the person
    /// is not left guessing.
    Failed {
        /// The release that could not be fetched.
        version: Version,
        /// Why not.
        why: String,
    },
}

/// Which release the automatic check should act on, if any: the standing
/// and the install are the whole decision, which is why they are the whole
/// of this function and the network stays out of it.
pub fn auto_standing(standing: Standing, installed: Result<PackageKind, Fixed>) -> Option<Version> {
    let Standing::Behind(version) = standing else {
        return None;
    };
    match installed {
        // Flatpak updates itself; a build from the tree or a distro
        // package is switched the way it was installed, not from here.
        Err(_) => None,
        Ok(_) => Some(version),
    }
}

/// Runs one automatic check: reads the releases the way the Version page
/// does, and acts on what this machine is. Nothing here installs - the
/// nearest it comes is the AppImage's download, which lands in the
/// updates folder and waits for a person's Switch Version.
pub fn automatic_check(dirs: &AppDirs) -> AutoAction {
    let current = Version::current();
    let releases = match fetch_releases() {
        Ok(releases) => releases,
        // A private repository with no token, a rate limit, a network
        // down: all said, none acted on, and never a fall back to
        // upstream - that is fetch_releases' own line, held for the
        // automatic check exactly as for the asked-for one.
        Err(why) => {
            return AutoAction::Skip {
                why: format!("could not read the releases: {why}"),
            };
        }
    };
    let standing = standing(current, &releases);
    let Some(newer) = auto_standing(standing, installed_as()) else {
        return AutoAction::Skip {
            why: match standing {
                Standing::Latest => "this build is the newest release".to_owned(),
                Standing::Ahead => {
                    "this build is newer than any release - a build from the tree".to_owned()
                }
                Standing::NoReleases => "no releases to read".to_owned(),
                Standing::Behind(_) => unreachable!("auto_standing named a Behind"),
            },
        };
    };
    match installed_as() {
        Ok(PackageKind::AppImage) => {
            // The release the standing named is one the list carries;
            // a list that somehow does not is said, not unwrapped.
            let release = releases.iter().find(|release| release.version == newer);
            match release.and_then(|release| fetch_package(release, PackageKind::AppImage).ok()) {
                Some(package) => {
                    let quiet = &mut |_received: u64, _total: u64| {};
                    let off = &AtomicBool::new(false);
                    match download_package(&package, &folder(dirs), off, quiet) {
                        Ok(file) => AutoAction::Downloaded {
                            version: newer,
                            file,
                        },
                        Err(why) => AutoAction::Failed {
                            version: newer,
                            why,
                        },
                    }
                }
                None => AutoAction::Failed {
                    version: newer,
                    why: format!(
                        "the release {}'s manifest did not answer an AppImage for this machine",
                        newer.tag()
                    ),
                },
            }
        }
        Ok(_) => AutoAction::Announce { version: newer },
        Err(_) => AutoAction::Skip {
            why: "this build was not installed from a package".to_owned(),
        },
    }
}

/// One GET, read whole as text. GitHub wants a user agent and says so with
/// a 403 otherwise. A token in the environment (`GITHUB_TOKEN`, or
/// `GH_TOKEN` - the same two names the installer takes) lifts the
/// anonymous sixty-requests-an-hour ceiling a Version-page check can
/// otherwise hit; it is sent to GitHub only, and only when present.
fn get_text(url: &str) -> Result<String, String> {
    // The fork's own rule holds here: a token the environment carries is
    // attached, because the fork's releases live beside its private
    // mirrors when they do; upstream's fetch carries no token and never
    // did. The request is dressed through net::get's own door, so the
    // proxy and the failure reasons apply to it as to every download.
    let user_agent = format!("Concat/{}", Version::current());
    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .ok();
    let response = crate::net::get(
        url,
        std::time::Duration::from_secs(20),
        std::time::Duration::from_secs(30),
        crate::net::Trust::Shipped,
        &|request| {
            let mut request = request
                .set("User-Agent", &user_agent)
                .set("Accept", "application/vnd.github+json");
            if let Some(token) = &token {
                request = request.set("Authorization", &format!("Bearer {token}"));
            }
            request
        },
    )
    .map_err(|failure| {
        let crate::net::Failure {
            error,
            cause,
            proxy,
        } = *failure;
        match error {
            ureq::Error::Status(code, response) => {
                let body = response.into_string().unwrap_or_default();
                let said = serde_json::from_str::<serde_json::Value>(&body)
                    .map(|value| github_message(&value))
                    .unwrap_or_else(|_| body.chars().take(200).collect());
                format!("{url} answered {code}: {said}")
            }
            other => {
                let through = proxy
                    .map(|proxy| format!(" (through the proxy {proxy})"))
                    .unwrap_or_default();
                format!(
                    "{url} did not answer: {}{through}: {other}",
                    cause.describe()
                )
            }
        }
    })?;
    response
        .into_string()
        .map_err(|error| format!("{url} was cut short: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_reads_a_tag_and_orders_by_its_numbers() {
        assert_eq!(Version::parse("v0.2.5"), Some(Version::new(0, 2, 5)));
        assert_eq!(Version::parse("0.2.5"), Some(Version::new(0, 2, 5)));
        assert_eq!(Version::parse(" v1.0.0 "), Some(Version::new(1, 0, 0)));
        for not_a_release in ["v0.3.0-beta.1", "nightly", "0.2", "0.2.5.1", "", "v"] {
            assert_eq!(Version::parse(not_a_release), None, "{not_a_release:?}");
        }
        // By number, not by text: ten comes after nine.
        assert!(Version::new(0, 2, 10) > Version::new(0, 2, 9));
        assert!(Version::new(0, 3, 0) > Version::new(0, 2, 99));
        assert!(Version::new(1, 0, 0) > Version::new(0, 9, 9));
        assert_eq!(Version::new(0, 2, 5).to_string(), "0.2.5");
        assert_eq!(Version::new(0, 2, 5).tag(), "v0.2.5");
        assert!(Version::current() >= Version::new(0, 2, 4));
    }

    #[test]
    fn the_forks_own_tags_read_and_order_by_their_iteration() {
        let cut = |n: u64| Version {
            funcut: Some(n),
            ..Version::new(0, 2, 8)
        };
        assert_eq!(Version::parse("v0.2.8-funcut.5"), Some(cut(5)));
        assert_eq!(Version::parse(" 0.2.8-funcut.12 "), Some(cut(12)));
        assert_eq!(cut(5).to_string(), "0.2.8-funcut.5");
        assert_eq!(cut(5).tag(), "v0.2.8-funcut.5");
        // A cut of a base is newer than the base and than an earlier cut,
        // and older than the next base: the fork re-cuts 0.2.8 while
        // upstream numbers on to 0.2.9.
        assert!(cut(5) > cut(4));
        assert!(cut(1) > Version::new(0, 2, 8));
        assert!(Version::new(0, 2, 9) > cut(5));
        // Nothing else with a hyphen opens: a pre-release, a shelf's own
        // tag, a suffix spelled wrong, a cut without its number.
        for not_a_release in [
            "v0.3.0-beta.1",
            "v0.2.8-funcut",
            "v0.2.8-funcut.5.1",
            "v0.2.8-FUNCUT.5",
            "v0.2.8-funcut.five",
            "audio-v1",
            "openvid-index-v1",
        ] {
            assert_eq!(Version::parse(not_a_release), None, "{not_a_release:?}");
        }
    }

    #[test]
    fn the_forks_own_release_list_reads_with_its_shelves_left_out() {
        let releases = releases_from_json(
            r#"[
            {"tag_name": "v0.2.8-funcut.5", "draft": false, "prerelease": false, "published_at": "2026-10-31T11:16:35Z"},
            {"tag_name": "openvid-index-v1", "draft": false, "prerelease": false, "published_at": "2026-10-23T14:51:30Z"},
            {"tag_name": "v0.2.8-funcut.4", "draft": false, "prerelease": false, "published_at": "2026-10-29T14:08:30Z"},
            {"tag_name": "v0.2.8-funcut.6", "draft": true, "prerelease": false, "published_at": null}
        ]"#,
            REPO,
        )
        .expect("reads");
        let tags: Vec<&str> = releases.iter().map(|r| r.tag.as_str()).collect();
        // The shelf's index and the still-unpublished .6 are not
        // releases a person is offered.
        assert_eq!(tags, ["v0.2.8-funcut.5", "v0.2.8-funcut.4"]);
        assert_eq!(releases[0].repo, REPO);
    }

    #[test]
    fn a_build_from_the_forks_own_tag_stands_where_it_is() {
        let releases = vec![Release {
            version: Version::parse("v0.2.8-funcut.6").expect("reads"),
            tag: "v0.2.8-funcut.6".to_owned(),
            repo: REPO,
            published: "2026-10-31".to_owned(),
        }];
        let on_it = Version {
            funcut: Some(6),
            ..Version::new(0, 2, 8)
        };
        assert_eq!(standing(on_it, &releases), Standing::Latest);
        let one_behind = Version {
            funcut: Some(5),
            ..Version::new(0, 2, 8)
        };
        assert_eq!(
            standing(one_behind, &releases),
            Standing::Behind(releases[0].version)
        );
        // A build that knows no iteration - one from the tree - sits
        // behind the fork's cut of the same base, which it is.
        assert_eq!(
            standing(Version::new(0, 2, 8), &releases),
            Standing::Behind(releases[0].version)
        );
    }

    const RELEASES: &str = r#"[
        {"tag_name": "nightly", "draft": false, "prerelease": true, "published_at": "2026-10-03T01:00:00Z"},
        {"tag_name": "v0.2.7", "draft": true, "prerelease": false, "published_at": null},
        {"tag_name": "v0.3.0-beta.1", "draft": false, "prerelease": true, "published_at": "2026-10-02T12:00:00Z"},
        {"tag_name": "v0.2.5", "draft": false, "prerelease": false, "published_at": "2026-09-28T09:30:00Z"},
        {"tag_name": "v0.2.6", "draft": false, "prerelease": false, "published_at": "2026-10-01T17:34:02Z"},
        {"tag_name": "v0.2.4", "draft": false, "prerelease": false, "published_at": "2026-09-24T17:34:02Z"},
        {"tag_name": "v0.2.6", "draft": false, "prerelease": false, "published_at": "2026-10-01T17:34:02Z"}
    ]"#;

    #[test]
    fn the_list_keeps_stable_releases_from_the_first_switchable_newest_first() {
        let releases = releases_from_json(RELEASES, REPO).expect("reads");
        let versions: Vec<String> = releases
            .iter()
            .map(|release| release.version.to_string())
            .collect();
        assert_eq!(versions, ["0.2.6", "0.2.5"]);
        assert_eq!(releases[0].tag, "v0.2.6");
        assert_eq!(releases[0].published, "2026-10-01");
        assert_eq!(releases[1].published, "2026-09-28");
        assert!(releases_from_json("[]", REPO).expect("reads").is_empty());
    }

    #[test]
    fn a_refusal_from_github_is_read_out_in_its_own_words() {
        let error = releases_from_json(
            r#"{"message": "API rate limit exceeded for 1.2.3.4."}"#,
            REPO,
        )
        .expect_err("refused");
        assert!(error.contains("rate limit"), "{error}");
        assert!(releases_from_json("not json", REPO).is_err());
    }

    #[test]
    fn standing_says_where_this_build_sits() {
        let releases = releases_from_json(RELEASES, REPO).expect("reads");
        assert_eq!(
            standing(Version::new(0, 2, 5), &releases),
            Standing::Behind(Version::new(0, 2, 6))
        );
        assert_eq!(standing(Version::new(0, 2, 6), &releases), Standing::Latest);
        assert_eq!(standing(Version::new(0, 2, 7), &releases), Standing::Ahead);
        assert_eq!(standing(Version::new(0, 2, 6), &[]), Standing::NoReleases);
    }

    const MANIFEST: &str = r#"{
        "schema": 2, "version": "0.2.6", "tag": "v0.2.6",
        "binaries": {
            "linux": {"x86_64": {"appimage": {"file": "Concat-0.2.6-linux-x86_64.AppImage", "url": "https://example.test/x.AppImage", "bytes": 1, "sha256": "5c75"},
                                 "deb": {"file": "../evil.deb", "url": "https://example.test/evil", "bytes": 1, "sha256": "00"}},
                      "aarch64": {"appimage": {"file": "Concat-0.2.6-linux-aarch64.AppImage", "url": "https://example.test/a.AppImage", "sha256": "7042"}}}
        }
    }"#;

    #[test]
    fn the_package_is_found_by_platform_architecture_and_kind() {
        let appimage = package_from_manifest(MANIFEST, "linux", "x86_64", PackageKind::AppImage)
            .expect("an AppImage");
        assert_eq!(appimage.url, "https://example.test/x.AppImage");
        // A size the manifest leaves out is zero, not a refusal.
        let arm = package_from_manifest(MANIFEST, "linux", "aarch64", PackageKind::AppImage)
            .expect("an arm build");
        assert_eq!(arm.bytes, 0);
        let missing = package_from_manifest(MANIFEST, "linux", "riscv64", PackageKind::AppImage)
            .expect_err("no riscv64 build in this manifest");
        assert!(
            missing.contains("no appimage package for linux riscv64"),
            "{missing}"
        );
        // A file name with a folder in it is not a file name.
        let evil = package_from_manifest(MANIFEST, "linux", "x86_64", PackageKind::Deb)
            .expect_err("refused");
        assert!(evil.contains("not a file name"), "{evil}");
    }

    #[test]
    fn the_manifest_names_the_architecture_as_it_is() {
        assert_eq!(manifest_arch("linux", "aarch64"), "aarch64");
        assert_eq!(manifest_arch("linux", "x86_64"), "x86_64");
    }

    #[test]
    fn a_linux_install_is_told_by_what_is_on_the_machine() {
        assert_eq!(
            linux_kind(true, false, false, false, false),
            Ok(PackageKind::AppImage)
        );
        assert_eq!(
            linux_kind(false, false, true, false, false),
            Ok(PackageKind::Deb)
        );
        assert_eq!(
            linux_kind(false, false, false, true, false),
            Ok(PackageKind::Pacman)
        );
        assert_eq!(
            linux_kind(false, false, false, false, true),
            Ok(PackageKind::Rpm)
        );
        // A Flatpak is a Flatpak whatever else the environment says.
        assert_eq!(
            linux_kind(true, true, true, false, false),
            Err(Fixed::Flatpak)
        );
        assert_eq!(
            linux_kind(false, false, false, false, false),
            Err(Fixed::NotFromPackage)
        );
        for kind in [
            PackageKind::AppImage,
            PackageKind::Deb,
            PackageKind::Rpm,
            PackageKind::Pacman,
        ] {
            assert!(!kind.key().is_empty());
        }
        assert_eq!(
            manifest_url(REPO, "v0.2.6"),
            "https://github.com/coreselfmastery/FUNCUT/releases/download/v0.2.6/manifest.json"
        );
        assert_eq!(
            manifest_url(UPSTREAM_REPO, "v0.2.6"),
            "https://github.com/jub0t/Concat/releases/download/v0.2.6/manifest.json"
        );
    }

    #[test]
    fn the_updates_folder_hangs_off_the_data_directory() {
        let dirs = AppDirs::under(Path::new("/tmp/concat-updates-test"));
        assert_eq!(folder(&dirs), Path::new("/tmp/concat-updates-test/updates"));
    }

    #[test]
    fn the_three_day_clock_reads_the_marker_not_the_process() {
        let now = std::time::SystemTime::now();
        let hours = |h: u64| std::time::Duration::from_secs(h * 60 * 60);
        // Never checked: due.
        assert!(check_due(None, now));
        // Just checked: not due, and neither is 71 hours after.
        assert!(!check_due(Some(now), now));
        assert!(!check_due(Some(now - hours(71)), now));
        // Three days to the second: due.
        assert!(check_due(Some(now - CHECK_INTERVAL), now));
        assert!(check_due(Some(now - hours(73)), now));
        // A clock set backwards left the marker in the future: due, not
        // stranded - a machine whose clock was wrong is a machine whose
        // marker is wrong too.
        assert!(check_due(Some(now + hours(1)), now));
    }

    #[test]
    fn the_marker_round_trips_and_a_garbage_one_reads_as_never() {
        let root = std::env::temp_dir().join("concat-update-marker-test");
        let _ = std::fs::remove_dir_all(&root);
        let dirs = AppDirs::under(&root);
        assert_eq!(last_checked(&dirs), None, "no marker yet");
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        record_checked(&dirs, when);
        // The marker holds whole seconds - a check's moment to the second
        // is the moment, and the nanoseconds are not worth the bytes.
        let round = |time: std::time::SystemTime| {
            time.duration_since(std::time::SystemTime::UNIX_EPOCH)
                .map(|since| since.as_secs())
                .unwrap_or(0)
        };
        assert_eq!(round(last_checked(&dirs).unwrap()), round(when));
        // A hand-edited or truncated marker is never, not a crash.
        std::fs::write(root.join("update-check"), "not a number").unwrap();
        assert_eq!(last_checked(&dirs), None);
        std::fs::write(root.join("update-check"), "").unwrap();
        assert_eq!(last_checked(&dirs), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_automatic_check_only_acts_on_a_package_it_could_replace() {
        let behind = |version: Version| Standing::Behind(version);
        // Not behind: nothing to do, whatever the install.
        for standing in [Standing::Latest, Standing::Ahead, Standing::NoReleases] {
            assert_eq!(auto_standing(standing, Ok(PackageKind::AppImage)), None);
            assert_eq!(auto_standing(standing, Err(Fixed::NotFromPackage)), None);
        }
        // Behind and replaceable: the newer version, whatever the package.
        let newer = Version::new(0, 3, 0);
        for kind in [
            Ok(PackageKind::AppImage),
            Ok(PackageKind::Deb),
            Ok(PackageKind::Rpm),
            Ok(PackageKind::Pacman),
        ] {
            assert_eq!(auto_standing(behind(newer), kind), Some(newer));
        }
        // Behind but fixed in place: a Flatpak updates itself, a build
        // from the tree is not this app's to replace - and neither is
        // ever told about a release, because there is nothing they could
        // do with the telling.
        assert_eq!(auto_standing(behind(newer), Err(Fixed::Flatpak)), None);
        assert_eq!(
            auto_standing(behind(newer), Err(Fixed::NotFromPackage)),
            None
        );
    }
}
