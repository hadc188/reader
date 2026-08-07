use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{ACCEPT, USER_AGENT};
use serde::{Deserialize, Serialize};

use crate::error::error::AppError;
use crate::service::json_document_service::JsonDocumentService;
use crate::util::time::now_ts;

const APP_NAMESPACE: &str = "_app";
const UPDATE_CACHE_NAME: &str = "version-update-cache";
const UPDATE_PREFERENCES_NAME: &str = "version-update-preferences";
const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/givenge/reader-rust/releases/latest";
const UPDATE_CACHE_TTL_MS: i64 = 6 * 60 * 60 * 1000;

#[derive(Clone)]
pub struct UpdateService {
    docs: Arc<JsonDocumentService>,
    client: reqwest::Client,
    current_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GithubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub html_url: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePreferences {
    pub dismissed_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VersionUpdateInfo {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub latest_name: Option<String>,
    pub release_url: Option<String>,
    pub published_at: Option<String>,
    pub update_available: bool,
    pub should_remind: bool,
    pub dismissed_version: Option<String>,
    pub checked_at: i64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCache {
    release: Option<GithubRelease>,
    checked_at: i64,
    error: Option<String>,
}

impl UpdateService {
    pub fn new(
        docs: Arc<JsonDocumentService>,
        timeout_secs: u64,
        current_version: impl Into<String>,
    ) -> Result<Self, AppError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()?;
        Ok(Self {
            docs,
            client,
            current_version: current_version.into(),
        })
    }

    pub async fn check(&self, force: bool) -> Result<VersionUpdateInfo, AppError> {
        let preferences = self.load_preferences().await?;
        let mut cache = self.load_cache().await?.unwrap_or_default();
        let now = now_ts() * 1000;
        let stale =
            cache.checked_at <= 0 || now.saturating_sub(cache.checked_at) >= UPDATE_CACHE_TTL_MS;

        if force || stale {
            match self.fetch_latest_release().await {
                Ok(release) => {
                    cache.release = Some(release);
                    cache.error = None;
                }
                Err(err) => {
                    cache.error = Some(err);
                }
            }
            cache.checked_at = now;
            self.docs
                .set_value(APP_NAMESPACE, UPDATE_CACHE_NAME, &cache)
                .await?;
        }

        Ok(build_update_info(
            &self.current_version,
            cache.release,
            Some(preferences),
            cache.error,
            cache.checked_at,
        ))
    }

    pub async fn dismiss(&self, version: &str) -> Result<VersionUpdateInfo, AppError> {
        let version = version.trim();
        if version.is_empty() {
            return Err(AppError::BadRequest("缺少版本号".to_string()));
        }
        let preferences = UpdatePreferences {
            dismissed_version: Some(version.to_string()),
        };
        self.docs
            .set_value(APP_NAMESPACE, UPDATE_PREFERENCES_NAME, &preferences)
            .await?;
        self.check(false).await
    }

    async fn fetch_latest_release(&self) -> Result<GithubRelease, String> {
        let response = self
            .client
            .get(LATEST_RELEASE_URL)
            .header(ACCEPT, "application/vnd.github+json")
            .header(
                USER_AGENT,
                format!("reader-rust/{}", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await
            .map_err(|err| err.to_string())?;
        let status = response.status();
        if !status.is_success() {
            return Err(format!("GitHub 返回 {}", status));
        }
        response
            .json::<GithubRelease>()
            .await
            .map_err(|err| err.to_string())
    }

    async fn load_preferences(&self) -> Result<UpdatePreferences, AppError> {
        let Some(value) = self
            .docs
            .get_value(APP_NAMESPACE, UPDATE_PREFERENCES_NAME)
            .await?
        else {
            return Ok(UpdatePreferences::default());
        };
        serde_json::from_value(value).map_err(|err| AppError::BadRequest(err.to_string()))
    }

    async fn load_cache(&self) -> Result<Option<UpdateCache>, AppError> {
        let Some(value) = self
            .docs
            .get_value(APP_NAMESPACE, UPDATE_CACHE_NAME)
            .await?
        else {
            return Ok(None);
        };
        let cache =
            serde_json::from_value(value).map_err(|err| AppError::BadRequest(err.to_string()))?;
        Ok(Some(cache))
    }
}

pub fn build_update_info(
    current_version: &str,
    release: Option<GithubRelease>,
    preferences: Option<UpdatePreferences>,
    error: Option<String>,
    checked_at: i64,
) -> VersionUpdateInfo {
    let dismissed_version = preferences.and_then(|prefs| prefs.dismissed_version);
    let latest_version = release.as_ref().map(|release| release.tag_name.clone());
    let update_available = latest_version
        .as_deref()
        .map(|latest| is_newer_version(latest, current_version))
        .unwrap_or(false);
    let dismissed_latest = latest_version
        .as_deref()
        .zip(dismissed_version.as_deref())
        .map(|(latest, dismissed)| same_version(latest, dismissed))
        .unwrap_or(false);

    VersionUpdateInfo {
        current_version: ensure_v_prefix(current_version),
        latest_version,
        latest_name: release.as_ref().and_then(|release| release.name.clone()),
        release_url: release.as_ref().map(|release| release.html_url.clone()),
        published_at: release.and_then(|release| release.published_at),
        update_available,
        should_remind: update_available && !dismissed_latest,
        dismissed_version,
        checked_at,
        error,
    }
}

pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    let Some(candidate_parts) = parse_version(candidate) else {
        return false;
    };
    let Some(current_parts) = parse_version(current) else {
        return false;
    };
    candidate_parts > current_parts
}

fn same_version(left: &str, right: &str) -> bool {
    version_key(left) == version_key(right)
}

fn ensure_v_prefix(version: &str) -> String {
    let version = version.trim();
    if version.starts_with('v') || version.starts_with('V') {
        version.to_string()
    } else {
        format!("v{}", version)
    }
}

fn version_key(version: &str) -> String {
    trim_version(version).to_ascii_lowercase()
}

fn parse_version(version: &str) -> Option<Vec<u64>> {
    let mut parts = Vec::new();
    for part in trim_version(version).split('.') {
        let digits = part.trim();
        if digits.is_empty() {
            return None;
        }
        parts.push(digits.parse::<u64>().ok()?);
    }
    while parts.len() < 3 {
        parts.push(0);
    }
    Some(parts)
}

fn trim_version(version: &str) -> String {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .to_string()
}
