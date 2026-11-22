// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Live stock APIs: Pexels and Pixabay, the two rights-cleared shelves.
//!
//! The OpenVid library beside this one is a research dataset - wonderful
//! bytes, no per-clip licence, and `openvid_import` refuses a monetized
//! project because of it ([`docs/STOCK_LICENSING.md`]). This module is
//! the other half: the live APIs whose clips carry a licence that allows
//! commercial use without attribution (Pexels License; Pixabay Content
//! License - neither allows reselling the clips as stock, which the
//! refusal texts say out loud). A key each, in the environment or the
//! settings, and a slot that came up empty locally has a place left to
//! shop.
//!
//! Everything is one round trip a search and one a fetch, through the
//! machine's own proxy routing ([`crate::net`]) the way every other
//! shelf rides it. Fetched clips land in `<data>/stock/<provider>/` and
//! are moved into a project by the API's import, the way OpenVid's are.

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;
use url::Url;

use crate::dirs::AppDirs;

/// How long a search may take to connect and to read. The APIs answer in
/// well under a second when they are healthy; the generous bounds are for
/// a proxy in the way and a cold TLS handshake.
const CONNECT: Duration = Duration::from_secs(10);
const READ: Duration = Duration::from_secs(30);

/// The largest clip this module will fetch: the same bound OpenVid keeps,
/// a lie about Content-Length must not fill the disk.
const MAX_CLIP_BYTES: u64 = 512 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;

/// A live provider. The name is the one the API's own URLs and the
/// settings file spell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    /// The Pexels video API, key as the Authorization header.
    Pexels,
    /// The Pixabay video API, key as a query parameter.
    Pixabay,
}

impl Provider {
    /// The name as the API, the settings and the MCP tools spell it -
    /// lowercase, the way `openvid_search`'s `source` filter spells its
    /// own shelves.
    pub fn name(self) -> &'static str {
        match self {
            Provider::Pexels => "pexels",
            Provider::Pixabay => "pixabay",
        }
    }

    /// The environment variable the provider's key usually lives in.
    pub fn key_env(self) -> &'static str {
        match self {
            Provider::Pexels => "PEXELS_API_KEY",
            Provider::Pixabay => "PIXABAY_API_KEY",
        }
    }

    /// The key a caller handed in, or the environment's own. The
    /// settings file's copy arrives as the explicit argument; the
    /// environment is the fallback so a headless agent with an exported
    /// key works with nothing else configured.
    pub fn key(self, given: Option<&str>) -> String {
        given
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                std::env::var(self.key_env())
                    .ok()
                    .filter(|k| !k.trim().is_empty())
            })
            .unwrap_or_default()
    }

    /// The provider behind a name, spelled loosely.
    pub fn of(name: &str) -> Option<Provider> {
        match name.trim().to_ascii_lowercase().as_str() {
            "pexels" => Some(Provider::Pexels),
            "pixabay" => Some(Provider::Pixabay),
            _ => None,
        }
    }

    /// Validate a clip URL before contacting it. Every redirect is checked
    /// by the same rule, so a provider-host redirect cannot turn this fetch
    /// into a request to an unrelated or local service.
    pub fn validate_download_url(self, raw: &str) -> Result<Url, String> {
        let parsed =
            Url::parse(raw).map_err(|error| format!("the clip URL does not parse: {error}"))?;
        if parsed.scheme() != "https" {
            return Err("the clip URL must be https - the shelves serve no plain http".to_owned());
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(
                "the clip URL must not contain username or password credentials".to_owned(),
            );
        }
        if parsed.port().is_some_and(|port| port != 443) {
            return Err("the clip URL must use the standard https port".to_owned());
        }
        let host = parsed.host_str().unwrap_or_default();
        let allowed: &[&str] = match self {
            Provider::Pexels => &["videos.pexels.com", "www.pexels.com", "player.vimeo.com"],
            Provider::Pixabay => &["cdn.pixabay.com", "i.pixabay.com"],
        };
        if !allowed.contains(&host) {
            return Err(format!(
                "the clip URL's host {host} is not one of {}'s own ({}) - a fetch from anywhere else would stamp bytes this shelf never licensed",
                self.name(),
                allowed.join(", ")
            ));
        }
        Ok(parsed)
    }

    /// The search URL for words and filters, without the key - Pexels
    /// takes its key as a header, Pixabay as a query parameter, so the
    /// URL alone stays shareable for one of them.
    fn search_url(self, query: &str, per_page: u32, orientation: &str) -> String {
        match self {
            Provider::Pexels => {
                let mut url = format!("https://api.pexels.com/videos/search?per_page={per_page}");
                if !query.is_empty() {
                    url.push_str("&query=");
                    url.push_str(&urlencode(query));
                }
                if !orientation.is_empty() {
                    url.push_str("&orientation=");
                    url.push_str(orientation);
                }
                url
            }
            Provider::Pixabay => {
                let mut url =
                    format!("https://pixabay.com/api/videos/?per_page={per_page}&safesearch=true");
                if !query.is_empty() {
                    url.push_str("&q=");
                    url.push_str(&urlencode(query));
                }
                // Pixabay's video search has no square: anything an
                // orientation filter would say is horizontal or vertical.
                if orientation == "portrait" || orientation == "vertical" {
                    url.push_str("&orientation=vertical");
                } else if orientation == "landscape" || orientation == "horizontal" {
                    url.push_str("&orientation=horizontal");
                }
                url
            }
        }
    }
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn redirected_url(provider: Provider, current: &Url, location: &str) -> Result<Url, String> {
    let location = location.trim();
    if location.is_empty() {
        return Err(format!(
            "{} redirected the clip without a Location value",
            provider.name()
        ));
    }
    let next = current.join(location).map_err(|error| {
        format!(
            "{} returned an invalid redirect URL: {error}",
            provider.name()
        )
    })?;
    provider.validate_download_url(next.as_str())
}

