//! 加密/摘要类 `java.*` 入口的算法名校验（issue #29 的同类问题）。
//!
//! issue #29 报告的是 `java.HMacHex` / `java.HMacBase64` 在收到 JCA 规范名
//! （`HmacSHA1`）时**静默返回空串**。同一处 `normalize_digest_name` 分发上还有
//! 两个入口有完全相同的问题，本文件盯住它们：
//!
//! | 入口 | 修复前 | 修复后 |
//! | --- | --- | --- |
//! | `digestHex(abc, "HmacSHA1")` | `""` | `a9993e36…`（SHA-1 摘要） |
//! | `digestHex(abc, "bogus")` | `""` | 抛 `不支持摘要算法 bogus` |
//! | `digestBase64Str(abc, "bogus")` | `""` | 抛同上 |
//! | `createSymmetricCrypto("BOGUS/…")` | `""` | 抛 `不支持的算法 BOGUS` |
//!
//! ## 一条重要边界
//!
//! 只有**配置错误**（算法名/模式/密钥长度）才抛错。**数据错误**（解密填充不对、
//! GCM 标签不匹配、base64 非法）必须继续返回空串 —— 书源经常故意试解密，失败后
//! 落回 `||` 分支（`decrypt(x) || result`），抛错会打断整条规则。
//! 这条边界由 `parser::js::CryptConfigError` 承担，`crypt_config_error_*` 两个
//! 用例分别盯住两侧。
use reader_rust::parser::js::eval_js;

fn call(expr: &str) -> String {
    eval_js(expr, "", "").unwrap_or_else(|err| panic!("{expr} 求值失败: {err}"))
}

fn expect_err(expr: &str) -> String {
    let err = eval_js(expr, "", "").expect_err(&format!("{expr} 应抛错而不是返回空串"));
    err.to_string()
}

// ── 摘要函数 ───────────────────────────────────────────────────────────

/// `digestHex` / `digestBase64Str` 要接受 JCA 的 `Hmac*` 前缀写法。
///
/// 书源常把 `HMacHex` 的算法名直接抄到摘要函数上。legado 的
/// `DigestUtil.digester("HmacSHA1")` 会抛 `NoSuchAlgorithmException`，但把
/// `HmacSHA1` 认成 `SHA-1` 并不会算错（HMAC 的底层摘要就是它），比让整条
/// 规则失败有用。
#[test]
fn digest_accepts_jca_prefixed_names() {
    // SHA-1("abc")
    assert_eq!(
        call(r#"java.digestHex("abc", "HmacSHA1")"#),
        "a9993e364706816aba3e25717850c26c9cd0d89d"
    );
    // SHA-256("abc")，确认前缀剥离后落到正确的摘要
    assert_eq!(
        call(r#"java.digestHex("abc", "HmacSHA256")"#),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    // 三种写法等价
    assert_eq!(
        call(r#"java.digestHex("abc", "SHA-256")"#),
        call(r#"java.digestHex("abc", "SHA256")"#)
    );
}

/// 未知算法要抛错，不能静默给空串。
#[test]
fn digest_unknown_algorithm_throws() {
    for expr in [
        r#"java.digestHex("abc", "bogus")"#,
        r#"java.digestBase64Str("abc", "bogus")"#,
        r#"java.digestHex("abc", "")"#,
    ] {
        let message = expect_err(expr);
        assert!(
            message.contains("不支持摘要算法"),
            "{expr} 的报错应指出算法不支持，实际: {message}"
        );
    }

    // 脚本能 try/catch 接住（书源的降级写法）
    assert_eq!(
        call(r#"try { java.digestHex("abc","bogus") } catch (e) { "caught" }"#),
        "caught"
    );
}

/// 正常算法名不受影响（防止校验写过头）。
#[test]
fn digest_known_algorithms_still_work() {
    for (algo, hex_len) in [
        ("MD5", 32usize),
        ("SHA-1", 40),
        ("SHA-224", 56),
        ("SHA-256", 64),
        ("SHA-384", 96),
        ("SHA-512", 128),
    ] {
        let out = call(&format!(r#"java.digestHex("abc", "{algo}")"#));
        assert_eq!(out.len(), hex_len, "{algo} 摘要长度不对: {out:?}");
        let b64 = call(&format!(r#"java.digestBase64Str("abc", "{algo}")"#));
        assert!(!b64.is_empty(), "{algo} 的 base64 摘要不应为空");
    }
}

// ── 对称加解密 ─────────────────────────────────────────────────────────

/// 配置错误（算法名 / 模式 / 密钥长度）要抛错。
#[test]
fn crypt_config_errors_throw() {
    for (expr, expect) in [
        (
            r#"java.createSymmetricCrypto("BOGUS/ECB/PKCS5Padding","0123456789abcdef","").encryptBase64("x")"#,
            "不支持的算法",
        ),
        (
            r#"java.createSymmetricCrypto("AES/XXX/PKCS5Padding","0123456789abcdef","").encryptBase64("x")"#,
            "不支持的模式",
        ),
        (
            r#"java.createSymmetricCrypto("AES/ECB/PKCS5Padding","short","").encryptBase64("x")"#,
            "AES 密钥长度无效",
        ),
    ] {
        let message = expect_err(expr);
        assert!(
            message.contains(expect),
            "报错应包含 {expect:?}，实际: {message}"
        );
    }
}

/// **数据错误绝不能抛错** —— 书源靠「解密失败 → 空串 → `||` 兜底」工作。
///
/// 这条是本次改动最容易踩坏的地方：把 `__sym` 整体改成抛错会让一批书源
/// 的兜底分支失效。
#[test]
fn crypt_data_errors_still_return_empty() {
    for expr in [
        // 垃圾密文：填充校验会失败
        r#"java.createSymmetricCrypto("AES/ECB/PKCS5Padding","1234567890abcdef","").decryptStr("bm90LWEtY2lwaGVy")"#,
        // GCM 认证标签不匹配
        r#"java.createSymmetricCrypto("AES/GCM/NoPadding","0123456789abcdef","0123456789ab").decryptStr("YWJjZGVmZ2hpamtsbW5vcA==")"#,
        // 非法 base64
        r#"java.createSymmetricCrypto("AES/ECB/PKCS5Padding","1234567890abcdef","").decryptStr("!!!")"#,
    ] {
        assert_eq!(
            call(expr),
            "",
            "{expr} 是数据错误，必须返回空串而不是抛错（书源靠 || 兜底）"
        );
    }
}

/// 正常加解密往返不受影响（含 `aesEncodeToBase64String` 这条直接走 `__sym` 的路径）。
#[test]
fn crypt_round_trip_still_works() {
    assert_eq!(
        call(
            r#"java.aesBase64DecodeToString(
                java.aesEncodeToBase64String("hello","1234567890abcdef","AES/ECB/PKCS5Padding",""),
                "1234567890abcdef","AES/ECB/PKCS5Padding","")"#
        ),
        "hello"
    );
    // `createSymmetricCrypto` 的链式写法
    assert_eq!(
        call(
            r#"var c = java.createSymmetricCrypto("AES/ECB/PKCS5Padding","1234567890abcdef","");
               c.decryptStr(c.encryptBase64("中文内容"))"#
        ),
        "中文内容"
    );
    // 加密输出非空（对照：修复前配置错误会给空串）
    assert!(!call(
        r#"java.createSymmetricCrypto("AES/ECB/PKCS5Padding","1234567890abcdef","").encryptBase64("abc")"#
    )
    .is_empty());
}
