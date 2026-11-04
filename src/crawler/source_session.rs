use cookie_store::CookieStore;
use reqwest::{cookie::CookieStore as ReqwestCookieStore, header::HeaderValue};
use std::{collections::HashMap, fs, path::{Path, PathBuf}, sync::{Arc, Mutex}};
use url::Url;

/// One source owns one cookie store, including requests made by its scripts.
/// Cookies retain their normal domain/path/expiry rules within that source.
pub struct SourceCookieJar {
    store: Mutex<CookieStore>,
    path: Option<PathBuf>,
}

impl SourceCookieJar {
    pub fn new(path: Option<PathBuf>) -> Self {
        let store = path.as_ref().and_then(|path| fs::read(path).ok())
            .and_then(|bytes| cookie_store::serde::json::load(bytes.as_slice()).ok())
            .unwrap_or_default();
        Self { store: Mutex::new(store), path }
    }

    fn persist(&self, store: &CookieStore) -> anyhow::Result<()> {
        if let Some(path) = &self.path {
            let mut bytes = Vec::new();
            cookie_store::serde::json::save_incl_expired_and_nonpersistent(store, &mut bytes)
                .map_err(|e| anyhow::anyhow!("保存登录状态失败: {e}"))?;
            write_private_state(path, &bytes)?;
        }
        Ok(())
    }

    pub fn get(&self, url: &Url) -> String {
        self.store.lock().unwrap_or_else(|e| e.into_inner()).get_request_values(url)
            .map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join("; ")
    }

    /// 当前 jar 里的全部 Cookie（供注入验证窗口，见
    /// `book_service::source_session_cookies`）。
    ///
    /// `iter_any` 不做域名过滤 —— 这里要的就是「这个源自己的全部会话」，
    /// 每个书源一个独立 jar，不会串台。
    ///
    /// **域级 / host-only 必须区分**：`CookieDomain::as_cow()` 对
    /// `Suffix(".pili45.com")` 与 `HostOnly("www.pili45.com")` 都返回裸域名，
    /// 直接拿去注入会把 host-only 的会话泄漏给整个域。这里给 `Suffix` 补回
    /// 前导点（`CookieRecord.domain` 的约定就是「带点=域级、不带点=host-only」，
    /// 与 `set_source_window_cookies` / `set_detailed` 一致）。
    pub fn snapshot(&self) -> Vec<crate::service::webview_cookies::CookieRecord> {
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter_any()
            .filter_map(|cookie| {
                let host = match &cookie.domain {
                    cookie_store::CookieDomain::Suffix(host) => format!(".{host}"),
                    cookie_store::CookieDomain::HostOnly(host) => host.clone(),
                    // `NotPresent` / `Empty` 没有可归属的域，注入窗口无从下手。
                    _ => return None,
                };
                Some(crate::service::webview_cookies::CookieRecord {
                    name: cookie.name().to_string(),
                    value: cookie.value().to_string(),
                    domain: host,
                    path: cookie.path.as_ref().to_string(),
                    secure: cookie.secure().unwrap_or(false),
                    http_only: cookie.http_only().unwrap_or(false),
                })
            })
            .collect()
    }

    pub fn set(&self, url: &Url, header: &str) -> anyhow::Result<()> {
        self.update(url, header, false)
    }

