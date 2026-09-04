use md5::{Digest, Md5};

pub fn md5_hex(input: &str) -> String {
    md5_hex_bytes(input.as_bytes())
}

/// 二进制内容直接哈希 (文件字节等), 不经 UTF-8 转换。
pub fn md5_hex_bytes(input: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(input);
    let result = hasher.finalize();
    hex::encode(result)
}