/// Percent-encode the words of a query the way a URL's query string wants
/// them: spaces as `%20`, everything URL-safe kept as itself. The API
/// hosts both read the encoding either way; this form never sends a raw
/// space, which at least one proxy in the wild eats badly.
fn urlencode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

/// One clip as the live APIs answer it, normalized: whatever the
/// provider's own dialect said, the fields the editor wants are here.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// The provider's own id, as its fetch URL is spelled back.
    pub id: String,
    /// Which live shelf the clip is on.
    pub provider: Provider,
    /// What the clip shows, in the provider's own words: Pexels' `alt`,
    /// Pixabay's `tags`.
    pub caption: String,
    /// A name for the bin: the caption's first words, as a file name can
    /// carry them.
    pub name: String,
    /// How long the clip runs, seconds.
    pub seconds: f64,
    /// The chosen file's frame width.
    pub width: u32,
    /// The chosen file's frame height.
    pub height: u32,
    /// The chosen file's frame rate, when the provider said one.
    pub fps: Option<f64>,
    /// The bytes to download, when the provider's headers said - the
    /// page shows it the way OpenVid's card does, and the fetch refuses
    /// a lie about it past the cap.
    pub bytes: u64,
    /// The direct download URL of the file the sizes describe.
    pub url: String,
    /// The provider's thumbnail, for the card to show one day.
    pub preview: String,
    /// Who shot it, when the provider named them - the licence's one ask
    /// is that the creator is not passed off as someone else's.
    pub author: String,
    /// The licence the clip carries, spelled once in the answer.
    pub license: &'static str,
}

/// A page of live results.
#[derive(Clone, Debug, Default)]
pub struct Found {
    /// The page's clips, normalized.
    pub hits: Vec<Hit>,
    /// How many match in all, this page aside.
    pub total: u64,
}

/// The live libraries, one per process: one shelf for provider searches,
/// one no-auto-redirect shelf for validated clip downloads, and the app's
/// own folders for the fetched clips' cache.
pub struct Library {
    /// Where fetched clips are cached, `<data>/stock/<provider>/.
    dirs: AppDirs,
    /// The agents for the burst of a search page, the way OpenVid's
    /// reads share one.
    shelf: crate::net::Shelf,
    /// Redirects are disabled here so each provider hop can be checked first.
    fetch_shelf: crate::net::Shelf,
}

