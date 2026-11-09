pub mod book;
pub mod book_group;
pub mod book_source;
pub mod bookmark;
pub mod backup;
pub mod debug;
pub mod edge_tts;
pub mod export;
pub mod http_tts;
pub mod ms_demo_tts;
pub mod network;
pub mod reading_stats;
pub mod replace_rule;
pub mod rss;
pub mod source_click;
pub mod speech;
pub mod update;
pub mod user;
pub mod webdav;
pub mod window;
pub mod xmnote;

use crate::error::error::{ApiResponse, AppError};
use std::borrow::Cow;

/// 承载上传文件名的 IPC header，与前端 `FILE_NAME_HEADER` 必须一致。
pub const FILE_NAME_HEADER: &str = "x-reader-file-name";

/// 还原 percent-encoded 文件名（纯函数，便于单测）。
pub fn parse_file_name_header(raw: &str) -> Result<String, AppError> {
    let decoded = urlencoding::decode(raw)
        .map_err(|_| AppError::BadRequest("文件名编码无效".to_string()))?
        .into_owned();
    let safe = decoded.trim();
    if safe.is_empty() {
        return Err(AppError::BadRequest("文件名不能为空".to_string()));
    }
    Ok(safe.to_string())
}

/// 从 IPC header 取文件名并还原 percent-encoding。
///
/// 前端把文件名放在 header 而不是 payload：payload 必须留给出原始字节（走
/// octet-stream 快速通道），一旦把字节和文件名打包成 JSON 对象，Tauri 会把
/// `Uint8Array` 逐元素序列化成十进制数字数组（见 frontend/src/api/invoke.ts
/// 的 `invokeUpload` 注释）。
///
/// header 值只能承载 ASCII，所以中文名在前端已 `encodeURIComponent`。
pub fn decode_header_file_name(request: &tauri::ipc::Request<'_>) -> Result<String, AppError> {
    let raw = request
        .headers()
        .get(FILE_NAME_HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::BadRequest("缺少文件名信息".to_string()))?;
    parse_file_name_header(raw)
}

/// 提取上传的原始字节。
///
/// 正常路径（custom protocol 可用）下 body 就是文件字节本身，这里借用零拷贝：
/// 各上传处理只按 `&[u8]` 使用，上传上限允许 100MB，多一次 clone 就是多一份
/// 整块内存与拷贝耗时。
///
/// 兜底：custom protocol 失败时 Tauri 会回落到 postMessage，那条路径把整个
/// message 做 JSON 序列化（tauri 的 ipc-protocol.js:66-85），payload 里的
/// `Uint8Array` 被降级成数字数组。此时仍按 `Vec<u8>` 还原 —— 慢一些，但不会
/// 让上传直接失败。
pub fn decode_upload_body(body: &tauri::ipc::InvokeBody) -> Result<Cow<'_, [u8]>, AppError> {
    match body {
        tauri::ipc::InvokeBody::Raw(bytes) => Ok(Cow::Borrowed(bytes.as_slice())),
        tauri::ipc::InvokeBody::Json(value) => serde_json::from_value::<Vec<u8>>(value.clone())
            .map(Cow::Owned)
            .map_err(|_| AppError::BadRequest("上传请求必须以二进制 body 发送".to_string())),
    }
}

#[tauri::command]
pub async fn health() -> Result<ApiResponse<&'static str>, AppError> {
    Ok(ApiResponse::ok("ok"))
}

#[cfg(test)]
mod tests {
    use super::{decode_upload_body, parse_file_name_header};

    /// 文件名走 IPC header，前端 `encodeURIComponent`、这里还原，两者必须严格互逆。
    /// 用例里的 encoded 值都是 `encodeURIComponent` 的真实输出（见每条右侧注释）。
    #[test]
    fn file_name_header_round_trips_encoded_names() {
        let cases = [
            ("%E5%8F%98%E5%BD%A2%E8%AE%B0.txt", "变形记.txt"),
            ("My%20Book.txt", "My Book.txt"),
            // encodeURIComponent('+') === '%2B' —— 还原成 '+'，不是空格
            ("a%2Bb.txt", "a+b.txt"),
            ("100%25.txt", "100%.txt"),
            ("a%26b%23c.txt", "a&b#c.txt"),
            ("%E4%B9%A6.epub", "书.epub"),
            // 这些字符 encodeURIComponent 都不转义，两侧都应原样保留
            ("a!~*'()-_.txt", "a!~*'()-_.txt"),
        ];
        for (encoded, expected) in cases {
            assert_eq!(
                parse_file_name_header(encoded).expect("should decode"),
                expected,
                "encoded input: {encoded}"
            );
        }
    }

    /// 裸 `+` 不经 encodeURIComponent 时也原样保留 —— urlencoding::decode 明确
    /// 不把 `+` 当空格（urlencoding-2.1.3/src/dec.rs:18）。这条守住的是解码侧
    /// 行为，与上一条的编码侧往返互补。
    #[test]
    fn file_name_header_keeps_bare_plus() {
        assert_eq!(parse_file_name_header("a+b.txt").unwrap(), "a+b.txt");
    }

    /// 纯 ASCII 名（无 % 转义）走 Borrowed 分支，同样要原样返回。
    #[test]
    fn file_name_header_accepts_plain_ascii() {
        assert_eq!(parse_file_name_header("novel.txt").unwrap(), "novel.txt");
    }

    /// 空白文件名要拒绝；首尾空白应被裁掉而不是当成非法。
    #[test]
    fn file_name_header_rejects_blank_and_trims() {
        assert!(parse_file_name_header("").is_err());
        assert!(parse_file_name_header("   ").is_err());
        assert!(parse_file_name_header("%20%20").is_err());
        assert_eq!(parse_file_name_header("%20book.txt%20").unwrap(), "book.txt");
    }

    /// 正常路径：body 是 Raw，零拷贝借用。
    /// 降级路径：custom protocol 不可用时 Tauri 回落 postMessage，payload 变成
    /// JSON 数字数组（tauri 的 ipc-protocol.js:66-85），这里要能还原而不是报错。
    #[test]
    fn upload_body_reads_raw_and_falls_back_to_number_array() {
        let raw = tauri::ipc::InvokeBody::Raw(vec![1u8, 2, 3]);
        assert_eq!(&*decode_upload_body(&raw).unwrap(), &[1u8, 2, 3]);

        // 数字数组（postMessage 降级路径的真实形态）
        let json = tauri::ipc::InvokeBody::Json(serde_json::json!([1, 2, 3]));
        assert_eq!(&*decode_upload_body(&json).unwrap(), &[1u8, 2, 3]);

        // 既不是字节也不是字节数组的东西要报错，而不是被当文件收下
        let bogus = tauri::ipc::InvokeBody::Json(serde_json::json!({ "file": [1, 2, 3] }));
        assert!(decode_upload_body(&bogus).is_err());

        // 越界数字（>255）同样要报错，不能静默截断
        let out_of_range = tauri::ipc::InvokeBody::Json(serde_json::json!([256]));
        assert!(decode_upload_body(&out_of_range).is_err());
    }
}
