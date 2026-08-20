use reqwest::{Client, NoProxy, Proxy};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use url::Url;

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
    active_proxy: Arc<RwLock<Option<String>>>,
    timeout_secs: u64,
}

impl HttpClient {
    pub fn new(timeout_secs: u64, proxy: Option<String>) -> anyhow::Result<Self> {
        let configured_proxy = match proxy {
            Some(value) => resolve_manual_proxy(&value)?,
            None => resolve_system_proxy(),
        };
        let client = build_client(timeout_secs, configured_proxy.as_deref())?;
        Ok(Self {
            client: Arc::new(RwLock::new(client)),
            active_proxy: Arc::new(RwLock::new(configured_proxy)),
            timeout_secs,
        })
    }

    pub fn client(&self) -> Client {
        self.client
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 当前是否有代理生效。
    pub fn active_proxy(&self) -> Option<String> {
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

        let current_proxy = self
            .active_proxy
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if current_proxy != configured_proxy {
            let client = build_client(self.timeout_secs, configured_proxy.as_deref())?;
            *self
                .client
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = client;
            *self
                .active_proxy
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = configured_proxy.clone();
        }

        Ok(ProxyStatus {
            active: configured_proxy.is_some(),
            address: configured_proxy,
        })
    }

    /// Discard the reqwest cookie jar by rebuilding the underlying Client with
    /// the same proxy. Used when a source cookie is changed/cleared so stale
    /// session cookies accumulated in the jar (e.g. an expired qidian session
    /// written by an antivirus-degraded page) stop being attached to requests.
    pub fn reset_cookie_jar(&self) -> anyhow::Result<()> {
        let configured_proxy = self
            .active_proxy
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let client = build_client(self.timeout_secs, configured_proxy.as_deref())?;
        *self
            .client
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = client;
        Ok(())
    }
}

fn build_client(timeout_secs: u64, proxy: Option<&str>) -> anyhow::Result<Client> {
    let mut builder = Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .cookie_store(true)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");
    if let Some(proxy) = proxy {
        let bypass = NoProxy::from_string("localhost,127.0.0.1,::1");
        builder = builder.proxy(Proxy::all(proxy)?.no_proxy(bypass));
    } else {
        builder = builder.no_proxy();
    }
    Ok(builder.build()?)
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
    platform_system_proxy()
        .or_else(environment_proxy)
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
fn platform_system_proxy() -> Option<String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    let settings = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings")
        .ok()?;
    let enabled = settings.get_value::<u32, _>("ProxyEnable").unwrap_or(0) == 1;
    if !enabled {
        return None;
    }
    let server = settings.get_value::<String, _>("ProxyServer").ok()?;
    select_proxy_server(&server)
}

#[cfg(target_os = "macos")]
fn platform_system_proxy() -> Option<String> {
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
            let host = setting_value(&text, "Server:")?;
            let port = setting_value(&text, "Port:")?;
            return Some(format!("http://{host}:{port}"));
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn platform_system_proxy() -> Option<String> {
    use std::process::Command;

    let mode = Command::new("gsettings")
        .args(["get", "org.gnome.system.proxy", "mode"])
        .output()
        .ok()?;
    if String::from_utf8_lossy(&mode.stdout).trim() != "'manual'" {
        return None;
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
            return Some(format!("http://{host}:{port}"));
        }
    }
    None
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn platform_system_proxy() -> Option<String> {
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
