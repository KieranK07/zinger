pub mod parser;

use super::{MarketAdapter, SearchContext};
use crate::models::{AdapterError, AdapterStatus, RateLimitPolicy, RawListing, SearchSpec};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Two consecutive challenges => stop polling FB for a day. A challenge is a
/// red flag for the account, so we err hard toward backing off.
const CHALLENGE_DISABLE_HOURS: i64 = 24;
const CHALLENGES_BEFORE_DISABLE: u32 = 2;

/// Persisted + runtime state for the FB adapter, shared (Arc) between the
/// registered adapter and the Tauri commands that drive login/install/enable.
#[derive(Debug, Default)]
pub struct FbState {
    // Persisted to the settings table (mirrored here for fast, lock-local reads).
    pub enabled: bool,
    pub tos_accepted: bool,
    pub logged_in: bool,
    // Runtime-only.
    pub chromium_installed: bool,
    pub session_expired: bool,
    pub consecutive_challenges: u32,
    pub disabled_until: Option<DateTime<Utc>>,
    pub download_pct: Option<u8>,
}

pub struct FbShared {
    pub state: Mutex<FbState>,
    pub profile_dir: PathBuf,
    pub sidecar_dir: PathBuf,
    pub max_pages: Mutex<u32>,
}

impl FbShared {
    pub fn new(profile_dir: PathBuf, sidecar_dir: PathBuf) -> Self {
        FbShared {
            state: Mutex::new(FbState::default()),
            profile_dir,
            sidecar_dir,
            max_pages: Mutex::new(3),
        }
    }

    fn script(&self) -> PathBuf {
        self.sidecar_dir.join("fb-sidecar.mjs")
    }

    /// Build the base sidecar command (node <script> <subcommand>). Returns
    /// None if the sidecar script isn't present — callers degrade gracefully.
    fn command(&self, subcommand: &str) -> Option<Command> {
        let script = self.script();
        if !script.exists() {
            return None;
        }
        let mut cmd = Command::new("node");
        cmd.arg(script).arg(subcommand);
        cmd.stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true);
        Some(cmd)
    }
}

/// Events the sidecar emits (NDJSON). Mirrors fb-sidecar.mjs.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SidecarEvent {
    Status { chromium_installed: bool },
    Progress { pct: u8 },
    Installed,
    LoginStarted,
    LoggedIn,
    LoginTimeout,
    Page { html: String },
    Done,
    NotLoggedIn,
    Challenge,
    Error { message: String },
}

/// What kind of download/login progress to surface to the UI.
#[derive(Debug, Clone, Serialize)]
pub struct FbProgress {
    pub kind: String, // "download" | "login" | "done" | "error"
    pub pct: Option<u8>,
    pub message: Option<String>,
}

pub struct FacebookAdapter {
    shared: Arc<FbShared>,
}

impl FacebookAdapter {
    pub fn new(shared: Arc<FbShared>) -> Self {
        FacebookAdapter { shared }
    }

    /// True only when a poll cycle may actually drive the browser. Crucially
    /// checked BEFORE any process is spawned, so a disabled adapter never
    /// loads the sidecar/Chromium (the isolation guarantee).
    fn runnable(state: &FbState) -> bool {
        state.enabled
            && state.tos_accepted
            && state.chromium_installed
            && state.logged_in
            && !state.session_expired
            && state.disabled_until.map(|t| t <= Utc::now()).unwrap_or(true)
    }
}

