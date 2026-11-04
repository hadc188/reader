//! 验证窗口与书源请求之间的**会话互通**（8045 / Cloudflare 站点）。
//!
//! ## 症状
//!
//! 书源 8045（霹雳书屋，位于 Cloudflare 之后）在独立窗口里过完人机验证、
//! 「查看 Cookie 状态」也显示 ✅，但**搜索和发现页仍然 403**。
//!
//! ## 两个根因
//!
//! 1. **UA 不一致**：Cloudflare 的 `cf_clearance` 按 User-Agent 绑定。验证窗口
//!    默认是 WebView2 的桌面 Chrome UA，而书源 `header` 里写的是
//!    `Android 13; Pixel 7 … Mobile` —— 请求换成移动 UA 后 token 立刻失效。
//!    legado 手机端对此的处理是建窗口时就把书源 UA 设进去：
//!    `headerMap[AppConst.UA_NAME]?.let { userAgentString = it }`。
//!
//! 2. **域级 cookie 丢成 host-only**：`CookieDomain::as_cow()` 对
//!    `Suffix(".pili45.com")` 和 `HostOnly("www.pili45.com")` 都返回裸域名，
//!    快照时若不补回前导点，注入窗口的 `cf_clearance` 会退化成 host-only，
//!    `www.` 子域拿不到。
//!
//! 本文件盯住第 2 点（纯数据层，可测）；第 1 点依赖 WebView2 运行时，只能实测。
use reader_rust::crawler::source_session::SourceCookieJar;
use url::Url;

fn anchor() -> Url {
    Url::parse("https://www.pili45.com/").unwrap()
}

/// 域级 cookie 的快照必须带前导点，否则再注入窗口时会退化成 host-only。
#[test]
fn snapshot_keeps_domain_suffix_marker() {
    let jar = SourceCookieJar::new(None);
    jar.set_detailed("cf_clearance", "tok", ".pili45.com", "/", true, true, &anchor())
        .expect("写入应成功");

    let snapshot = jar.snapshot();
    let cookie = snapshot
        .iter()
        .find(|c| c.name == "cf_clearance")
        .expect("应能快照出 cf_clearance");

    assert_eq!(
        cookie.domain, ".pili45.com",
        "域级 cookie 必须保留前导点，否则注入窗口后只对裸域生效"
    );
    assert!(cookie.http_only, "cf_clearance 是 HttpOnly，注入窗口时要保持");
    assert!(cookie.secure);
}

/// host-only cookie 的快照不带前导点 —— 不能被误升级成域级。
#[test]
fn snapshot_keeps_host_only_cookie_scoped() {
    let jar = SourceCookieJar::new(None);
    jar.set_detailed("sid", "abc", "www.pili45.com", "/", true, false, &anchor())
        .expect("写入应成功");

    let cookie = jar
        .snapshot()
        .into_iter()
        .find(|c| c.name == "sid")
        .expect("应能快照出 sid");
    assert_eq!(cookie.domain, "www.pili45.com", "host-only 不应被加前导点");
}

/// 快照 → 重新写入一个新 jar，作用域语义必须完全一致（模拟「jar → 窗口」的往返）。
#[test]
fn snapshot_reinjection_preserves_scope() {
    let source = SourceCookieJar::new(None);
    source
        .set_detailed("cf_clearance", "tok", ".pili45.com", "/", true, true, &anchor())
        .unwrap();

    // 模拟 write_into_window：拿快照里的 domain 原样重建
    let target = SourceCookieJar::new(None);
    for cookie in source.snapshot() {
        target
            .set_detailed(
                &cookie.name,
                &cookie.value,
                &cookie.domain,
                &cookie.path,
                cookie.secure,
                cookie.http_only,
                &anchor(),
            )
            .unwrap();
    }

    // 子域必须能拿到（这条是 8045 的关键）
    assert_eq!(
        target.get(&Url::parse("https://www.pili45.com").unwrap()),
        "cf_clearance=tok",
        "重新注入后 www 子域应仍能看到 cf_clearance"
    );
}

/// 空 jar 不应产出任何记录。
#[test]
fn empty_jar_snapshots_nothing() {
    assert!(SourceCookieJar::new(None).snapshot().is_empty());
}

/// 多个 cookie 都要出现在快照里（`cf_clearance` + `__cf_bm` 是常见组合）。
#[test]
fn snapshot_contains_all_cookies() {
    let jar = SourceCookieJar::new(None);
    jar.set_detailed("cf_clearance", "tok", ".pili45.com", "/", true, true, &anchor())
        .unwrap();
    jar.set_detailed("__cf_bm", "bm", ".pili45.com", "/", true, true, &anchor())
        .unwrap();
    jar.set_detailed("PHPSESSID", "sid", "www.pili45.com", "/", true, false, &anchor())
        .unwrap();

    let names: Vec<String> = jar.snapshot().into_iter().map(|c| c.name).collect();
    assert_eq!(names.len(), 3, "实际: {names:?}");
    for expected in ["cf_clearance", "__cf_bm", "PHPSESSID"] {
        assert!(names.iter().any(|n| n == expected), "缺少 {expected}");
    }
}
