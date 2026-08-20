use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{ACCEPT, USER_AGENT};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::crawler::http_client::HttpClient;
use crate::error::error::AppError;
use crate::service::json_document_service::JsonDocumentService;
use crate::util::time::now_ts;

const APP_NAMESPACE: &str = "_app";
const UPDATE_CACHE_NAME: &str = "version-update-cache";
const UPDATE_PREFERENCES_NAME: &str = "version-update-preferences";
const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/hadc188/reader/releases/latest";
/// 成功检查的缓存有效期。设置抽屉打开时用它展示缓存结果。
const UPDATE_CACHE_TTL_MS: i64 = 60 * 60 * 1000;
/// 上次检查失败后的重试间隔: 网络波动不应长时间掩盖新版本。
const UPDATE_ERROR_RETRY_MS: i64 = 15 * 60 * 1000;
const MAX_ASSET_BYTES: u64 = 512 * 1024 * 1024;
/// 直连 GitHub 失败时依次尝试的镜像前缀(前缀式加速代理)。
/// 镜像内容仍会经过大小/格式/便携包结构校验, 校验失败即丢弃。
const DOWNLOAD_MIRROR_PREFIXES: &[&str] = &["https://ghfast.top/", "https://gh-proxy.com/"];

#[derive(Clone)]
pub struct UpdateService {
    docs: Arc<JsonDocumentService>,
    http: HttpClient,
    current_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GithubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub html_url: String,
    pub published_at: Option<String>,
    #[serde(default)]
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GithubAsset {
    pub name: String,
    #[serde(rename = "browserDownloadUrl", alias = "browser_download_url")]
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
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
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateCache {
    release: Option<GithubRelease>,
    checked_at: i64,
    error: Option<String>,
}

impl UpdateService {
    /// 共用爬虫的 HttpClient: 更新请求随「系统/手动代理」设置一起走代理,
    /// 无代理直连 GitHub 慢或失败时再回退镜像加速。
    pub fn new(
        docs: Arc<JsonDocumentService>,
        http: HttpClient,
        current_version: impl Into<String>,
    ) -> Self {
        Self {
            docs,
            http,
            current_version: current_version.into(),
        }
    }

    pub async fn check(&self, force: bool) -> Result<VersionUpdateInfo, AppError> {
        let preferences = self.load_preferences().await?;
        let mut cache = self.load_cache().await?.unwrap_or_default();
        let now = now_ts() * 1000;
        // 失败的检查用更短的重试间隔, 避免一次网络失败把新版本提示掩盖数小时。
        let ttl = if cache.error.is_some() {
            UPDATE_ERROR_RETRY_MS
        } else {
            UPDATE_CACHE_TTL_MS
        };
        let stale = cache.checked_at <= 0 || now.saturating_sub(cache.checked_at) >= ttl;

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
        let error = match self.fetch_release_with(self.http.client()).await {
            Ok(release) => return Ok(release),
            Err(error) => error,
        };
        // 代理出口 IP 是共享的, 很容易被 GitHub 未认证限额(每 IP 60 次/小时)
        // 限流; 失败时再直连尝试一次, 两边网络环境互补。
        if self.http.active_proxy().is_some() {
            if let Ok(direct) = self.http.client_direct() {
                if let Ok(release) = self.fetch_release_with(direct).await {
                    return Ok(release);
                }
            }
        }
        Err(error)
    }

    async fn fetch_release_with(&self, client: reqwest::Client) -> Result<GithubRelease, String> {
        let response = client
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
            return Err(format_github_api_error(status, response).await);
        }
        response
            .json::<GithubRelease>()
            .await
            .map_err(|err| err.to_string())
    }

    pub async fn download_asset_to_path<F>(
        &self,
        asset: &GithubAsset,
        path: &Path,
        mut on_progress: F,
    ) -> Result<u64, AppError>
    where
        F: FnMut(u64, u64),
    {
        if asset.size > MAX_ASSET_BYTES {
            return Err(AppError::BadRequest("更新文件过大，已停止下载".to_string()));
        }
        let url = url::Url::parse(&asset.browser_download_url)
            .map_err(|_| AppError::BadRequest("更新地址无效".to_string()))?;
        if url.scheme() != "https" || url.host_str() != Some("github.com") {
            return Err(AppError::BadRequest("更新地址不是受信任的 GitHub 地址".to_string()));
        }

        // 直连优先; 无代理环境下直连失败时依次尝试镜像加速。
        let mut candidates: Vec<String> = vec![asset.browser_download_url.clone()];
        candidates.extend(
            DOWNLOAD_MIRROR_PREFIXES
                .iter()
                .map(|prefix| format!("{prefix}{}", asset.browser_download_url)),
        );

        let mut last_error: Option<AppError> = None;
        for candidate in &candidates {
            match self
                .download_from_url(candidate, asset.size, path, &mut on_progress)
                .await
            {
                Ok(downloaded) => return Ok(downloaded),
                Err(error) => {
                    let _ = tokio::fs::remove_file(path).await;
                    last_error = Some(error);
                }
            }
        }
        Err(last_error.unwrap_or_else(|| AppError::BadRequest("下载更新失败".to_string())))
    }

    async fn download_from_url<F>(
        &self,
        url: &str,
        asset_size: u64,
        path: &Path,
        on_progress: &mut F,
    ) -> Result<u64, AppError>
    where
        F: FnMut(u64, u64),
    {
        let response = self
            .http
            .client()
            .get(url)
            .timeout(Duration::from_secs(15 * 60))
            .header(ACCEPT, "application/octet-stream")
            .header(
                USER_AGENT,
                format!("reader-rust/{}", env!("CARGO_PKG_VERSION")),
            )
            .send()
            .await
            .map_err(|error| AppError::BadRequest(format!("下载更新失败: {error}")))?;
        if !response.status().is_success() {
            return Err(AppError::BadRequest(format!(
                "下载更新失败: 服务器返回 {}",
                response.status()
            )));
        }
        if response.content_length().unwrap_or_default() > MAX_ASSET_BYTES {
            return Err(AppError::BadRequest("更新文件过大，已停止下载".to_string()));
        }

        let response_size = response.content_length().unwrap_or_default();
        if asset_size > 0 && response_size > 0 && response_size != asset_size {
            return Err(AppError::BadRequest(format!(
                "更新文件大小与发行版记录不一致（应为 {} 字节，实际为 {} 字节）",
                asset_size, response_size
            )));
        }
        let expected_size = if asset_size > 0 {
            asset_size
        } else {
            response_size
        };
        let mut response = response;
        let mut file = tokio::fs::File::create(path)
            .await
            .map_err(|error| AppError::BadRequest(format!("创建更新临时文件失败: {error}")))?;
        let mut downloaded = 0_u64;
        on_progress(downloaded, expected_size);

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| AppError::BadRequest(format!("读取更新文件失败: {error}")))?
        {
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            if downloaded > MAX_ASSET_BYTES {
                return Err(AppError::BadRequest("更新文件过大，已停止下载".to_string()));
            }
            if expected_size > 0 && downloaded > expected_size {
                return Err(AppError::BadRequest("更新文件超过发行版记录大小，已停止下载".to_string()));
            }
            file.write_all(&chunk)
                .await
                .map_err(|error| AppError::BadRequest(format!("写入更新文件失败: {error}")))?;
            on_progress(downloaded, expected_size);
        }
        file.flush()
            .await
            .map_err(|error| AppError::BadRequest(format!("保存更新文件失败: {error}")))?;
        file.sync_all()
            .await
            .map_err(|error| AppError::BadRequest(format!("同步更新文件失败: {error}")))?;
        if expected_size > 0 && downloaded != expected_size {
            return Err(AppError::BadRequest(format!(
                "更新文件下载不完整（应为 {} 字节，实际为 {} 字节）",
                expected_size, downloaded
            )));
        }
        Ok(downloaded)
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

/// 把 GitHub API 的失败响应转成带原因的错误信息:
/// 403 最常见的是未认证接口限流(每 IP 60 次/小时, 共享出口 IP 很容易触发),
/// 其次是网络劫持或代理拦截。响应体里的 message 会原样透出。
async fn format_github_api_error(
    status: reqwest::StatusCode,
    response: reqwest::Response,
) -> String {
    let remaining = response
        .headers()
        .get("x-ratelimit-remaining")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let reset_at = response
        .headers()
        .get("x-ratelimit-reset")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|ts| *ts > 0);
    let body_message = response
        .json::<serde_json::Value>()
        .await
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(|message| message.as_str())
                .map(str::trim)
                .filter(|message| !message.is_empty())
                .map(str::to_string)
        });

    let mut text = format!("GitHub 返回 {status}");
    if let Some(message) = body_message {
        let message = message.chars().take(160).collect::<String>();
        text.push_str(&format!("：{message}"));
    }

    let rate_limited = remaining.as_deref() == Some("0")
        || text.to_ascii_lowercase().contains("rate limit");
    if rate_limited {
        text.push_str("（未认证接口限额为每 IP 60 次/小时，共享网络容易触发）");
        if let Some(ts) = reset_at {
            if let Some(time) = chrono::DateTime::from_timestamp(ts, 0) {
                let local = time.with_timezone(&chrono::Local);
                text.push_str(&format!("，额度将于 {} 重置", local.format("%H:%M")));
            }
        }
        return text;
    }
    if status.as_u16() == 403 {
        text.push_str("（可能是网络劫持或代理拦截，可配置代理后重试）");
    }
    text
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
    let published_at = release
        .as_ref()
        .and_then(|release| release.published_at.clone());
    let assets = release
        .as_ref()
        .map(|release| release.assets.clone())
        .unwrap_or_default();

    VersionUpdateInfo {
        current_version: ensure_v_prefix(current_version),
        latest_version,
        latest_name: release.as_ref().and_then(|release| release.name.clone()),
        release_url: release.as_ref().map(|release| release.html_url.clone()),
        published_at,
        update_available,
        should_remind: update_available && !dismissed_latest,
        dismissed_version,
        checked_at,
        error,
        assets,
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

#[cfg(test)]
mod tests {
    use super::GithubAsset;

    #[test]
    fn github_asset_uses_github_input_and_frontend_output_names() {
        let asset: GithubAsset = serde_json::from_value(serde_json::json!({
            "name": "Reader-v2-windows-x64-setup.exe",
            "browser_download_url": "https://github.com/example/setup.exe",
            "size": 42
        }))
        .unwrap();
        let value = serde_json::to_value(asset).unwrap();

        assert_eq!(
            value.get("browserDownloadUrl").and_then(|value| value.as_str()),
            Some("https://github.com/example/setup.exe")
        );
        assert!(value.get("browser_download_url").is_none());
    }
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
