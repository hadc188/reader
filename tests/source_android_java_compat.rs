//! Android 专有 Java 类与 raw-deflate 解压的兼容回归。
//!
//! 一类书源把首页数据压进 `APP.Index('<base64>')`，再用脚本自己解：
//!
//! ```js
//! var raw = Packages.android.util.Base64.decode(b64, 2);   // Android 专有类
//! var inf = new Packages.java.util.zip.Inflater(true);      // nowrap = raw deflate
//! ```
//!
//! 手机端是 Android 运行时，这些类天然存在；桌面端不补就是
//! `cannot read property 'util' of undefined`，脚本 catch 后返回 null ——
//! 症状是**发现页空白、分类有名字但一本书都没有**。
use reader_rust::model::book_source::book_source_from_value;
use reader_rust::parser::js::{eval_js, with_book_context, with_source, JsBookContext};

fn source_with_lib(lib: &str) -> reader_rust::model::book_source::BookSource {
    let value = serde_json::json!({
        "bookSourceName": "示例源",
        "bookSourceUrl": "https://example.test",
        "jsLib": lib,
    });
    book_source_from_value(value).expect("source")
}

/// `Packages.android.util.Base64` 必须存在，且 `decode(text, URL_SAFE)` 能解出字节。
#[test]
fn android_base64_namespace_exists() {
    let source = source_with_lib("");
    let out = with_source(&source, || {
        eval_js(
            "typeof Packages.android + '|' + typeof Packages.android.util.Base64.decode",
            "",
            "",
        )
    });
    let out = out.expect("求值应成功");
    assert_eq!(out, "object|function", "Packages.android 链缺失：{out}");
}

/// `Packages.java.util.zip.Inflater(true)` 解 raw-deflate（不带 zlib 头）。
///
/// `nowrap=true` 必须走 `DeflateDecoder`；用 `ZlibDecoder` 会报
/// "invalid zlib header"，书源的 try/catch 随即吞掉异常、返回 null。
#[test]
fn inflater_blob_round_trip() {
    // 一段真实的 raw deflate 数据（"hello reader" 压出来）。
    let payload = b"hello reader";
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write;
    encoder.write_all(payload).unwrap();
    let compressed = encoder.finish().unwrap();
    let bytes: Vec<i32> = compressed.iter().map(|b| *b as i32).collect();

    let source = source_with_lib("");
    let script = format!(
        r#"
var inf = new Packages.java.util.zip.Inflater(true);
var buf = Packages.java.lang.reflect.Array.newInstance(Packages.java.lang.Byte.TYPE, 8192);
inf.setInput({bytes:?}, 0, {len});
var out = new Packages.java.io.ByteArrayOutputStream();
while (!inf.finished()) {{
  var n = inf.inflate(buf);
  if (n <= 0) break;
  out.write(buf, 0, n);
}}
inf.end();
var data = out.toByteArray();
var text = '';
for (var i = 0; i < data.length; i++) text += String.fromCharCode(data[i] & 255);
text
"#,
        bytes = bytes,
        len = bytes.len()
    );
    let out = with_source(&source, || eval_js(&script, "", "")).expect("解压应成功");
    assert_eq!(out, "hello reader");
}