impl Library {
    /// The process's library. The folders are the first caller's - the
    /// app's own, in every process that matters.
    pub fn shared(dirs: &AppDirs) -> &'static Library {
        static LIBRARY: OnceLock<Library> = OnceLock::new();
        LIBRARY.get_or_init(|| Library {
            dirs: dirs.clone(),
            shelf: crate::net::Shelf::new("https://api.pexels.com/", CONNECT, READ),
            fetch_shelf: crate::net::Shelf::new_without_redirects(
                "https://api.pexels.com/",
                CONNECT,
                READ,
            ),
        })
    }

    /// Where a provider's clips are cached.
    fn folder(&self, provider: Provider) -> std::path::PathBuf {
        self.dirs.data.join("stock").join(provider.name())
    }

    /// The path of a fetched clip, when it is on this machine.
    pub fn fetched(&self, provider: Provider, id: &str) -> Option<std::path::PathBuf> {
        let path = self.folder(provider).join(format!("{}.mp4", safe_stem(id)));
        path.is_file().then_some(path)
    }

    /// One provider's standing: whether a key is in reach and what the
    /// licence allows, the words the settings page and the MCP tool's
    /// status say.
    pub fn provider_status(&self, provider: Provider, key: Option<&str>) -> ProviderStatus {
        let configured = !provider.key(key).is_empty();
        ProviderStatus {
            provider: provider.name(),
            configured,
            key_env: provider.key_env(),
            license: license_of(provider),
        }
    }

    /// Searches one live shelf. The key is the one the settings hold or
    /// the environment's own; without one the refusal is a named error,
    /// not a surprise. `page` is the provider's own, 1-based.
    pub fn search(
        &self,
        provider: Provider,
        query: &str,
        per_page: u32,
        page: u32,
        orientation: Option<&str>,
        key: Option<&str>,
    ) -> Result<Found, String> {
        let key = provider.key(key);
        if key.is_empty() {
            return Err(format!(
                "no {} key: set one in Settings > Stock APIs or export {}",
                provider.name(),
                provider.key_env()
            ));
        }
        let per_page = per_page.clamp(1, 40);
        let orientation = orientation
            .map(|name| match name {
                "vertical" | "portrait" => "portrait",
                "horizontal" | "landscape" => "landscape",
                "square" => "square",
                _ => "",
            })
            .unwrap_or("");
        // The key rides the request the way the provider's own dialect
        // takes it: Pexels as the Authorization header's whole value - no
        // Bearer prefix - and Pixabay as a query parameter on every call.
        let send = |request: ureq::Request| match provider {
            Provider::Pexels => request.set("Authorization", &key),
            // The URL was built with the key already in it.
            Provider::Pixabay => request,
        };
        let url = match provider {
            Provider::Pexels => provider.search_url(query, per_page, orientation),
            Provider::Pixabay => {
                let mut url = provider.search_url(query, per_page, orientation);
                url.push_str(if url.contains('?') { "&" } else { "?" });
                url.push_str("key=");
                url.push_str(&urlencode(&key));
                url
            }
        };
        let url = if page > 1 {
            format!("{url}&page={page}")
        } else {
            url
        };
        let response = self.shelf.get(&url, &send).map_err(|failure| {
            // The full URL carries the Pixabay key, and ureq prints the
            // URL in a status error - the secret went to the caller, the
            // log and the receipt. Print only what happened.
            let status = match &failure.cause {
                crate::net::Cause::Status(code) => Some(*code),
                _ => None,
            };
            let hint = match status {
                Some(401) | Some(403) => " - the key was refused",
                Some(404) => " - the endpoint moved",
                Some(429) => " - the rate limit for this key is spent; a minute's wait",
                Some(500..=599) => " - the provider is having a moment; retry",
                _ => "",
            };
            match status {
                Some(code) => format!(
                    "{} search failed: the server answered HTTP {code}{hint}",
                    provider.name()
                ),
                None => format!("{} search failed: {failure}", provider.name()),
            }
        })?;
        if response.status() != 200 {
            let status = response.status();
            let body = response.into_string().unwrap_or_default();
            let hint = match status {
                401 | 403 => " - the key was refused",
                404 => " - the endpoint moved",
                429 => " - the rate limit for this key is spent; a minute's wait",
                _ => "",
            };
            return Err(format!(
                "{} answered {status}{hint}: {}",
                provider.name(),
                body.chars().take(300).collect::<String>()
            ));
        }
        let body = response
            .into_string()
            .map_err(|error| format!("{}'s answer could not be read: {error}", provider.name()))?;
        let json: Value = serde_json::from_str(&body)
            .map_err(|error| format!("{}'s answer was not JSON: {error}", provider.name()))?;
        let found = match provider {
            Provider::Pexels => pexels_found(&json),
            Provider::Pixabay => pixabay_found(&json),
        };
        Ok(found)
    }

    fn fetch_response(&self, provider: Provider, mut url: Url) -> Result<ureq::Response, String> {
        let send = |request: ureq::Request| request;
        for followed in 0..=MAX_REDIRECTS {
            let response = match self.fetch_shelf.get(url.as_str(), &send) {
                Ok(response) => response,
                Err(failure) => {
                    let redirect = matches!(
                        &failure.error,
                        ureq::Error::Status(code, _) if is_redirect(*code)
                    );
                    if redirect {
                        match failure.error {
                            ureq::Error::Status(_, response) => response,
                            _ => unreachable!("the status was checked above"),
                        }
                    } else {
                        let status = match failure.cause {
                            crate::net::Cause::Status(code) => Some(code),
                            _ => None,
                        };
                        let hint = match status {
                            Some(401) | Some(403) => " - the key was refused",
                            Some(404) => " - the clip moved",
                            Some(429) => " - the rate limit for this key is spent; a minute's wait",
                            Some(500..=599) => " - the provider is having a moment; retry",
                            _ => "",
                        };
                        return Err(match status {
                            Some(code) => format!(
                                "{} fetch failed: the server answered HTTP {code}{hint}",
                                provider.name()
                            ),
                            None => format!("{} fetch failed: {failure}", provider.name()),
                        });
                    }
                }
            };
            if !is_redirect(response.status()) {
                return Ok(response);
            }
            if followed == MAX_REDIRECTS {
                return Err(format!(
                    "{} sent more than {MAX_REDIRECTS} redirects",
                    provider.name()
                ));
            }
            let location = response
                .header("Location")
                .map(str::to_owned)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| {
                    format!(
                        "{} redirected the clip without a Location header",
                        provider.name()
                    )
                })?;
            url = redirected_url(provider, &url, &location)?;
        }
        Err(format!("{} sent too many redirects", provider.name()))
    }

    /// Fetches one clip from its own URL - the search page's hit carries
    /// the exact file it described, and this is the one door every caller
    /// (the page, the API, the MCP tool) uses.
    pub fn fetch_url(
        &self,
        provider: Provider,
        id: &str,
        url: &str,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<std::path::PathBuf, String> {
        let url = provider.validate_download_url(url)?;
        if let Some(path) = self.fetched(provider, id) {
            return Ok(path);
        }
        let folder = self.folder(provider);
        std::fs::create_dir_all(&folder)
            .map_err(|error| format!("{}'s cache could not be made: {error}", provider.name()))?;
        let target = folder.join(format!("{}.mp4", safe_stem(id)));
        let partial = target.with_extension("mp4.part");
        // Provider hosts may redirect to their CDN. The fetch shelf returns
        // each 3xx without following it; fetch_response validates every
        // Location before making the next request.
        let response = self.fetch_response(provider, url)?;
        if response.status() != 200 {
            let status = response.status();
            let body = response.into_string().unwrap_or_default();
            return Err(format!(
                "{} answered {status} for the clip: {}",
                provider.name(),
                body.chars().take(300).collect::<String>()
            ));
        }
        let declared = response
            .header("Content-Length")
            .and_then(|text| text.parse::<u64>().ok());
        if let Some(bytes) = declared.filter(|bytes| *bytes > MAX_CLIP_BYTES) {
            return Err(format!(
                "the clip says it is {} MB, past the {} MB cap",
                bytes / 1_000_000,
                MAX_CLIP_BYTES / 1_000_000
            ));
        }
        let mut file = std::fs::File::create(&partial)
            .map_err(|error| format!("the clip's part file could not be made: {error}"))?;
        let mut written = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        use std::io::Read as _;
        use std::io::Write as _;
        let mut reader = response.into_reader();
        loop {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = std::fs::remove_file(&partial);
                return Err("cancelled".to_owned());
            }
            let read = reader
                .read(&mut buffer)
                .map_err(|error| format!("the clip's bytes stopped: {error}"))?;
            if read == 0 {
                break;
            }
            written += read as u64;
            if written > MAX_CLIP_BYTES {
                let _ = std::fs::remove_file(&partial);
                return Err(format!(
                    "the clip passed the {} MB cap mid-answer",
                    MAX_CLIP_BYTES / 1_000_000
                ));
            }
            file.write_all(&buffer[..read])
                .map_err(|error| format!("the clip's bytes could not be written: {error}"))?;
        }
        if written == 0 {
            let _ = std::fs::remove_file(&partial);
            return Err(format!("{} sent an empty clip", provider.name()));
        }
        // A body that ends "cleanly" short of its own Content-Length is a
        // truncated download (a proxy cut, a dropped connection): the old
        // code renamed it into the cache anyway, and the cache serves by
        // existence - a head-mangled file was served forever.
        if let Some(bytes) = declared.filter(|bytes| *bytes > 0 && written != *bytes) {
            let _ = std::fs::remove_file(&partial);
            return Err(format!(
                "the clip stopped at {written} of its {bytes} bytes: a truncated download, not a clip"
            ));
        }
        std::fs::rename(&partial, &target)
            .map_err(|error| format!("the clip could not be landed: {error}"))?;
        Ok(target)
    }
}