#[async_trait]
impl MarketAdapter for FacebookAdapter {
    fn id(&self) -> &'static str {
        "facebook"
    }

    fn display_name(&self) -> &'static str {
        "Facebook Marketplace"
    }

    async fn search(
        &self,
        spec: &SearchSpec,
        _ctx: &SearchContext,
    ) -> Result<Vec<RawListing>, AdapterError> {
        // Snapshot the state and decide WITHOUT spawning anything.
        let (runnable, expired, max_pages) = {
            let state = self.shared.state.lock().unwrap();
            (Self::runnable(&state), state.session_expired, *self.shared.max_pages.lock().unwrap())
        };

        if expired {
            // Never a silent empty on expiry — surface it so health and the UI
            // can prompt re-login.
            return Err(AdapterError::Auth("Facebook session expired — re-login required".into()));
        }
        if !runnable {
            // Off / no ToS / no Chromium / 24h-disabled: contribute nothing,
            // spawn nothing. health() explains why.
            return Ok(Vec::new());
        }

        let Some(mut cmd) = self.shared.command("search") else {
            return Err(AdapterError::Disabled("FB sidecar not installed".into()));
        };
        cmd.arg("--profile")
            .arg(&self.shared.profile_dir)
            .arg("--query")
            .arg(&spec.keywords)
            .arg("--max-pages")
            .arg(max_pages.to_string())
            .arg("--min-delay")
            .arg("20")
            .arg("--max-delay")
            .arg("45");

        let mut child = cmd
            .spawn()
            .map_err(|e| AdapterError::Network(format!("spawn sidecar: {e}")))?;
        let stdout = child.stdout.take().expect("piped stdout");
        let mut lines = BufReader::new(stdout).lines();

        let mut listings = Vec::new();
        let mut outcome: Result<(), AdapterError> = Ok(());
        while let Ok(Some(line)) = lines.next_line().await {
            match serde_json::from_str::<SidecarEvent>(&line) {
                Ok(SidecarEvent::Page { html }) => {
                    if let Ok(mut parsed) = parser::parse_search_page(&html) {
                        listings.append(&mut parsed);
                    }
                }
                Ok(SidecarEvent::Done) => break,
                Ok(SidecarEvent::Challenge) => {
                    outcome = Err(AdapterError::Blocked("Facebook challenge/checkpoint".into()));
                    break;
                }
                Ok(SidecarEvent::NotLoggedIn) => {
                    outcome = Err(AdapterError::Auth("Facebook session expired".into()));
                    break;
                }
                Ok(SidecarEvent::Error { message }) => {
                    outcome = Err(AdapterError::Network(format!("sidecar: {message}")));
                    break;
                }
                _ => {}
            }
        }
        let _ = child.wait().await;

        // Update FB-specific health bookkeeping from the outcome.
        {
            let mut state = self.shared.state.lock().unwrap();
            match &outcome {
                Err(AdapterError::Blocked(_)) => {
                    state.consecutive_challenges += 1;
                    if state.consecutive_challenges >= CHALLENGES_BEFORE_DISABLE {
                        state.disabled_until =
                            Some(Utc::now() + Duration::hours(CHALLENGE_DISABLE_HOURS));
                        state.consecutive_challenges = 0;
                    }
                }
                Err(AdapterError::Auth(_)) => {
                    state.session_expired = true;
                    state.logged_in = false;
                }
                _ => {
                    state.consecutive_challenges = 0;
                }
            }
        }

        outcome?;
        // De-dup across scroll pages (the same card recurs as you scroll).
        let mut seen = std::collections::HashSet::new();
        listings.retain(|l| seen.insert(l.source_id.clone()));
        Ok(listings)
    }

    fn health(&self) -> AdapterStatus {
        let state = self.shared.state.lock().unwrap();
        if !state.enabled {
            return AdapterStatus::Disabled { reason: "off — enable in Settings".into() };
        }
        if !state.tos_accepted {
            return AdapterStatus::NotConfigured {
                reason: "accept the Facebook terms warning to enable".into(),
            };
        }
        if !state.chromium_installed {
            let pct = state.download_pct;
            return AdapterStatus::NotConfigured {
                reason: match pct {
                    Some(p) => format!("downloading Chromium… {p}%"),
                    None => "Chromium download required (~341 MB)".into(),
                },
            };
        }
        if let Some(until) = state.disabled_until {
            if until > Utc::now() {
                return AdapterStatus::Disabled {
                    reason: format!("auto-disabled after repeated challenges until {}", until.to_rfc3339()),
                };
            }
        }
        if state.session_expired || !state.logged_in {
            return AdapterStatus::Degraded { reason: "re-login required".into() };
        }
        AdapterStatus::Ok
    }

    fn rate_limit_policy(&self) -> RateLimitPolicy {
        RateLimitPolicy {
            min_delay_ms: 20_000,
            max_delay_ms: 45_000,
            max_requests_per_cycle: 3,
        }
    }
}

// ---- Sidecar drivers used by Tauri commands (not the poll path) ----

/// Download Chromium via the sidecar's `install`, reporting progress through
/// `on_progress`. Updates `chromium_installed` on success.
pub async fn install_chromium<F>(shared: Arc<FbShared>, on_progress: F) -> Result<(), String>
where
    F: Fn(FbProgress) + Send + 'static,
{
    let Some(mut cmd) = shared.command("install") else {
        return Err("FB sidecar script not found".into());
    };
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();

    let mut installed = false;
    while let Ok(Some(line)) = lines.next_line().await {
        match serde_json::from_str::<SidecarEvent>(&line) {
            Ok(SidecarEvent::Progress { pct }) => {
                shared.state.lock().unwrap().download_pct = Some(pct);
                on_progress(FbProgress { kind: "download".into(), pct: Some(pct), message: None });
            }
            Ok(SidecarEvent::Installed) => {
                installed = true;
                break;
            }
            Ok(SidecarEvent::Error { message }) => {
                let _ = child.wait().await;
                on_progress(FbProgress {
                    kind: "error".into(),
                    pct: None,
                    message: Some(message.clone()),
                });
                return Err(message);
            }
            _ => {}
        }
    }
    let _ = child.wait().await;
    {
        let mut s = shared.state.lock().unwrap();
        s.chromium_installed = installed;
        s.download_pct = None;
    }
    if installed {
        on_progress(FbProgress { kind: "done".into(), pct: Some(100), message: None });
        Ok(())
    } else {
        Err("download did not complete".into())
    }
}