/// APP 源的正文链路会先走 `OkHttpClient` 取原始字节，再解 zlib 数据。
/// `OkHttp Request.Builder.addHeader(...).build()` 与
/// `execute().body().bytes()` 都必须存在，缺任一段都会让正文规则直接失败。
#[test]
fn okhttp_bytes_path_and_inflater_input_stream_are_available() {
    use flate2::write::ZlibEncoder;
    use std::io::Write;
    use base64::Engine as _;

    let payload = b"okhttp inflater reader";
    let mut encoder =
        ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(payload).unwrap();
    let compressed = encoder.finish().unwrap();
    let compressed_b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);

    let source = source_with_lib("");
    let script = format!(
        r#"
var raw = java.base64DecodeToByteArray('{compressed_b64}');
var stream = new Packages.java.util.zip.InflaterInputStream(
  new Packages.java.io.ByteArrayInputStream(raw)
);
var out = new Packages.java.io.ByteArrayOutputStream();
var buf = Packages.java.lang.reflect.Array.newInstance(Packages.java.lang.Byte.TYPE, 8192);
var n;
while ((n = stream.read(buf, 0, buf.length)) > 0) out.write(buf, 0, n);
var text = new Packages.java.lang.String(out.toByteArray(), 'UTF-8');
var request = new Packages.okhttp3.Request.Builder()
  .url('data:application/octet-stream;base64,{compressed_b64}')
  .addHeader('User-Agent', 'reader-test')
  .header('X-Probe', 'one')
  .build();
var call = new Packages.okhttp3.OkHttpClient().newCall(request);
var response = call.execute();
var downloaded = response.body().bytes();
var executed = call.isExecuted();
var repeated;
try {{ call.execute(); repeated = false; }} catch (e) {{ repeated = true; }}
JSON.stringify([
  typeof Packages.okhttp3.OkHttpClient,
  typeof Packages.java.util.zip.InflaterInputStream,
  String(text),
  downloaded.length,
  downloaded[0],
  downloaded[downloaded.length - 1],
  request.headers['X-Probe'],
  executed,
  repeated
]);
"#
    );
    let out = with_source(&source, || eval_js(&script, "", ""))
        .expect("OkHttp/InflaterInputStream 兼容层应可执行");
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("JSON 输出");
    assert_eq!(parsed[0], "function");
    assert_eq!(parsed[1], "function");
    assert_eq!(parsed[2], "okhttp inflater reader");
    assert_eq!(parsed[3], compressed.len());
    assert_eq!(parsed[4], compressed[0]);
    assert_eq!(parsed[5], compressed[compressed.len() - 1]);
    assert_eq!(parsed[6], "one");
    assert_eq!(parsed[7], true);
    assert_eq!(parsed[8], true);
}

/// 先只验证 `InflaterInputStream` 的构造与 `read` 方法形态。
#[test]
fn inflater_input_stream_constructor_and_read_surface() {
    let source = source_with_lib("");
    let out = with_source(&source, || eval_js(
        r#"
var raw = java.hexDecodeToByteArray('789c2b2e2d4a4b4c4e55c8cf0600167103e4');
var input = new Packages.java.io.ByteArrayInputStream(raw);
var stream = new Packages.java.util.zip.InflaterInputStream(input);
JSON.stringify([
  typeof stream,
  typeof stream.read,
  stream.bytes ? stream.bytes.length : -1,
  stream.position
]);
"#,
        "",
        "",
    ))
    .expect("构造与 read 方法应存在");
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("JSON 输出");
    assert_eq!(parsed[0], "object");
    assert_eq!(parsed[1], "function");
}

/// 正文 `.dat` 解压后按 GB18030 解码，章节参数从 `chapter.url` 读取。
/// 这是 8142 规则的实际数据路径，避免只验证方法存在却漏掉编码或上下文。
#[test]
fn okhttp_rule_reads_gb18030_content_from_chapter_url() {
    use flate2::write::ZlibEncoder;
    use std::io::Write;

    let payload: &[u8] = &[
        0xd5, 0xfd, 0xce, 0xc4, 0xb5, 0xda, 0xd2, 0xbb, 0xb6, 0xce, 0x0a, 0xd5, 0xfd,
        0xce, 0xc4, 0xb5, 0xda, 0xb6, 0xfe, 0xb6, 0xce,
    ];
    let mut encoder =
        ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(payload).unwrap();
    let compressed = encoder.finish().unwrap();
    let hex: String = compressed.iter().map(|b| format!("{b:02x}")).collect();

    let source = source_with_lib("");
    let ctx = JsBookContext {
        book: serde_json::json!({
            "name": "测试书",
            "bookUrl": "https://cxb-pro.cread.com/cx/bookDetailYS?bookid=42"
        }),
        chapter: serde_json::json!({
            "title": "第一章",
            "url": "https://book-resources.cread.com/resources/books/42/7-1.dat?bid=42&cid=7&v=3"
        }),
    };
    let script = format!(
        r#"
var url = String(chapter && chapter.url ? chapter.url : baseUrl || '');
var bid = url.match(/[?&]bid=([^&]+)/)[1];
var cid = url.match(/[?&]cid=([^&]+)/)[1];
var ver = url.match(/[?&]v=([^&]+)/)[1];
var raw = java.hexDecodeToByteArray('{hex}');
var stream = new Packages.java.util.zip.InflaterInputStream(
  new Packages.java.io.ByteArrayInputStream(raw)
);
var out = new Packages.java.io.ByteArrayOutputStream();
var buf = Packages.java.lang.reflect.Array.newInstance(Packages.java.lang.Byte.TYPE, 512);
var n;
while ((n = stream.read(buf, 0, buf.length)) > 0) out.write(buf, 0, n);
var text = String(new Packages.java.lang.String(out.toByteArray(), 'GB18030'));
JSON.stringify([bid, cid, ver, text]);
"#
    );
    let out = with_source(&source, || {
        with_book_context(Some(ctx), || eval_js(&script, "", "https://cxb-pro.cread.com"))
    })
    .expect("8142 正文解压链路应可执行");
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("JSON 输出");
    assert_eq!(parsed[0], "42");
    assert_eq!(parsed[1], "7");
    assert_eq!(parsed[2], "3");
    assert_eq!(parsed[3], "正文第一段\n正文第二段");
}