/// One provider's standing, as the settings page and the status tool say
/// it.
#[derive(Clone, Debug)]
pub struct ProviderStatus {
    /// The provider's name, as the settings and the tools spell it.
    pub provider: &'static str,
    /// Whether a key is in reach.
    pub configured: bool,
    /// The environment variable the key usually lives in.
    pub key_env: &'static str,
    /// What the clips' licence allows.
    pub license: &'static str,
}

/// The licence words each provider's clips carry. Neither allows reselling
/// the clips themselves as stock; both allow commercial use in a made
/// work. Pexels asks that the clips not be passed off as someone else's;
/// Pixabay's own licence page is the fine print.
pub fn license_of(provider: Provider) -> &'static str {
    match provider {
        Provider::Pexels => {
            "Pexels License: free for commercial use, no attribution required; do not redistribute the clips as stock"
        }
        Provider::Pixabay => {
            "Pixabay Content License: free for commercial use, no attribution required; do not redistribute the clips as stock"
        }
    }
}

/// A file-name stem out of a provider id, whatever the id's own dialect
/// allowed: digits and letters stay, everything else is an underscore,
/// and the empty result - which cannot name a file - is refused upstream.
pub fn safe_stem(id: &str) -> String {
    id.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

/// Reads a Pexels video search page into the normalized [`Found`]. Pure,
/// so a recorded page tests the field mapping without a network.
fn pexels_found(json: &Value) -> Found {
    let videos = json
        .get("videos")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = json
        .get("total_results")
        .and_then(Value::as_u64)
        .unwrap_or(videos.len() as u64);
    Found {
        hits: videos.iter().filter_map(pexels_hit).collect(),
        total,
    }
}

/// One Pexels video row: the `alt` says what it shows, `video_files`
/// says which files exist, and the best one for the editor is the
/// largest that is still at most 4K - the API's own order is not a
/// promise, so the choice is made here.
fn pexels_hit(video: &Value) -> Option<Hit> {
    let id = video.get("id").and_then(Value::as_i64)?;
    let alt = video
        .get("alt")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let duration = video.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
    let author = video
        .get("user")
        .and_then(|user| user.get("name"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let preview = video
        .get("image")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let files = video.get("video_files").and_then(Value::as_array)?;
    // The largest file at or under 4K, mp4, is the one to keep; ties go
    // to the first, which is the API's own idea of the best.
    let mut best: Option<(&Value, u64)> = None;
    for file in files {
        let kind = file.get("file_type").and_then(Value::as_str).unwrap_or("");
        if kind != "video/mp4" {
            continue;
        }
        let width = file.get("width").and_then(Value::as_u64).unwrap_or(0);
        let height = file.get("height").and_then(Value::as_u64).unwrap_or(0);
        let pixels = width * height;
        if pixels > 4096 * 2160 {
            continue;
        }
        let better = best.is_none_or(|(_, held)| pixels > held);
        if better {
            best = Some((file, pixels));
        }
    }
    let (file, _) = best?;
    let url = file.get("link").and_then(Value::as_str)?.to_owned();
    let width = file.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
    let height = file.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
    let fps = file
        .get("fps")
        .and_then(Value::as_f64)
        .filter(|fps| *fps > 0.0);
    let name = name_of(&alt);
    Some(Hit {
        id: id.to_string(),
        provider: Provider::Pexels,
        caption: alt,
        name,
        seconds: duration,
        width,
        height,
        fps,
        bytes: 0,
        url,
        preview,
        author,
        license: license_of(Provider::Pexels),
    })
}

/// Reads a Pixabay video search page. The provider's own shape:
/// `hits[].videos.{large,medium,small,tiny}`, each a `{url, width,
/// height}` - and the large is the one to keep when it exists.
fn pixabay_found(json: &Value) -> Found {
    let hits = json
        .get("hits")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = json
        .get("totalHits")
        .and_then(Value::as_u64)
        .unwrap_or(hits.len() as u64);
    Found {
        hits: hits.iter().filter_map(pixabay_hit).collect(),
        total,
    }
}

/// One Pixabay hit: `tags` is what it shows, `videos` is the ladder of
/// files, and the largest rung is the one the editor wants.
fn pixabay_hit(hit: &Value) -> Option<Hit> {
    let id = hit.get("id").and_then(Value::as_i64)?;
    let tags = hit
        .get("tags")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let duration = hit.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
    let picture = hit
        .get("picture_id")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let preview = if picture.is_empty() {
        String::new()
    } else {
        format!("https://i.pixabay.com/{picture}/960x540.jpg")
    };
    let videos = hit.get("videos")?.as_object()?;
    // The ladder, largest first: large, then medium, then small, then
    // tiny. The first that exists is the one to keep.
    for rung in ["large", "medium", "small", "tiny"] {
        if let Some(file) = videos.get(rung) {
            let url = file.get("url").and_then(Value::as_str)?;
            if url.is_empty() {
                continue;
            }
            let width = file.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
            let height = file.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
            let name = name_of(&tags);
            return Some(Hit {
                id: id.to_string(),
                provider: Provider::Pixabay,
                caption: tags.clone(),
                name,
                seconds: duration,
                width,
                height,
                fps: None,
                bytes: 0,
                url: url.to_owned(),
                preview,
                author: String::new(),
                license: license_of(Provider::Pixabay),
            });
        }
    }
    None
}

/// A file-name out of a caption's first words: the words a bin entry can
/// carry, ASCII-friendlied, capped at a name's own length.
fn name_of(caption: &str) -> String {
    let words: String = caption
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        // A word with its own punctuation - "shore," or "rocky's" - must
        // not stack a dash on the join's own: one separator is a name,
        // a run of them is not.
        .fold(String::new(), |mut out, ch| {
            if ch == '-' && out.ends_with('-') {
                out
            } else {
                out.push(ch);
                out
            }
        });
    let trimmed = words.trim_matches('-');
    if trimmed.is_empty() {
        "clip".to_owned()
    } else {
        trimmed.chars().take(48).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_providers_name_their_keys_and_their_licence() {
        assert_eq!(Provider::Pexels.name(), "pexels");
        assert_eq!(Provider::Pixabay.key_env(), "PIXABAY_API_KEY");
        assert_eq!(Provider::of("Pexels "), Some(Provider::Pexels));
        assert_eq!(Provider::of("PIXABAY"), Some(Provider::Pixabay));
        assert_eq!(Provider::of("openvid"), None);
        assert!(license_of(Provider::Pexels).contains("no attribution"));
        assert!(license_of(Provider::Pixabay).contains("do not redistribute"));
    }

    #[test]
    fn live_download_urls_are_exact_https_provider_hosts() {
        let pexels = Provider::Pexels
            .validate_download_url("https://videos.pexels.com/video-files/1.mp4")
            .expect("the Pexels CDN");
        assert_eq!(pexels.host_str(), Some("videos.pexels.com"));
        assert!(
            Provider::Pexels
                .validate_download_url("http://videos.pexels.com/video-files/1.mp4")
                .is_err()
        );
        assert!(
            Provider::Pexels
                .validate_download_url("https://videos.pexels.com.evil.test/file.mp4")
                .is_err()
        );
        assert!(
            Provider::Pexels
                .validate_download_url("https://user@videos.pexels.com/file.mp4")
                .is_err()
        );
        assert!(
            Provider::Pexels
                .validate_download_url("https://videos.pexels.com:8443/file.mp4")
                .is_err()
        );
        assert!(
            Provider::Pixabay
                .validate_download_url("https://videos.pexels.com/file.mp4")
                .is_err()
        );
        assert!(
            Provider::Pixabay
                .validate_download_url("https://cdn.pixabay.com/video/1.mp4")
                .is_ok()
        );
    }

    #[test]
    fn every_redirect_target_is_checked_before_the_next_request() {
        let start = Provider::Pexels
            .validate_download_url("https://www.pexels.com/video/1")
            .expect("the provider host");
        let cdn = redirected_url(
            Provider::Pexels,
            &start,
            "https://videos.pexels.com/video-files/1.mp4",
        )
        .expect("the provider CDN");
        assert_eq!(cdn.host_str(), Some("videos.pexels.com"));
        assert!(
            redirected_url(
                Provider::Pexels,
                &start,
                "https://127.0.0.1/latest/meta-data"
            )
            .is_err()
        );
        assert!(
            redirected_url(
                Provider::Pexels,
                &start,
                "http://videos.pexels.com/file.mp4"
            )
            .is_err()
        );
        assert!(redirected_url(Provider::Pexels, &start, "https://evil.example/file.mp4").is_err());
    }

    #[test]
    fn the_query_encoding_keeps_the_safe_and_encodes_the_rest() {
        assert_eq!(urlencode("ocean sunset"), "ocean%20sunset");
        assert_eq!(urlencode("a&b=c?"), "a%26b%3Dc%3F");
        assert_eq!(urlencode("safe-words_2.0"), "safe-words_2.0");
    }

    #[test]
    fn a_pexels_page_reads_into_normalized_hits() {
        let page = json!({
            "total_results": 2,
            "videos": [
                {
                    "id": 2499611,
                    "duration": 13.0,
                    "alt": "Waves rolling onto a sandy shore at sunset",
                    "image": "https://images.pexels.com/videos/2499611/pictures/preview-0.jpg",
                    "user": {"name": "Some One"},
                    "video_files": [
                        {"id": 1, "quality": "uhd", "file_type": "video/mp4", "width": 7680, "height": 4320, "fps": 30.0, "link": "https://videos.pexels.com/files/uhd.mp4"},
                        {"id": 2, "quality": "hd", "file_type": "video/mp4", "width": 2560, "height": 1440, "fps": 30.0, "link": "https://videos.pexels.com/files/hd.mp4"},
                        {"id": 3, "quality": "sd", "file_type": "video/mp4", "width": 960, "height": 540, "fps": 25.0, "link": "https://videos.pexels.com/files/sd.mp4"}
                    ]
                },
                {"id": 857195, "duration": 8.0, "alt": "", "video_files": [
                    {"id": 4, "quality": "sd", "file_type": "video/mp4", "width": 960, "height": 540, "fps": 24.0, "link": "https://videos.pexels.com/files/other.mp4"}
                ]}
            ]
        });
        let found = pexels_found(&page);
        assert_eq!(found.total, 2);
        assert_eq!(found.hits.len(), 2);
        let first = &found.hits[0];
        assert_eq!(first.id, "2499611");
        // The 8K file is past the 4K ceiling, so the 1440p one is the
        // best kept.
        assert_eq!(first.width, 2560);
        assert_eq!(first.height, 1440);
        assert_eq!(first.url, "https://videos.pexels.com/files/hd.mp4");
        assert_eq!(first.fps, Some(30.0));
        assert_eq!(first.author, "Some One");
        assert_eq!(first.name, "Waves-rolling-onto-a");
        let second = &found.hits[1];
        // An empty alt is a clip named clip, not a hole in the page.
        assert_eq!(second.name, "clip");
        assert_eq!(second.seconds, 8.0);
    }

    #[test]
    fn a_pixabay_page_walks_the_ladder_to_large() {
        let page = json!({
            "total": 100, "totalHits": 7,
            "hits": [
                {
                    "id": 5,
                    "tags": "ocean, waves, sunset",
                    "duration": 12,
                    "picture_id": "abc-123",
                    "videos": {
                        "large": {"url": "https://cdn.pixabay.com/video/2023/large.mp4", "width": 1920, "height": 1080, "duration": 12},
                        "medium": {"url": "https://cdn.pixabay.com/video/2023/medium.mp4", "width": 1280, "height": 720, "duration": 12},
                        "tiny": {"url": "https://cdn.pixabay.com/video/2023/tiny.mp4", "width": 640, "height": 360, "duration": 12}
                    }
                },
                {"id": 6, "tags": "forest", "duration": 4, "videos": {"tiny": {"url": "https://cdn.pixabay.com/t.mp4", "width": 480, "height": 360}}}
            ]
        });
        let found = pixabay_found(&page);
        assert_eq!(found.total, 7);
        assert_eq!(found.hits.len(), 2);
        assert_eq!(
            found.hits[0].url,
            "https://cdn.pixabay.com/video/2023/large.mp4"
        );
        assert_eq!(found.hits[0].width, 1920);
        assert_eq!(found.hits[0].caption, "ocean, waves, sunset");
        assert_eq!(
            found.hits[0].preview,
            "https://i.pixabay.com/abc-123/960x540.jpg"
        );
        assert_eq!(found.hits[1].url, "https://cdn.pixabay.com/t.mp4");
        assert_eq!(found.hits[1].seconds, 4.0);
    }

    #[test]
    fn a_page_without_the_fields_is_empty_not_wrong() {
        assert!(pexels_found(&json!({})).hits.is_empty());
        assert!(pixabay_found(&json!({})).hits.is_empty());
        assert!(
            pexels_found(&json!({"videos": [{"id": 5}]}))
                .hits
                .is_empty()
        );
    }

    #[test]
    fn stems_and_names_are_file_safe() {
        assert_eq!(safe_stem("857195"), "857195");
        assert_eq!(safe_stem("a/b c"), "a_b_c");
        assert_eq!(name_of("   "), "clip");
        assert_eq!(
            name_of("Waves!!! rolling onto a sandy shore"),
            "Waves-rolling-onto-a"
        );
    }

    #[test]
    fn the_search_urls_carry_the_filters() {
        let url = Provider::Pexels.search_url("ocean waves", 20, "portrait");
        assert!(url.contains("per_page=20"));
        assert!(url.contains("query=ocean%20waves"));
        assert!(url.contains("orientation=portrait"));
        let url = Provider::Pixabay.search_url("ocean", 20, "vertical");
        assert!(url.contains("safesearch=true"));
        assert!(url.contains("q=ocean"));
        assert!(url.contains("orientation=vertical"));
        // Pixabay's video search has no square; the shape is left off.
        let url = Provider::Pixabay.search_url("", 20, "square");
        assert!(!url.contains("orientation="));
    }
}
