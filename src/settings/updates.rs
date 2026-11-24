//! User-requested release metadata only. Construction and polling never do I/O.
use super::i18n::tr;
use semver::Version;
use serde::Deserialize;
use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

const RELEASES_API: &str =
    "https://api.github.com/repos/dmytro-yemelianov/upyr/releases?per_page=100";
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    state: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct AvailableRelease {
    version: Version,
    preview: bool,
    pub url: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CheckError {
    Network,
    RateLimited,
    InvalidResponse,
    NoCompatibleRelease,
    Worker,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) enum CheckState {
    #[default]
    Idle,
    Checking,
    Current,
    Available(AvailableRelease),
    Failed(CheckError),
}

impl CheckState {
    pub fn message(&self) -> String {
        match self {
            Self::Idle => {
                tr("Contacts GitHub only when you press the button. Installation stays manual.")
                    .into()
            }
            Self::Checking => tr("Checking for updates…").into(),
            Self::Current => tr("No newer version is available for this platform.").into(),
            Self::Available(release) => {
                let version = &release.version;
                let suffix = if release.preview {
                    tr(" (preview)")
                } else {
                    ""
                };
                super::i18n::localized_format!(
                    "Upyr {version}{suffix} is available.",
                    "Доступний Upyr {version}{suffix}."
                )
            }
            Self::Failed(CheckError::RateLimited) => {
                tr("GitHub's request limit was reached. Try again later.").into()
            }
            Self::Failed(CheckError::InvalidResponse) => {
                tr("Could not read the release information. Try again later.").into()
            }
            Self::Failed(CheckError::NoCompatibleRelease) => {
                tr("No release for this platform was found.").into()
            }
            Self::Failed(CheckError::Network) => {
                tr("Could not reach GitHub. Check your connection and try again.").into()
            }
            Self::Failed(CheckError::Worker) => {
                tr("Could not start or finish the check. Try again.").into()
            }
        }
    }

    pub fn release_url(&self) -> Option<&str> {
        match self {
            Self::Available(release) => Some(&release.url),
            _ => None,
        }
    }
}

type CheckResult = Result<Option<AvailableRelease>, CheckError>;

#[derive(Default)]
pub(super) struct UpdateCheck {
    pub state: CheckState,
    pending: Option<Receiver<CheckResult>>,
}

impl UpdateCheck {
    /// Call only from the explicit Check for updates button handler.
    pub fn start(&mut self) {
        self.start_with(check_releases);
    }

    pub fn is_checking(&self) -> bool {
        self.pending.is_some()
    }

    fn start_with(&mut self, check: impl FnOnce() -> CheckResult + Send + 'static) {
        if self.is_checking() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        match std::thread::Builder::new()
            .name("upyr-update-check".into())
            .spawn(move || {
                let _ = sender.send(check());
            }) {
            Ok(_) => {
                self.state = CheckState::Checking;
                self.pending = Some(receiver);
            }
            Err(_) => self.state = CheckState::Failed(CheckError::Worker),
        }
    }

    /// Collect an already requested result; this never starts a request.
    pub fn poll(&mut self) -> bool {
        let Some(receiver) = &self.pending else {
            return false;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err(CheckError::Worker),
        };
        self.pending = None;
        self.state = match result {
            Ok(Some(release)) => CheckState::Available(release),
            Ok(None) => CheckState::Current,
            Err(error) => CheckState::Failed(error),
        };
        true
    }
}

fn platform_asset_prefix() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64" | "x86_64") => "upyr-macos-universal-",
        ("windows", "x86_64") => "upyr-windows-x86_64-",
        ("linux", "x86_64") => "upyr-linux-amd64-",
        // Never suggest an incompatible package on unsupported architectures.
        _ => "unsupported-platform-",
    }
}

