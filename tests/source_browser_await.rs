//! `java.startBrowserAwait(url, title)` 的**等待语义**。
//!
//! ## 症状（yckceo 书源 8054，番茄登录）
//!
//! 用户在窗口里登录完关掉窗口，桌面端既不显示「番茄凭证抓取诊断」页，也不
//! 提示成功 —— 登录永远停在「保存失败」。
//!
//! ## 根因
//!
//! 书源脚本是这么写的（`jsLib` 的 `_dpFqWebLogin`）：
//!
//! ```js
//! jv.startBrowserAwait('https://fanqienovel.com/', '番茄登录（登录完成后返回）');
//! return _dpFqGrabSave(jv, ckObj);   // ← 关窗之后才该跑
//! ```
//!
//! legado 的 `startBrowserAwait` 会**阻塞**：`SourceVerificationHelp.getVerificationResult`
//! 在 `while (getResult(...) == null) LockSupport.parkNanos(...)` 里一直等，
//! 直到 `WebViewActivity.finish()` 调 `checkResult()` 唤醒，才继续往下执行。
//!
//! 桌面端早先把纯 http(s) 网址当成「打开即返回」（fire-and-forget）：窗口刚弹
//! 出来、用户还没登录，下一行就已经去抓 Cookie 了 —— 抓到空会话，上传失败，
//! 诊断页自然也不出现。
//!
//! ## 现在的行为
//!
//! `startBrowserAwait` 对纯网址走 `await_external_window`：投递开窗请求后阻塞在
//! `recv_timeout` 上，直到窗口 `Destroyed` 事件回传「已完成」才继续。
//!
//! 本文件盯住「关窗 → 唤醒」这条握手，以及「窗口没开成不能空等」的保护。
use reader_rust::service::source_browser;

/// 没在登录上下文（`BROWSER_CONTEXT` 为空）时，`await_external_window` 必须
/// 立刻报错，而不是挂住线程 —— 否则阅读页里的段评/本章说会把 JS 线程卡 30 分钟。
///
/// 注意：`BROWSER_CONTEXT` 是线程局部的，本测试线程上必然是空的。
#[test]
fn await_without_login_context_fails_fast() {
    let request = reader_rust::service::book_service::ShowBrowserRequest {
        url: "https://fanqienovel.com/".to_string(),
        html: None,
        preload_js: None,
        options: None,
        title: Some("番茄登录".to_string()),
        await_result: true,
        panel_id: None,
        external_window: true,
        book_source_url: Some("https://m.qidian.com".to_string()),
        await_panel_id: None,
    };

    let started = std::time::Instant::now();
    let err = source_browser::await_external_window(request).unwrap_err();
    assert!(
        err.contains("登录窗口"),
        "应在缺少登录上下文时明确报错，实际: {err}"
    );
    // 必须是「立刻失败」而不是「等超时」。
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "缺少上下文时必须立刻返回，实际耗时 {:?}",
        started.elapsed()
    );
}

/// 关窗通知发给不存在的 id 不应 panic（用户可能重复关、或窗口没建起来）。
#[test]
fn completing_unknown_window_is_harmless() {
    source_browser::complete_external_window("external-does-not-exist", "https://example.com/");
    // 再来一次也不应出问题（幂等）。
    source_browser::complete_external_window("external-does-not-exist", "https://example.com/");
}

/// 取消一个不存在的等待同样不应 panic。
#[test]
fn cancelling_unknown_window_is_harmless() {
    source_browser::cancel_browser("external-does-not-exist");
}