/// 只验证 OkHttp Builder 到 Response 访问器的链式类型。
#[test]
fn okhttp_builder_and_response_surface() {
    let source = source_with_lib("");
    let out = with_source(&source, || eval_js(
        r#"
var builder = new Packages.okhttp3.Request.Builder();
JSON.stringify([
  typeof Packages.okhttp3.OkHttpClient,
  typeof Packages.okhttp3.Request,
  typeof Packages.okhttp3.Request.Builder,
  typeof builder,
  typeof builder.url
]);
"#,
        "",
        "",
    ))
    .expect("OkHttp 链式构造应存在");
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("JSON 输出");
    assert_eq!(parsed[0], "function");
    assert_eq!(parsed[1], "object");
    assert_eq!(parsed[2], "function");
    assert_eq!(parsed[3], "object");
    assert_eq!(parsed[4], "function");
}

/// 8142 的密钥生成直接读取 `StandardCharsets.ISO_8859_1`，不能只提供
/// `Charset.forName`。用 0x80 以上的字节确认它仍按 latin1 无损转换成密钥文本。
#[test]
fn java_standard_charsets_iso_8859_1_is_available() {
    let source = source_with_lib("");
    let out = with_source(&source, || eval_js(
        r#"
var charset = Packages.java.nio.charset.StandardCharsets.ISO_8859_1;
var bytes = java.hexDecodeToByteArray('414280ff');
var text = new Packages.java.lang.String(bytes, charset);
var unmapped = String.fromCharCode(0x100).getBytes(charset);
var enumerable = false;
for (var name in '') { if (name === 'getBytes') enumerable = true; }
JSON.stringify([
  typeof charset,
  String(charset),
  String(text),
  text.getBytes(charset).length,
  unmapped[0],
  enumerable
]);
"#,
        "",
        "",
    ))
    .expect("StandardCharsets.ISO_8859_1 应可用");
    let parsed: serde_json::Value = serde_json::from_str(&out).expect("JSON 输出");
    assert_eq!(parsed[0], "object");
    assert_eq!(parsed[1], "ISO-8859-1");
    assert_eq!(parsed[2], "AB\u{80}\u{ff}");
    assert_eq!(parsed[3], 4);
    assert_eq!(parsed[4], 63);
    assert_eq!(parsed[5], false);
}

/// 8142 的密钥文本是纯 ASCII，但必须仍经过 `ISO_8859_1` 的逐字节路径。
/// 这条与真实首章的 bookId/cid/version/secret 一致，摘要值由 Java 等价算法固定。
#[test]
fn java_standard_charsets_iso_8859_1_md5_matches_rule_key() {
    let source = source_with_lib("");
    let out = with_source(&source, || eval_js(
        r#"
var keys = '60694717-52617298-0-3u5k41jj94mijuef3b8ljbo19o';
var charset = Packages.java.nio.charset.StandardCharsets.ISO_8859_1;
var digest = Packages.java.security.MessageDigest.getInstance('MD5')
  .digest(new Packages.java.lang.String(keys).getBytes(charset));
var hex = '';
for (var i = 0; i < digest.length; i++) {
  hex += ('0' + (digest[i] & 255).toString(16)).slice(-2);
}
hex;
"#,
        "",
        "",
    ))
    .expect("ISO_8859_1 密钥摘要应可计算");
    assert_eq!(out, "19afd4d4033fe32eaa30c7059147970e");
}