/// Open a visible browser for manual login. Resolves "logged_in" or an error.
/// On success, clears the expired flag and sets logged_in.
pub async fn login(shared: Arc<FbShared>) -> Result<String, String> {
    let Some(mut cmd) = shared.command("login") else {
        return Err("FB sidecar script not found".into());
    };
    cmd.arg("--profile").arg(&shared.profile_dir);
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();

    let mut result = Err("login window closed before sign-in".to_string());
    while let Ok(Some(line)) = lines.next_line().await {
        match serde_json::from_str::<SidecarEvent>(&line) {
            Ok(SidecarEvent::LoggedIn) => {
                let mut s = shared.state.lock().unwrap();
                s.logged_in = true;
                s.session_expired = false;
                s.consecutive_challenges = 0;
                result = Ok("logged_in".to_string());
                break;
            }
            Ok(SidecarEvent::LoginTimeout) => {
                result = Err("login timed out — try again".to_string());
                break;
            }
            Ok(SidecarEvent::Error { message }) => {
                result = Err(message);
                break;
            }
            _ => {}
        }
    }
    let _ = child.wait().await;
    result
}

/// Run `status` and update chromium_installed. Spawns the sidecar briefly;
/// only called from explicit UI actions, never the poll cycle.
pub async fn refresh_chromium_status(shared: &FbShared) -> Result<bool, String> {
    let Some(mut cmd) = shared.command("status") else {
        return Err("FB sidecar script not found".into());
    };
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).lines();
    let mut installed = false;
    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(SidecarEvent::Status { chromium_installed }) =
            serde_json::from_str::<SidecarEvent>(&line)
        {
            installed = chromium_installed;
        }
    }
    let _ = child.wait().await;
    shared.state.lock().unwrap().chromium_installed = installed;
    Ok(installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> SearchSpec {
        SearchSpec {
            keywords: "dewalt drill".into(),
            category: None,
            location_text: "Seattle, WA".into(),
            lat: None,
            lng: None,
            radius_km: 40.0,
            price_ceiling: None,
        }
    }

    /// A bogus sidecar dir: if any code path tried to spawn the sidecar it
    /// would have to find this script. The disabled/non-runnable paths must
    /// return BEFORE reaching it, so these tests prove no spawn is attempted.
    fn shared() -> Arc<FbShared> {
        Arc::new(FbShared::new(
            "/nonexistent/profile".into(),
            "/nonexistent/sidecar".into(),
        ))
    }

    #[tokio::test]
    async fn disabled_adapter_returns_empty_without_spawning() {
        let shared = shared(); // state defaults: enabled = false
        let adapter = FacebookAdapter::new(shared);
        let out = adapter.search(&spec(), &SearchContext::default()).await;
        assert!(matches!(out, Ok(ref v) if v.is_empty()));
    }

    #[tokio::test]
    async fn enabled_but_not_logged_in_returns_empty_without_spawning() {
        let shared = shared();
        {
            let mut s = shared.state.lock().unwrap();
            s.enabled = true;
            s.tos_accepted = true;
            s.chromium_installed = true;
            s.logged_in = false; // not runnable
        }
        let adapter = FacebookAdapter::new(shared);
        let out = adapter.search(&spec(), &SearchContext::default()).await;
        assert!(matches!(out, Ok(ref v) if v.is_empty()));
    }

    #[tokio::test]
    async fn expired_session_surfaces_auth_error_never_silent_empty() {
        let shared = shared();
        {
            let mut s = shared.state.lock().unwrap();
            s.enabled = true;
            s.tos_accepted = true;
            s.chromium_installed = true;
            s.logged_in = true;
            s.session_expired = true;
        }
        let adapter = FacebookAdapter::new(shared);
        let out = adapter.search(&spec(), &SearchContext::default()).await;
        assert!(matches!(out, Err(AdapterError::Auth(_))));
    }

    #[test]
    fn health_reflects_lifecycle_states() {
        let shared = shared();
        let adapter = FacebookAdapter::new(shared.clone());
        // Off by default.
        assert!(matches!(adapter.health(), AdapterStatus::Disabled { .. }));
        {
            let mut s = shared.state.lock().unwrap();
            s.enabled = true; // ToS not yet accepted
        }
        assert!(matches!(adapter.health(), AdapterStatus::NotConfigured { .. }));
        {
            let mut s = shared.state.lock().unwrap();
            s.tos_accepted = true;
            s.chromium_installed = true; // logged_in still false
        }
        assert!(matches!(adapter.health(), AdapterStatus::Degraded { .. }));
        {
            let mut s = shared.state.lock().unwrap();
            s.logged_in = true;
        }
        assert!(matches!(adapter.health(), AdapterStatus::Ok));
    }
}