    /// 按**完整作用域**写入一条 cookie（从浏览窗口回收登录态时用）。
    ///
    /// 为什么不能复用 [`Self::set`]：那个方法收的是 `Cookie` 头字符串，走
    /// `CookieStore::parse`，而 `parse` **不解析 `Domain` 属性** ——
    /// `"a=b; Domain=.example.com; Path=/"` 会把 `Domain=…` 当成值的一部分
    /// 存进去（实测），cookie 于是被钉成 host-only，只在写入时用的那个 host 上
    /// 生效。
    ///
    /// 这条差异直接表现为「登录窗口里过了 Cloudflare 验证、窗口关掉之后仍然
    /// 提示未检测到 cf_clearance」：`cf_clearance` 的作用域是 `.pili45.com`，
    /// 而书源地址是 `https://www.pili45.com` —— host-only 的 `pili45.com`
    /// cookie **不会**发给 `www.` 子域。
    ///
    /// 这里自己构造 `cookie::Cookie` 并调用 `insert_raw`，`Domain` 因此被当作
    /// 真实作用域处理，与浏览器行为一致。
    pub fn set_detailed(
        &self,
        name: &str,
        value: &str,
        domain: &str,
        path: &str,
        secure: bool,
        http_only: bool,
        request_url: &Url,
    ) -> anyhow::Result<()> {
        let mut builder = cookie::Cookie::build((name.to_string(), value.to_string()))
            .path(path.to_string());
        // **只有带前导点才算域级 cookie**（RFC 6265：`Domain=.example.com` 表示
        // 整域，不带点表示 host-only）。
        //
        // `cookie` crate 的 `.domain("example.com")` 会被 `cookie_store` 存成
        // `CookieDomain::Suffix("example.com")` —— 也就是**整域**生效。若不加
        // 判断地把任何域名都塞进去，host-only 会话（只在 `www.` 上有效的登录态）
        // 会被静默升级成整域，泄漏给所有子域。
        //
        // 不设 `.domain()` 时，`cookie_store` 按请求 URL 的主机名自动判为
        // `HostOnly`，正是想要的效果。
        let trimmed = domain.trim();
        if let Some(suffix) = trimmed.strip_prefix('.') {
            if !suffix.is_empty() {
                builder = builder.domain(suffix.to_string());
            }
        }
        if secure {
            builder = builder.secure(true);
        }
        if http_only {
            builder = builder.http_only(true);
        }
        let cookie = builder.build();
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = store.clone();
        next.insert_raw(&cookie, request_url)
            .map_err(|e| anyhow::anyhow!("写入 Cookie 失败: {e}"))?;
        self.persist(&next)?;
        *store = next;
        Ok(())
    }

    pub fn replace(&self, url: &Url, header: &str) -> anyhow::Result<()> {
        self.update(url, header, true)
    }

    /// 按名字移除该 URL 生效的 cookie。
    ///
    /// 面板页用 `document.cookie = 'x=; expires=<过去时间>'` 删除 cookie(退出登录),
    /// 这类写入必须把条目从 jar 去掉, 否则空值会被当成有效值发给上游。
    pub fn remove(&self, url: &Url, name: &str) -> anyhow::Result<()> {
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = store.clone();
        let keys = next.iter_any()
            .filter(|cookie| cookie.domain.matches(url) && cookie.name() == name)
            .filter_map(|cookie| Some((
                cookie.domain.as_cow()?.into_owned(),
                cookie.path.as_ref().to_string(),
                cookie.name().to_string(),
            )))
            .collect::<Vec<_>>();
        for (domain, path, name) in keys { next.remove(&domain, &path, &name); }
        self.persist(&next)?;
        *store = next;
        Ok(())
    }

    fn update(&self, url: &Url, header: &str, replace: bool) -> anyhow::Result<()> {
        HeaderValue::from_str(header).map_err(|_| anyhow::anyhow!("Cookie 格式无效"))?;
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = store.clone();
        if replace { remove_url_cookies(&mut next, url); }
        for pair in header.split(';').map(str::trim).filter(|pair| !pair.is_empty()) {
            anyhow::ensure!(pair.split_once('=').is_some_and(|(name, _)| !name.trim().is_empty()), "Cookie 格式无效");
            next.parse(&format!("{pair}; Path=/"), url)
                .map_err(|_| anyhow::anyhow!("Cookie 格式无效"))?;
        }
        self.persist(&next)?;
        *store = next;
        Ok(())
    }

    pub fn clear_url(&self, url: &Url) -> anyhow::Result<()> {
        self.replace(url, "")
    }

    pub fn clear(&self) -> anyhow::Result<()> {
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        let empty = CookieStore::default();
        self.persist(&empty)?;
        *store = empty;
        Ok(())
    }
}

fn remove_url_cookies(store: &mut CookieStore, url: &Url) {
    let keys = store.iter_any().filter(|cookie| cookie.domain.matches(url))
        .filter_map(|cookie| Some((cookie.domain.as_cow()?.into_owned(), cookie.path.as_ref().to_string(), cookie.name().to_string())))
        .collect::<Vec<_>>();
    for (domain, path, name) in keys { store.remove(&domain, &path, &name); }
}

impl ReqwestCookieStore for SourceCookieJar {
    fn set_cookies(&self, headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        for header in headers {
            if let Ok(value) = header.to_str() { let _ = store.parse(value, url); }
        }
        if let Err(err) = self.persist(&store) { tracing::warn!("保存书源会话失败: {err}"); }
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        let value = self.get(url);
        if value.is_empty() { None } else { HeaderValue::from_str(&value).ok() }
    }
}

