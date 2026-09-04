//! 提取耗时基准: `cargo run --example pdf_timing -- <a.pdf> [b.pdf ...]`
use std::time::Instant;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: pdf_timing <file.pdf> [more.pdf ...]");
        std::process::exit(2);
    }
    for path in paths {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{path}: 读取失败: {e}");
                continue;
            }
        };
        let t = Instant::now();
        match pdf_extract::extract_text_from_mem_by_pages(&bytes) {
            Ok(pages) => println!(
                "{}: {} pages, {} chars, total {:?}",
                path,
                pages.len(),
                pages.iter().map(|p| p.chars().count()).sum::<usize>(),
                t.elapsed()
            ),
            Err(e) => eprintln!("{path}: 提取失败: {e:?}"),
        }
    }
}