fn select_release(body: &[u8], current: &str, asset_prefix: &str) -> CheckResult {
    let releases: Vec<Release> =
        serde_json::from_slice(body).map_err(|_| CheckError::InvalidResponse)?;
    let current = Version::parse(current).map_err(|_| CheckError::InvalidResponse)?;
    let newest = releases
        .into_iter()
        .filter_map(|release| {
            if release.draft
                || !release.assets.iter().any(|asset| {
                    asset.state == "uploaded"
                        && asset.name.starts_with(asset_prefix)
                        && [".exe", ".zip", ".dmg", ".deb", ".tar.gz"]
                            .iter()
                            .any(|ext| asset.name.ends_with(ext))
                })
            {
                return None;
            }
            let number = release
                .tag_name
                .strip_prefix("preview-")
                .or_else(|| release.tag_name.strip_prefix('v'))?;
            let version = Version::parse(number).ok()?;
            let preview = release.prerelease
                || release.tag_name.starts_with("preview-")
                || !version.pre.is_empty();
            // Construct the destination from a validated SemVer tag, never a response URL.
            let url = format!(
                "{}/releases/tag/{}",
                super::REPOSITORY_URL,
                release.tag_name
            );
            Some(AvailableRelease {
                version,
                preview,
                url,
            })
        })
        .max_by(|a, b| {
            a.version
                .cmp_precedence(&b.version)
                .then_with(|| b.preview.cmp(&a.preview))
        });
    let newest = newest.ok_or(CheckError::NoCompatibleRelease)?;
    Ok((newest.version.cmp_precedence(&current).is_gt()).then_some(newest))
}

