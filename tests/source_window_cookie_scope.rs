//! 登录窗口回收 Cookie 时的**作用域**语义（yckceo 8045 / Cloudflare 站点）。
//!
//! 症状：书源 8045（霹雳书屋）在独立窗口里过完 Cloudflare 人机验证，关掉窗口
//! 回到面板点「🍪 查看Cookie状态」，仍然 `❌ 未检测到 cf_clearance Cookie`。
//!
//! 根因在 `set_source_window_cookies`：它把 `CookieRecord` 拼成
//! `name=value; Path=/` 交给 `SourceCookieJar::set`，而 `CookieStore::parse`
//! **不解析 `Domain` 属性** ——
//!
//! ```text
//! set("https://pili45.com/", "cf_clearance=abc; Domain=.pili45.com; Path=/")
//!   => 存进去的值变成 "Path=/; Domain=.pili45.com; cf_clearance=abc"
//! ```
//!
//! 也就是说作用域信息被当成值的一部分吞掉，cookie 退化成 **host-only**，
//! 只在写入时用的那个 host 上生效。而 Cloudflare 的 `cf_clearance` 作用域是
//! `.pili45.com`，书源地址却是 `https://www.pili45.com` —— 于是子域取不到，
//! `java.getCookie()` 返回空串。
//!
//! 修法：`SourceCookieJar::set_detailed` 用 `cookie::Cookie` 显式带 Domain，
//! 走 `CookieStore::insert_raw`，作用域被正确记录。
use reader_rust::crawler::source_session::SourceCookieJar;
use url::Url;

fn jar() -> SourceCookieJar {
    SourceCookieJar::new(None)
}

fn anchor() -> Url {
    Url::parse("https://www.pili45.com/").unwrap()
}

/// Cloudflare 的 `cf_clearance` 带 `Domain=.pili45.com`，必须对子域可见。
///
/// 这条就是 8045 的直接回归：修复前 `www.pili45.com` 上读出来是空串。
#[test]
fn parent_domain_cookie_is_visible_to_subdomain() {
    let jar = jar();
    jar.set_detailed("cf_clearance", "tok123", ".pili45.com", "/", true, true, &anchor())
        .expect("写入应成功");

    assert_eq!(
        jar.get(&Url::parse("https://www.pili45.com").unwrap()),
        "cf_clearance=tok123",
        "带 Domain=.pili45.com 的 cookie 必须对 www 子域可见（修复前为空）"
    );
    assert_eq!(
        jar.get(&Url::parse("https://pili45.com").unwrap()),
        "cf_clearance=tok123",
        "上级域自身也可见"
    );
    // 不相关的域不能拿到
    assert_eq!(jar.get(&Url::parse("https://other.com").unwrap()), "");
}

/// 站点精确指定 `www.pili45.com` 时，不能泄漏给裸域或其它子域。
#[test]
fn exact_host_cookie_stays_scoped() {
    let jar = jar();
    jar.set_detailed("sid", "abc", "www.pili45.com", "/", true, false, &anchor())
        .expect("写入应成功");

    assert_eq!(
        jar.get(&Url::parse("https://www.pili45.com").unwrap()),
        "sid=abc"
    );
    assert_eq!(
        jar.get(&Url::parse("https://pili45.com").unwrap()),
        "",
        "精确 host 的 cookie 不应泄漏给裸域"
    );
    assert_eq!(
        jar.get(&Url::parse("https://m.pili45.com").unwrap()),
        "",
        "精确 host 的 cookie 不应泄漏给其它子域"
    );
}

/// 没有 Domain 属性（host-only）时，归属写入锚点所在的 host。
#[test]
fn host_only_cookie_uses_anchor_host() {
    let jar = jar();
    jar.set_detailed("hostonly", "v", "", "/", true, false, &anchor())
        .expect("写入应成功");

    assert_eq!(
        jar.get(&Url::parse("https://www.pili45.com").unwrap()),
        "hostonly=v"
    );
    assert_eq!(
        jar.get(&Url::parse("https://pili45.com").unwrap()),
        "",
        "host-only cookie 不应升级成域级"
    );
}

/// 同名 cookie 以新值覆盖，不产生重复项。
#[test]
fn rewriting_same_cookie_replaces_value() {
    let jar = jar();
    jar.set_detailed("cf_clearance", "old", ".pili45.com", "/", true, true, &anchor())
        .unwrap();
    jar.set_detailed("cf_clearance", "new", ".pili45.com", "/", true, true, &anchor())
        .unwrap();

    // 用户在窗口里重新验证过一次，jar 里应是新值且只有一份。
    let got = jar.get(&Url::parse("https://www.pili45.com").unwrap());
    assert_eq!(got, "cf_clearance=new");
}

/// 多个键按 `name=value; name2=value2` 拼接（书源脚本里 `indexOf('cf_clearance')` 依赖这个形态）。
#[test]
fn multiple_cookies_are_joined() {
    let jar = jar();
    jar.set_detailed("cf_clearance", "tok", ".pili45.com", "/", true, true, &anchor())
        .unwrap();
    jar.set_detailed("__cf_bm", "bm", ".pili45.com", "/", true, true, &anchor())
        .unwrap();

    let got = jar.get(&Url::parse("https://www.pili45.com").unwrap());
    assert!(got.contains("cf_clearance=tok"), "实际: {got}");
    assert!(got.contains("__cf_bm=bm"), "实际: {got}");
}