pub struct SourceSession {
    pub identity: String,
    pub source_url: String,
    pub cookies: Arc<SourceCookieJar>,
    state: Mutex<HashMap<String, String>>,
    state_path: Option<PathBuf>,
    request_times: tokio::sync::Mutex<std::collections::VecDeque<tokio::time::Instant>>,
}

impl SourceSession {
    pub fn new(user_ns: &str, source_url: &str, root: Option<&Path>) -> Self {
        let key = format!("{user_ns}::{source_url}");
        let dir = root.map(|root| root.join(crate::util::hash::md5_hex(&key)));
        let state_path = dir.as_ref().map(|dir| dir.join("state.json"));
        let state = state_path.as_ref().and_then(|path| fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self {
            identity: format!("{}::{key}", root.map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()),
            source_url: source_url.to_string(),
            cookies: Arc::new(SourceCookieJar::new(dir.map(|dir| dir.join("cookies.json")))),
            state: Mutex::new(state), state_path,
            request_times: tokio::sync::Mutex::new(std::collections::VecDeque::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned()
    }

    pub fn put(&self, key: &str, value: Option<&str>) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = state.clone();
        match value {
            Some(value) => { next.insert(key.to_string(), value.to_string()); }
            None => { next.remove(key); }
        }
        if let Some(path) = &self.state_path { write_private_state(path, &serde_json::to_vec(&next)?)?; }
        *state = next;
        Ok(())
    }

    pub async fn request(&self, client: &reqwest::Client, req: super::fetcher::RequestSpec, rate: Option<&str>) -> anyhow::Result<super::fetcher::FetchResponse> {
        self.request_with(client, req, rate, false).await
    }

    /// 字节级请求: 与 [`Self::request`] 相同的限速/串行语义, 但请求体按原始字节
    /// 发送, 响应体不自动解压。
    pub async fn request_raw(&self, client: &reqwest::Client, req: super::fetcher::RequestSpec, rate: Option<&str>) -> anyhow::Result<super::fetcher::FetchResponse> {
        self.request_with(client, req, rate, true).await
    }

    async fn request_with(&self, client: &reqwest::Client, req: super::fetcher::RequestSpec, rate: Option<&str>, raw: bool) -> anyhow::Result<super::fetcher::FetchResponse> {
        // 只有最后一步的发送方式随 `raw` 变化, 限速/串行逻辑完全一致:
        // 用 `send` 收口, 避免在每条提前返回的分支里重复同一个 if/else。
        macro_rules! send {
            ($client:expr, $req:expr) => {
                if raw {
                    super::fetcher::fetch_raw_with_client($client, $req).await
                } else {
                    super::fetcher::fetch_with_client($client, $req).await
                }
            };
        }
        let rate = rate.unwrap_or("").trim();
        if rate.is_empty() || rate == "0" || req.url.starts_with("data:") {
            return send!(client, req);
        }
        let (limit, duration, serial) = if let Some((limit, window)) = rate.split_once('/') {
            match (limit.trim().parse::<usize>(), window.trim().parse::<u64>()) {
                (Ok(limit), Ok(window)) if limit > 0 => (limit, window, false),
                _ => return send!(client, req),
            }
        } else {
            match rate.parse::<u64>() {
                Ok(duration) => (1, duration, true),
                Err(_) => return send!(client, req),
            }
        };
        let mut starts = self.request_times.lock().await;
        let window = std::time::Duration::from_millis(duration);
        loop {
            let now = tokio::time::Instant::now();
            while starts.front().is_some_and(|start| now.duration_since(*start) >= window) { starts.pop_front(); }
            if starts.len() < limit { break; }
            tokio::time::sleep_until(*starts.front().unwrap() + window).await;
        }
        starts.push_back(tokio::time::Instant::now());
        if !serial {
            drop(starts);
            return send!(client, req);
        }
        // The guard releases even when a cancellable native request is dropped.
        let response = send!(client, req);
        drop(starts);
        response
    }
}

#[derive(Clone)]
pub struct SourceHttpContext {
    pub session: Arc<SourceSession>,
    pub client: reqwest::Client,
    /// 与 `client` 共享同一个 cookie jar, 但**不跟随 3xx**。
    /// legado 的 `java.get/head/post` 是 `followRedirects(false)` 语义,
    /// 书源靠读 302 的 `Location` 做两步搜索。见 `http_client::build_source_client_no_redirect`。
    pub client_no_redirect: reqwest::Client,
    pub proxy: Option<String>,
    pub timeout_secs: u64,
}

pub(crate) fn write_private_state(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() { fs::create_dir_all(parent)?; }
    let tmp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    Ok(())
}