fn check_releases() -> CheckResult {
    // No credentials, user content, persistent cache, redirects or retry loop.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    let mut response = agent
        .get(RELEASES_API)
        .header("User-Agent", concat!("Upyr/", env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|error| match error {
            ureq::Error::StatusCode(403 | 429) => CheckError::RateLimited,
            _ => CheckError::Network,
        })?;
    if response.status() != 200 {
        return Err(CheckError::InvalidResponse);
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(|_| CheckError::Network)?;
    select_release(&body, super::APP_VERSION, platform_asset_prefix())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release(tag: &str) -> serde_json::Value {
        json!({"tag_name":tag,"draft":false,"prerelease":false,
            "assets":[{"name":"upyr-windows-x86_64-setup.exe","state":"uploaded"}]})
    }

    fn select(releases: Vec<serde_json::Value>, current: &str) -> CheckResult {
        select_release(
            &serde_json::to_vec(&releases).unwrap(),
            current,
            "upyr-windows-x86_64-",
        )
    }

    #[test]
    fn selects_by_semver_not_order_or_lexical_comparison() {
        let found = select(
            vec![
                release("v0.9.0"),
                release("preview-0.10.0"),
                release("v0.8.0"),
            ],
            "0.9.0",
        )
        .unwrap()
        .unwrap();
        assert_eq!(found.version, Version::new(0, 10, 0));
        assert!(found.preview);
        assert_eq!(
            found.url,
            "https://github.com/dmytro-yemelianov/upyr/releases/tag/preview-0.10.0"
        );
    }

    #[test]
    fn ignores_equal_versions_build_metadata_and_older_versions() {
        for tag in [
            "v0.6.0",
            "preview-0.6.0",
            "v0.6.0+rebuild.2",
            "v0.5.0",
            "v0.6.0-rc.1",
        ] {
            assert_eq!(select(vec![release(tag)], "0.6.0"), Ok(None));
        }
    }

    #[test]
    fn stable_wins_tie_and_semver_prereleases_are_respected() {
        let found = select(
            vec![
                release("v0.7.0"),
                release("preview-0.7.0"),
                release("v0.7.0-rc.1"),
            ],
            "0.6.0",
        )
        .unwrap()
        .unwrap();
        assert!(!found.preview);
        assert!(found.url.ends_with("/v0.7.0"));
        assert!(
            select(vec![release("v0.6.0")], "0.6.0-rc.1")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn skips_drafts_invalid_tags_and_incompatible_or_incomplete_assets() {
        let mut draft = release("v99.0.0");
        draft["draft"] = json!(true);
        let mut other = release("v98.0.0");
        other["assets"][0]["name"] = json!("upyr-macos-universal-98.0.0.dmg");
        let mut incomplete = release("v97.0.0");
        incomplete["assets"][0]["state"] = json!("starter");
        let mut checksum = release("v96.0.0");
        checksum["assets"][0]["name"] = json!("upyr-windows-x86_64-96.0.0.zip.sha256");
        let found = select(
            vec![
                draft,
                other,
                incomplete,
                checksum,
                release("v100.0.0/../../other"),
                release("macos-preview-old"),
                release("v0.7.0"),
            ],
            "0.6.0",
        )
        .unwrap()
        .unwrap();
        assert_eq!(found.version, Version::new(0, 7, 0));
    }

    #[test]
    fn empty_or_bad_data_never_claims_up_to_date() {
        assert_eq!(
            select(vec![], "0.6.0"),
            Err(CheckError::NoCompatibleRelease)
        );
        assert_eq!(
            select(vec![release("unknown")], "0.6.0"),
            Err(CheckError::NoCompatibleRelease)
        );
        for body in [
            b"not json".as_slice(),
            b"{}",
            b"[{\"tag_name\":\"v0.7.0\"}]",
        ] {
            assert_eq!(
                select_release(body, "0.6.0", "upyr-"),
                Err(CheckError::InvalidResponse)
            );
        }
    }

    #[test]
    fn idle_poll_is_inert_duplicate_clicks_are_ignored_and_failure_can_retry() {
        let mut check = UpdateCheck::default();
        assert_eq!(check.state, CheckState::Idle);
        assert!(!check.poll());
        assert!(!check.is_checking());
        let (send, recv) = mpsc::channel();
        check.start_with(move || recv.recv().unwrap());
        assert_eq!(check.state, CheckState::Checking);
        check.start_with(|| panic!("duplicate worker"));
        assert!(!check.poll());
        send.send(Err(CheckError::Network)).unwrap();
        finish(&mut check);
        assert_eq!(check.state, CheckState::Failed(CheckError::Network));
        check.start_with(|| Ok(None));
        finish(&mut check);
        assert_eq!(check.state, CheckState::Current);
        assert!(!check.is_checking());
    }

    fn finish(check: &mut UpdateCheck) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !check.poll() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn disconnected_worker_reports_failure_and_reenables_the_button() {
        let (sender, receiver) = mpsc::channel();
        let mut check = UpdateCheck {
            state: CheckState::Checking,
            pending: Some(receiver),
        };
        drop(sender);
        assert!(check.poll());
        assert_eq!(check.state, CheckState::Failed(CheckError::Worker));
        assert!(!check.is_checking());
    }

    #[test]
    fn about_controls_are_localized_inert_until_clicked_and_disable_while_checking() {
        use crate::{
            config::{Config, UiLanguage},
            settings::{ActivationInbox, SettingsApp, SettingsTab},
        };
        use eframe::egui;
        for (language, button, link) in [
            (UiLanguage::English, "Check for updates", "Open release"),
            (
                UiLanguage::Ukrainian,
                "Перевірити оновлення",
                "Відкрити реліз",
            ),
        ] {
            crate::settings::i18n::set_language(language);
            let mut app = SettingsApp::new(
                Config {
                    ui_language: language,
                    ..Config::default()
                },
                crate::autostart::AutostartStatus {
                    enabled: false,
                    state: crate::autostart::AutostartState::Disabled,
                    location: String::new(),
                    detail: None,
                },
                SettingsTab::About,
                ActivationInbox {
                    directory: std::env::temp_dir().join("unused-update-test-inbox"),
                },
            );
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let render = |app: &mut SettingsApp| {
                let mut output = None;
                for _ in 0..2 {
                    output = Some(ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(480.0, 800.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| app.draw_about(ui));
                        },
                    ));
                }
                output.unwrap().platform_output.accesskit_update.unwrap()
            };
            let tree = render(&mut app);
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some(button))
            );
            assert!(
                !tree
                    .nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some(link))
            );
            assert_eq!(app.updates.state, CheckState::Idle);
            assert!(!app.updates.is_checking());
            let (sender, receiver) = mpsc::channel();
            app.updates.start_with(move || receiver.recv().unwrap());
            let tree = render(&mut app);
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some(button) && node.is_disabled())
            );
            sender
                .send(select(
                    vec![release("preview-99.0.0")],
                    super::super::APP_VERSION,
                ))
                .unwrap();
            finish(&mut app.updates);
            let tree = render(&mut app);
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, node)| node.label() == Some(link))
            );
            assert!(ctx.used_size().x <= 481.0);
        }
    }

    #[test]
    #[ignore = "explicit live GitHub check; ordinary tests must remain offline"]
    fn live_release_lookup() {
        assert!(check_releases().is_ok());
    }
}
