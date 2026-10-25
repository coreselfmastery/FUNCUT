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
}

impl Version {
    /// The version with these three numbers.
    pub const fn new(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
        }
    }

    /// `0.2.5` or `v0.2.5`. Anything else - a pre-release such as
    /// `v0.3.0-beta.1`, the `nightly` tag, two numbers - is `None`: not a
    /// release a person is offered.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version::new(major, minor, patch))
    }

    /// The version this binary was built as: the workspace's.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version::new(0, 0, 0))
    }

    /// The release's tag: `v0.2.5`.
    pub fn tag(self) -> String {
        format!("v{self}")
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
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
/// own are preferred, and only a fork that has published none of them -
/// or whose list would not answer - takes upstream Concat's, the way
/// `install-linux.sh` falls back at the command line.
pub fn fetch_releases() -> Result<Vec<Release>, String> {
    let own = get_text(&releases_url(REPO)).and_then(|json| releases_from_json(&json, REPO));
    match own {
        Ok(releases) if !releases.is_empty() => Ok(releases),
        _ => releases_from_json(&get_text(&releases_url(UPSTREAM_REPO))?, UPSTREAM_REPO),
    }
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

/// One GET, read whole as text. GitHub wants a user agent and says so with
/// a 403 otherwise.
fn get_text(url: &str) -> Result<String, String> {
    let agent = crate::net::agent(
        std::time::Duration::from_secs(20),
        std::time::Duration::from_secs(30),
    );
    let response = agent
        .get(url)
        .set("User-Agent", &format!("Concat/{}", Version::current()))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(code, response) => {
                let body = response.into_string().unwrap_or_default();
                let said = serde_json::from_str::<serde_json::Value>(&body)
                    .map(|value| github_message(&value))
                    .unwrap_or_else(|_| body.chars().take(200).collect());
                format!("{url} answered {code}: {said}")
            }
            other => format!("{url} did not answer: {other}"),
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
}
