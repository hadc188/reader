use reqwest::{Client, NoProxy, Proxy};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use url::Url;
use std::{collections::HashMap, path::PathBuf};
use super::source_session::{SourceHttpContext, SourceSession};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyMode {
    System,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProxyStatus {
    pub active: bool,
    pub address: Option<String>,
}

#[derive(Clone)]
pub struct HttpClient {
    client: Arc<RwLock<Client>>,
    proxy_mode: Arc<RwLock<ProxyMode>>,
    active_proxy: Arc<RwLock<Option<String>>>,
    timeout_secs: u64,
    source_sessions: Arc<RwLock<HashMap<String, Arc<SourceSession>>>>,
    source_clients: Arc<RwLock<HashMap<String, Client>>>,
    source_storage_dir: Arc<RwLock<Option<PathBuf>>>,
}

impl HttpClient {
    pub fn new(timeout_secs: u64, proxy: Option<String>) -> anyhow::Result<Self> {
        let (proxy_mode, configured_proxy) = match proxy {
            Some(value) => (ProxyMode::Manual, resolve_manual_proxy(&value)?),
            None => (ProxyMode::System, resolve_system_proxy()),
        };
        let client = build_client(timeout_secs, configured_proxy.as_deref())?;
        Ok(Self {
            client: Arc::new(RwLock::new(client)),
            proxy_mode: Arc::new(RwLock::new(proxy_mode)),
            active_proxy: Arc::new(RwLock::new(configured_proxy)),
            timeout_secs,
            source_sessions: Arc::default(),
            source_clients: Arc::default(),
            source_storage_dir: Arc::default(),
        })
    }

    pub fn client(&self) -> Client {
        // 系统代理可能由 Clash 等外部程序在运行时开关, 不能只沿用启动时的配置。
        let _ = self.refresh_system_proxy();
        self.client
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn set_source_storage_dir(&self, path: PathBuf) {
        *self.source_storage_dir.write().unwrap_or_else(|e| e.into_inner()) = Some(path);
    }

    pub fn source_context(&self, user_ns: &str, source_url: &str, proxy: Option<&str>) -> anyhow::Result<SourceHttpContext> {
        self.refresh_system_proxy()?;
        let proxy = match proxy.map(str::trim).filter(|v| !v.is_empty()) {
            Some(value) => resolve_manual_proxy(value)?,
            None => self.current_active_proxy(),
        };
        self.source_context_with_proxy(user_ns, source_url, proxy)
    }

    /// 与 [`Self::source_context`] 相同, 但**强制直连**(忽略系统/手动代理)。
    ///
    /// 代理出口是共享 IP, 容易被站点 WAF 整个拦掉(返回几百字节的 403 拒绝页);
    /// 同一台机器的直连出口常常正常。书源请求在这种情况下用它重试一次,
    /// 避免像 shuhaoxs.com 那样「手机端能看、桌面端整页空白」。
    pub fn source_context_direct(
        &self,
        user_ns: &str,
        source_url: &str,
    ) -> anyhow::Result<SourceHttpContext> {
        self.source_context_with_proxy(user_ns, source_url, None)
    }

    fn source_context_with_proxy(
        &self,
        user_ns: &str,
        source_url: &str,
        proxy: Option<String>,
    ) -> anyhow::Result<SourceHttpContext> {
        let source_url = crate::util::text::normalize_source_url(source_url);
        let key = format!("{user_ns}::{source_url}");
        let session = {
            let mut sessions = self.source_sessions.write().unwrap_or_else(|e| e.into_inner());
            sessions.entry(key.clone()).or_insert_with(|| {
                let root = self.source_storage_dir.read().unwrap_or_else(|e| e.into_inner());
                Arc::new(SourceSession::new(user_ns, &source_url, root.as_deref()))
            }).clone()
        };
        let client_key = format!("{key}::proxy={}", proxy.as_deref().unwrap_or(""));
        let mut clients = self.source_clients.write().unwrap_or_else(|e| e.into_inner());
        let client = match clients.get(&client_key) {
            Some(client) => client.clone(),
            None => {
                let client = build_source_client(self.timeout_secs, proxy.as_deref(), session.cookies.clone())?;
                clients.insert(client_key.clone(), client.clone());
                client
            }
        };
        // 不跟随重定向的孪生 client: legado 的 `java.get/head/post` 对应
        // `Jsoup.connect(...).followRedirects(false)`, 书源靠读 302 的 `Location`
        // 做两步搜索(先拿 searchid 再拼结果地址)。与上面的 client 共用同一个
        // cookie jar, 登录态不受影响。按需构建, 只有真正用到才付这份开销。
        let no_redirect_key = format!("{client_key}::no-redirect");
        let client_no_redirect = match clients.get(&no_redirect_key) {
            Some(client) => client.clone(),
            None => {
                let client = build_source_client_no_redirect(self.timeout_secs, proxy.as_deref(), session.cookies.clone())?;
                clients.insert(no_redirect_key, client.clone());
                client
            }
        };
        Ok(SourceHttpContext {
            session,
            client,
            client_no_redirect,
            proxy,
            timeout_secs: self.timeout_secs,
        })
    }

    /// 当前是否有代理生效。
    pub fn active_proxy(&self) -> Option<String> {
        let _ = self.refresh_system_proxy();
        self.current_active_proxy()
    }

    fn current_active_proxy(&self) -> Option<String> {
        self.active_proxy
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 完全不走代理的直连客户端。用于代理出口 IP 被目标站点限流时的回退。
    pub fn client_direct(&self) -> anyhow::Result<Client> {
        build_client(self.timeout_secs, None)
    }

    pub fn client_with_proxy(&self, proxy: Option<&str>) -> anyhow::Result<Client> {
        match proxy.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => {
                let proxy = resolve_manual_proxy(value)?;
                build_client(self.timeout_secs, proxy.as_deref())
            }
            None => Ok(self.client()),
        }
    }

    /// Build a fresh client with its own cookie jar. The shared client is kept
    /// for unrelated requests (RSS, updates, etc.), while book-source traffic
    /// uses one client per source so sessions cannot cross-contaminate.
    pub fn new_client_with_proxy(&self, proxy: Option<&str>) -> anyhow::Result<Client> {
        self.refresh_system_proxy()?;
        let configured_proxy = match proxy.map(str::trim).filter(|value| !value.is_empty()) {
            Some(value) => resolve_manual_proxy(value)?,
            None => self
                .active_proxy
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        };
        build_client(self.timeout_secs, configured_proxy.as_deref())
    }

    pub fn configure_proxy(
        &self,
        mode: ProxyMode,
        proxy: Option<&str>,
    ) -> anyhow::Result<ProxyStatus> {
        let configured_proxy = match mode {
            ProxyMode::System => resolve_system_proxy(),
            ProxyMode::Manual => {
                let value = proxy
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| anyhow::anyhow!("手动代理地址不能为空"))?;
                resolve_manual_proxy(value)?
            }
        };

        // 持有模式写锁, 防止配置切换与系统代理热刷新交错后写回旧模式。
        let mut current_mode = self
            .proxy_mode
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.install_proxy_if_changed(configured_proxy.clone())?;
        *current_mode = mode;

        Ok(ProxyStatus {
            active: configured_proxy.is_some(),
            address: configured_proxy,
        })
    }

    /// Discard the reqwest cookie jar by rebuilding the underlying Client with
    /// the same proxy. Used when a source cookie is changed/cleared so stale
    /// session cookies accumulated in the jar (e.g. an expired login session
    /// written by an antivirus-degraded page) stop being attached to requests.
    pub fn reset_cookie_jar(&self) -> anyhow::Result<()> {
        self.refresh_system_proxy()?;
        // 与代理切换共用 active_proxy 写锁, 防止基于旧代理构建的客户端
        // 在切换完成后覆盖新客户端。
        let configured_proxy = self
            .active_proxy
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let client = build_client(self.timeout_secs, configured_proxy.as_deref())?;
        *self
            .client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = client;
        Ok(())
    }

    fn refresh_system_proxy(&self) -> anyhow::Result<()> {
        let proxy_mode = self
            .proxy_mode
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *proxy_mode != ProxyMode::System {
            return Ok(());
        }
        self.install_proxy_if_changed(resolve_system_proxy())?;
        Ok(())
    }

    fn install_proxy_if_changed(&self, configured_proxy: Option<String>) -> anyhow::Result<bool> {
        if self.current_active_proxy() == configured_proxy {
            return Ok(false);
        }
        let mut active_proxy = self
            .active_proxy
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *active_proxy == configured_proxy {
            return Ok(false);
        }

        let client = build_client(self.timeout_secs, configured_proxy.as_deref())?;
        *self
            .client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = client;
        *active_proxy = configured_proxy;
        self.source_clients
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        Ok(true)
    }
}

fn build_client(timeout_secs: u64, proxy: Option<&str>) -> anyhow::Result<Client> {
    build_client_builder(timeout_secs, proxy)?.cookie_store(true).build().map_err(Into::into)
}

pub(crate) fn build_source_client(timeout_secs: u64, proxy: Option<&str>, jar: Arc<super::source_session::SourceCookieJar>) -> anyhow::Result<Client> {
    build_source_builder(timeout_secs, proxy)?.cookie_provider(jar).build().map_err(Into::into)
}

/// 与 [`build_source_client`] 相同, 但不跟随 3xx。
///
/// 对应 legado 的 `Jsoup.connect(...).followRedirects(false)`(`java.get`/`java.head`/
/// `java.post`)。书源用这种写法**读 302 响应的 `Location` 头**完成两步搜索:
/// 先用搜索词换一个 `searchid`, 再拿它拼结果页地址。跟随后 `Location` 消失,
/// 脚本在 `location.match(...)` 处抛空引用, 整个搜索直接失败。
pub(crate) fn build_source_client_no_redirect(timeout_secs: u64, proxy: Option<&str>, jar: Arc<super::source_session::SourceCookieJar>) -> anyhow::Result<Client> {
    build_source_builder(timeout_secs, proxy)?
        .redirect(reqwest::redirect::Policy::none())
        .cookie_provider(jar)
        .build()
        .map_err(Into::into)
}

/// 与 [`build_source_client`] 相同, 但关闭响应的自动解压。
///
/// `java.net.URL(...).openConnection()` 形态的脚本按 Java 语义自行处理
/// `Content-Encoding`(例如源脚本自己 new GZIPInputStream), 自动解压会把已解压
/// 的正文再解一次并丢掉原始头部, 因此这条路径必须拿到未解码的字节。
pub(crate) fn build_raw_source_client(timeout_secs: u64, proxy: Option<&str>, jar: Arc<super::source_session::SourceCookieJar>) -> anyhow::Result<Client> {
    build_source_builder(timeout_secs, proxy)?
        .gzip(false)
        .brotli(false)
        .deflate(false)
        .cookie_provider(jar)
        .build()
        .map_err(Into::into)
}

fn build_client_builder(timeout_secs: u64, proxy: Option<&str>) -> anyhow::Result<reqwest::ClientBuilder> {
    let builder = build_builder_base(timeout_secs, proxy)?
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");
    Ok(builder)
}

/// 书源请求用的 builder: 在通用配置之上打开 **浏览器风格的首部写法**。
///
/// Cloudflare 的托管挑战(managed challenge)对某些站点不只看 UA, 还看首部的
/// **大小写与顺序**: 同一份 `User-Agent` 用 `user-agent`(全小写)发出去直接
/// `403 cf-mitigated: challenge`, 改成 `User-Agent` 并把 `Host` 放在第一个首部
/// 就返回 200(实测站点上逐项二分确认)。reqwest/hyper 默认把首部全小写化 ——
/// 与真实浏览器(Chrome 发的是 Title-Case)不一致, 于是「手机端能读、桌面端
/// 整页被拦」。
///
/// `http1_title_case_headers` 只影响 HTTP/1.x(h2 要求小写, hyper 会自行处理),
/// 显式 `Host` 由 [`crate::crawler::fetcher`] 注入到请求首位。
fn build_source_builder(timeout_secs: u64, proxy: Option<&str>) -> anyhow::Result<reqwest::ClientBuilder> {
    Ok(build_builder_base(timeout_secs, proxy)?
        .http1_title_case_headers()
        .http1_only())
}

/// legado 的读超时。OkHttp 里 `readTimeout = 60s`、`callTimeout = 60s`
/// （见 legado `HttpHelper.kt`），与 15s 的连接超时是两个独立的轴。
///
/// 早先这里只有一个 `timeout_secs` 同时套住连接和读取：正文页/接口慢一点
/// （尤其是带图床或反爬延迟的站）就直接超时，而手机端同样的源能读 —— 这类
/// 「手机能看、桌面空白」有一部分就是这个原因。
///
/// 不把 60s 做成配置项：它是**协议层容忍度**而不是产品偏好，跟 `request_timeout_secs`
/// （连接阶段多久算不通）语义不同，混在一起调会让用户以为调大就能救慢站，
/// 实际先把「站点已挂」的判断拖长了。
const SOURCE_READ_TIMEOUT_SECS: u64 = 60;

fn build_builder_base(timeout_secs: u64, proxy: Option<&str>) -> anyhow::Result<reqwest::ClientBuilder> {
    let mut builder = Client::builder()
        // TLS：与 legado 一致，**不校验证书与主机名**。
        //
        // legado 的 `HttpHelper.kt` 用的是 `SSLHelper.unsafeSSLSocketFactory` +
        // `unsafeTrustManager` + `unsafeHostnameVerifier`，即全量跳过校验。书源
        // 站点里自签名证书、过期证书、证书链不全、`m.xxx.net` 与 `m.xxx.com`
        // 共用同一张证书的情况都很常见 —— 严格校验会直接把这些源判死，
        // 表现成「手机端能读、桌面端报错」。
        //
        // 风险面与手机端相同：书源抓的是公开网页，唯一需要保密的是登录态
        // Cookie，而那一层本来也是明文 HTTP 也照发。
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        // 连接阶段：沿用可配置的 `request_timeout_secs`。
        .connect_timeout(Duration::from_secs(timeout_secs.max(1)))
        // 读取阶段：按 legado 的 60s。`timeout` 是 reqwest 的**总**超时，
        // 必须 ≥ 读超时，否则连接快的慢响应仍会被总超时砍掉。
        .read_timeout(Duration::from_secs(SOURCE_READ_TIMEOUT_SECS))
        .timeout(Duration::from_secs(
            SOURCE_READ_TIMEOUT_SECS.max(timeout_secs),
        ));
    if let Some(proxy) = proxy {
        let bypass = NoProxy::from_string("localhost,127.0.0.1,::1");
        builder = builder.proxy(Proxy::all(proxy)?.no_proxy(bypass));
    } else {
        builder = builder.no_proxy();
    }
    Ok(builder)
}

fn resolve_manual_proxy(value: &str) -> anyhow::Result<Option<String>> {
    let normalized = if value.contains("://") {
        value.trim().to_string()
    } else {
        format!("http://{}", value.trim())
    };
    let parsed = Url::parse(&normalized).map_err(|_| anyhow::anyhow!("代理地址格式无效"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(anyhow::anyhow!("代理地址仅支持 HTTP 或 HTTPS"));
    }
    Ok(Some(normalized))
}

fn resolve_system_proxy() -> Option<String> {
    resolve_system_proxy_from(platform_system_proxy(), environment_proxy())
}

fn resolve_system_proxy_from(
    platform_proxy: Option<Option<String>>,
    environment_proxy: Option<String>,
) -> Option<String> {
    platform_proxy
        .unwrap_or(environment_proxy)
        .and_then(|value| resolve_manual_proxy(&value).ok().flatten())
}

fn environment_proxy() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy"]
        .into_iter()
        .find_map(|key| std::env::var(key).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(target_os = "windows")]
fn platform_system_proxy() -> Option<Option<String>> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    let settings = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enabled = match settings.get_value::<u32, _>("ProxyEnable") {
        Ok(value) => value == 1,
        Err(_) => return None,
    };
    if !enabled {
        return Some(None);
    }
    let Some(server) = settings.get_value::<String, _>("ProxyServer").ok() else {
        return Some(None);
    };
    Some(select_proxy_server(&server))
}

#[cfg(target_os = "macos")]
fn platform_system_proxy() -> Option<Option<String>> {
    use std::process::Command;

    let output = Command::new("networksetup")
        .arg("-listallnetworkservices")
        .output()
        .ok()?;
    let services = String::from_utf8_lossy(&output.stdout);
    for service in services.lines().skip(1).map(str::trim).filter(|line| !line.is_empty()) {
        let service = service.trim_start_matches('*').trim();
        for proxy_type in ["-getsecurewebproxy", "-getwebproxy"] {
            let proxy = Command::new("networksetup")
                .args([proxy_type, service])
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&proxy.stdout);
            if !text.lines().any(|line| line.trim() == "Enabled: Yes") {
                continue;
            }
            let Some(host) = setting_value(&text, "Server:") else {
                return Some(None);
            };
            let Some(port) = setting_value(&text, "Port:") else {
                return Some(None);
            };
            return Some(Some(format!("http://{host}:{port}")));
        }
    }
    Some(None)
}

