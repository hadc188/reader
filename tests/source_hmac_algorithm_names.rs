//! `java.HMacHex` / `java.HMacBase64` 的算法名归一化（issue #29）。
//!
//! legado 的 `HMacHex(data, algorithm, key)` 底层是
//! `javax.crypto.Mac.getInstance(algorithm)`，规范名是 **JCA 写法**
//! （`HmacSHA1` / `HmacSHA256` / `HmacMD5` …）；而书源里也常混着摘要名
//! （`SHA-1` / `SHA256`）。两边都得认。
//!
//! 早先只有 `Packages.javax.crypto.Mac` 那条字节路径剥了 `HMAC` 前缀，
//! `java.HMacHex` 走的 `java_hmac` 没剥：
//!
//! | 调用 | 修复前 | 修复后 |
//! | --- | --- | --- |
//! | `HMacHex("abc", "HmacSHA1", "key")` | `""` | `4fd0b215…` |
//! | `HMacHex("abc", "SHA-1", "key")` | `4fd0b215…` | `4fd0b215…` |
//!
//! 空签名不会抛错、不会打日志，书源只是被服务端回 `Invalid Signature` —— 这也是
//! 这个 bug 值得一条专门回归的原因。
use reader_rust::parser::js::eval_js;

/// 固定向量：`HMAC-SHA1(key="key", data="abc")`。
const HMAC_SHA1_HEX: &str = "4fd0b215276ef12f2b3e4c8ecac2811498b656fc";
const HMAC_SHA1_B64: &str = "T9CyFSdu8S8rPkyOysKBFJi2Vvw=";

fn call(expr: &str) -> String {
    eval_js(expr, "", "").unwrap_or_else(|err| panic!("{expr} 求值失败: {err}"))
}

/// JCA 规范名必须能算出结果（修复前是空串）。
#[test]
fn jca_algorithm_names_produce_digests() {
    assert_eq!(
        call(r#"java.HMacHex("abc", "HmacSHA1", "key")"#),
        HMAC_SHA1_HEX,
        "HmacSHA1 是 Mac.getInstance 的规范名，必须支持"
    );
    assert_eq!(
        call(r#"java.HMacBase64("abc", "HmacSHA1", "key")"#),
        HMAC_SHA1_B64
    );
    // 大小写不敏感
    assert_eq!(
        call(r#"java.HMacHex("abc", "hmacsha1", "key")"#),
        HMAC_SHA1_HEX
    );
}

/// 摘要名写法继续可用（不少书源这么写，不能为了 JCA 名把它们弄坏）。
#[test]
fn digest_algorithm_names_still_work() {
    assert_eq!(call(r#"java.HMacHex("abc", "SHA-1", "key")"#), HMAC_SHA1_HEX);
    assert_eq!(call(r#"java.HMacHex("abc", "sha1", "key")"#), HMAC_SHA1_HEX);
    assert_eq!(call(r#"java.HMacBase64("abc", "SHA-1", "key")"#), HMAC_SHA1_B64);
}

/// JCA 允许 `HmacSHA-256` 这种带连字符的写法；各摘要算法都要认。
#[test]
fn all_hmac_algorithms_accept_both_naming_styles() {
    for (algo, digest_name, hex_len) in [
        ("HmacMD5", "MD5", 32usize),
        ("HmacSHA1", "SHA-1", 40),
        ("HmacSHA224", "SHA-224", 56),
        ("HmacSHA256", "SHA-256", 64),
        ("HmacSHA384", "SHA-384", 96),
        ("HmacSHA512", "SHA-512", 128),
    ] {
        let out = call(&format!(r#"java.HMacHex("abc", "{algo}", "key")"#));
        assert_eq!(out.len(), hex_len, "{algo} 摘要长度不对: {out:?}");
        assert!(
            out.chars().all(|c| c.is_ascii_hexdigit()),
            "{algo} 应返回十六进制: {out:?}"
        );

        // 同一算法的摘要名写法应给出**相同**结果。
        let alt = call(&format!(r#"java.HMacHex("abc", "{digest_name}", "key")"#));
        assert_eq!(alt, out, "{algo} 与 {digest_name} 应等价");
    }

    // `HmacSHA-256` 与 `HmacSHA256` 等价（JCA 的连字符变体）。
    assert_eq!(
        call(r#"java.HMacHex("abc", "HmacSHA-256", "key")"#),
        call(r#"java.HMacHex("abc", "HmacSHA256", "key")"#)
    );
}

/// 未知算法要**抛错**，而不是静默返回空串。
///
/// legado 的 `Mac.getInstance(未知名)` 抛 `NoSuchAlgorithmException`；这里给等价的
/// 明确报错，书源调试面板能直接指出算法名写错。
#[test]
fn unknown_algorithm_throws_instead_of_returning_empty() {
    for expr in [
        r#"java.HMacHex("abc", "HmacSHA999", "key")"#,
        r#"java.HMacBase64("abc", "bogus", "key")"#,
    ] {
        let err = eval_js(expr, "", "").expect_err(&format!("{expr} 应抛错而不是返回空串"));
        let message = err.to_string();
        assert!(
            message.contains("不支持 HMAC 算法"),
            "报错应指出算法不支持，实际: {message}"
        );
    }

    // 脚本里可以用 try/catch 接住（书源常见的降级写法）。
    let out = call(
        r#"try { java.HMacHex("abc","bogus","key") } catch (e) { "caught" }"#,
    );
    assert_eq!(out, "caught");
}

/// 字节版路径（`Packages.javax.crypto.Mac`）与 `java.HMacHex` 必须同源同结果。
///
/// 这两条 HMAC 路径曾经各自解析算法名，只有字节版剥了 `HMAC` 前缀 —— issue #29
/// 的根因。这条用例盯住"两边对同一个算法名给出同一个摘要"。
#[test]
fn byte_path_and_string_path_agree_on_jca_names() {
    let out = call(
        r#"
var viaJava = String(java.HMacHex("abc", "HmacSHA256", "key"));
var mac = Packages.javax.crypto.Mac.getInstance("HmacSHA256");
mac.init(new Packages.javax.crypto.spec.SecretKeySpec(java.strToBytes("key"), "HmacSHA256"));
var bytes = mac.doFinal(java.strToBytes("abc"));
var hex = "";
for (var i = 0; i < bytes.length; i++) {
  var b = Number(bytes[i]) & 0xff;
  hex += (b < 16 ? "0" : "") + b.toString(16);
}
viaJava === hex ? "same" : (viaJava + " vs " + hex)
"#,
    );
    assert_eq!(out, "same", "两条 HMAC 路径对 HmacSHA256 应算出同一摘要");
}