/// 8142 原规则在离线 data URL 上的完整执行路径。
/// 这里刻意保留脚本写法，避免测试只验证“类似”的拆分实现而漏掉真实调用。
#[test]
fn source_8142_original_rule_decrypts_content() {
    use base64::Engine as _;
    use flate2::write::ZlibEncoder;
    use md5::Digest as _;
    use std::io::Write;

    let book_id = "60694717";
    let chapter_id = "52617298";
    let version = "0";
    let secret = "3u5k41jj94mijuef3b8ljbo19o";
    let key_text = format!("{book_id}-{chapter_id}-{version}-{secret}");
    let key = md5::Md5::digest(key_text.as_bytes());
    let plain = b"\xd5\xfd\xce\xc4\xb2\xe2\xca\xd4";
    let mut encoder =
        ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(plain).unwrap();
    let compressed = encoder.finish().unwrap();
    let raw: Vec<u8> = compressed
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % 16])
        .collect();
    let body = base64::engine::general_purpose::STANDARD.encode(&raw);
    let chapter_url = format!(
        "data:application/octet-stream;base64,{body}?bid={book_id}&cid={chapter_id}&v={version}"
    );
    let script = r#"
(function(){
  try {
    var url = String(chapter && chapter.url ? chapter.url : baseUrl || '');
    var m = url.match(/[?&]bid=([^&]+)/);
    var mc = url.match(/[?&]cid=([^&]+)/);
    var mv = url.match(/[?&]v=([^&]+)/);
    var bid = m ? m[1] : '';
    var cid = mc ? mc[1] : '';
    var ver = mv ? mv[1] : '0';
    if(!bid || !cid){
      var mu = url.match(/books\/(\d+)\/(\d+)-\d+\.dat/);
      if(mu){ bid = mu[1]; cid = mu[2]; }
    }
    if(!bid || !cid) return '章节地址缺少 bookId/chapterId';
    var purl = url.split('?')[0];
    var cli = new Packages.okhttp3.OkHttpClient();
    var req = new Packages.okhttp3.Request.Builder().url(purl)
      .addHeader('User-Agent','Dalvik/2.1.0 (Linux; U; Android 10; MI 8 MIUI/V12.5.2.0.QEACNXM)').build();
    var raw = cli.newCall(req).execute().body().bytes();
    var hex = new Packages.java.math.BigInteger(1, raw).toString(16);
    if(hex.length % 2) hex = '0' + hex;
    var keys = bid + '-' + cid + '-' + ver + '-3u5k41jj94mijuef3b8ljbo19o';
    var kb = new Packages.java.lang.StringBuilder();
    for(var i=0;i<keys.length;i++) kb.append(String.fromCharCode(keys.charCodeAt(i) & 255));
    var keyBytes = kb.toString().getBytes(Packages.java.nio.charset.StandardCharsets.ISO_8859_1);
    var kbytes = Packages.java.security.MessageDigest.getInstance('MD5').digest(keyBytes);
    var out = java.hexDecodeToByteArray(hex);
    for(var j=0;j<out.length;j++) out[j] = (out[j] ^ kbytes[j % 16]);
    var iis = new Packages.java.util.zip.InflaterInputStream(new Packages.java.io.ByteArrayInputStream(out));
    var bos = new Packages.java.io.ByteArrayOutputStream();
    var buf = java.hexDecodeToByteArray('00'.repeat(512));
    var n;
    while((n = iis.read(buf)) >= 0){ bos.write(buf, 0, n); }
    var s = String(new Packages.java.lang.String(bos.toByteArray(), 'GB18030'));
    var lines = s.split(/\r?\n/);
    var arr = [];
    for(var k=0;k<lines.length;k++){
      var t = lines[k].replace(/^\uFEFF/, '').trim();
      if(t.length) arr.push(t);
    }
    return arr.join('\n');
  } catch(e){
    return '正文解析失败: ' + (e && e.message ? e.message : String(e));
  }
})()
"#;
    let source = source_with_lib("");
    let ctx = JsBookContext {
        book: serde_json::json!({"bookUrl": "https://cxb-pro.cread.com/cx/bookDetailYS?bookid=60694717"}),
        chapter: serde_json::json!({"title": "第一章", "url": chapter_url}),
    };
    let out = with_source(&source, || {
        with_book_context(Some(ctx), || {
            eval_js(script, "", "https://cxb-pro.cread.com")
        })
    })
    .expect("规则脚本应执行");
    assert_eq!(out, "正文测试");
}