#[cfg(target_os = "linux")]
fn platform_system_proxy() -> Option<Option<String>> {
    use std::process::Command;

    let mode = Command::new("gsettings")
        .args(["get", "org.gnome.system.proxy", "mode"])
        .output()
        .ok()?;
    if String::from_utf8_lossy(&mode.stdout).trim() != "'manual'" {
        return Some(None);
    }
    for scheme in ["https", "http"] {
        let schema = format!("org.gnome.system.proxy.{scheme}");
        let host = Command::new("gsettings")
            .args(["get", &schema, "host"])
            .output()
            .ok()?;
        let host = String::from_utf8_lossy(&host.stdout)
            .trim()
            .trim_matches('\'')
            .to_string();
        let port = Command::new("gsettings")
            .args(["get", &schema, "port"])
            .output()
            .ok()?;
        let port = String::from_utf8_lossy(&port.stdout).trim().to_string();
        if !host.is_empty() && port.parse::<u16>().ok().filter(|port| *port > 0).is_some() {
            return Some(Some(format!("http://{host}:{port}")));
        }
    }
    Some(None)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn platform_system_proxy() -> Option<Option<String>> {
    None
}

#[cfg(target_os = "windows")]
fn select_proxy_server(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if !value.contains('=') {
        return Some(value.to_string());
    }
    let entries = value.split(';').filter_map(|entry| entry.split_once('='));
    let mut http = None;
    for (scheme, address) in entries {
        let address = address.trim();
        if address.is_empty() {
            continue;
        }
        match scheme.trim().to_ascii_lowercase().as_str() {
            "https" => return Some(address.to_string()),
            "http" => http = Some(address.to_string()),
            _ => {}
        }
    }
    http
}

#[cfg(target_os = "macos")]
fn setting_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(key).map(str::trim))
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_proxy_accepts_local_address_without_scheme() {
        assert_eq!(
            resolve_manual_proxy("127.0.0.1:7890").unwrap(),
            Some("http://127.0.0.1:7890".to_string())
        );
    }

    #[test]
    fn manual_proxy_rejects_unsupported_scheme() {
        assert!(resolve_manual_proxy("socks5://127.0.0.1:7890").is_err());
    }

    #[test]
    fn explicit_system_proxy_state_takes_precedence_over_environment_proxy() {
        let environment = Some("http://env-proxy.example:7890".to_string());
        assert_eq!(resolve_system_proxy_from(Some(None), environment.clone()), None);
        assert_eq!(
            resolve_system_proxy_from(
                Some(Some("http://system-proxy.example:7890".to_string())),
                environment,
            ),
            Some("http://system-proxy.example:7890".to_string())
        );
    }

    #[test]
    fn environment_proxy_is_used_only_when_platform_state_is_unavailable() {
        assert_eq!(
            resolve_system_proxy_from(None, Some("env-proxy.example:7890".to_string())),
            Some("http://env-proxy.example:7890".to_string())
        );
    }

    #[test]
    fn changing_proxy_snapshot_rebuilds_clients_and_clears_source_cache() {
        let proxy = Some("http://127.0.0.1:7890".to_string());
        let http = HttpClient::new(5, proxy.clone()).unwrap();
        let source = "https://source.example.com";

        let context = http.source_context("proxy-refresh", source, None).unwrap();
        assert_eq!(context.proxy, proxy);
        assert!(!http
            .source_clients
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty());

        assert!(http.install_proxy_if_changed(None).unwrap());
        assert_eq!(http.active_proxy(), None);
        assert!(http
            .source_clients
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty());

        let direct = http.source_context("proxy-refresh", source, None).unwrap();
        assert_eq!(direct.proxy, None);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn selects_https_then_http_from_windows_proxy_list() {
        assert_eq!(
            select_proxy_server("http=127.0.0.1:7890;https=127.0.0.1:7891"),
            Some("127.0.0.1:7891".to_string())
        );
        assert_eq!(
            select_proxy_server("http=127.0.0.1:7890"),
            Some("127.0.0.1:7890".to_string())
        );
    }
}
