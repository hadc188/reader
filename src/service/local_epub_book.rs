use crate::error::error::AppError;
use crate::model::{book::Book, book_chapter::BookChapter};
use quick_xml::events::{attributes::Attribute, BytesStart, Event};
use quick_xml::Reader;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokio::fs;

pub const LOCAL_EPUB_ORIGIN: &str = "local-epub";
pub const LOCAL_EPUB_ORIGIN_NAME: &str = "本地 EPUB";
pub const MAX_EPUB_UPLOAD_BYTES: usize = 80 * 1024 * 1024;
const MAX_EPUB_UNPACKED_BYTES: u64 = 300 * 1024 * 1024;
const MAX_EPUB_FILE_COUNT: usize = 3_000;
const LOCAL_BOOK_DIR: &str = "local_books";
const LOCAL_EPUB_HASH_LEN: usize = 32;

/// 书里没有 nav/ncx 时, 从每章开头读多少字节来提取章节名。
const TITLE_SCAN_BYTES: usize = 8 * 1024;

/// 为发现"漏登记的 CSS", 导入时最多扫几章 XHTML 的 `<link>`。
const HTML_STYLE_SCAN_COUNT: usize = 8;

/// 单个 zip 条目允许解压出的最大字节数(实际读取量, 不看中央目录的声明值)。
const MAX_EPUB_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

/// 无 `<link>` 章节使用的通用图片样式在 `styles/` 下的文件名。
const IMAGE_BASE_FILE: &str = "__image-base.css";

/// 解析产物的版本号。读取索引时版本低于它就用 `original.epub` 就地重建。
///
/// 只靠"`stylesheets` 是否为空"判断是不够的: 解析规则后续再有改进(例如字数
/// 统计修正), 已经重建过一次的书不会再触发, 旧结果会被永久保留。
/// 1 = 清洗 XHTML 但不保留原书样式; 2 = 保留作用域化 CSS、SVG 封面、修正字数;
/// 3 = 正文改为按需解压(导入不再逐章生成 HTML); 4 = 不再保留原书 font-family,
/// 字体交给阅读器设置。
const CURRENT_EPUB_ENGINE_VERSION: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredEpubChapter {
    title: String,
    url: String,
    index: i32,
    /// 该章在 `original.epub` 里的路径(v3 起)。正文不再预先落盘, 翻到这一章时才解压。
    ///
    /// `default` 是为了读得进 v2 及更早的索引: 那些章节只有 `file_name`。
    #[serde(default)]
    path: String,
    /// v2 及更早版本落盘的章节文件名。只在没有 `path`(无法从 zip 取) 时兜底。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredEpubAsset {
    path: String,
    content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredEpubIndex {
    book_url: String,
    name: String,
    author: String,
    file_name: String,
    byte_len: usize,
    /// 正文字数。正文改为按需解压后导入阶段不再统计, 因此通常为 0(表示未知)。
    #[serde(default)]
    char_len: usize,
    cover_url: Option<String>,
    chapters: Vec<StoredEpubChapter>,
    assets: Vec<StoredEpubAsset>,
    /// 生成这份索引时的解析器版本; 旧索引缺少该字段时按 0 处理并触发重建。
    #[serde(default)]
    engine_version: u32,
}

#[derive(Debug, Clone)]
struct EpubManifestItem {
    path: String,
    media_type: String,
    properties: String,
}

#[derive(Debug, Clone, Default)]
struct EpubPackage {
    title: Option<String>,
    author: Option<String>,
    manifest: HashMap<String, EpubManifestItem>,
    spine: Vec<String>,
    cover_id: Option<String>,
    nav_id: Option<String>,
    ncx_id: Option<String>,
    /// `<meta name="cover" content="...">` 的值。EPUB 2 里它通常写的是某张图片的
    /// **manifest id**, 但也常见直接写文件名, 两种都要认。
    cover_meta: Option<String>,
    /// 无 OPF 声明时按图片顺序回退的候选(见 `resolve_cover_path`)。
    image_items: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct NavPointState {
    label: String,
}

#[derive(Debug, Clone)]
pub struct LocalEpubAsset {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

/// 已打开的 zip 句柄。`ZipArchive` 不实现 `Clone`, 用 `Arc<Mutex<_>>` 共享;
/// 章节读取是同步解压(毫秒级), 持锁期间不 await。
type EpubArchiveHandle = Arc<tokio::sync::Mutex<zip::ZipArchive<File>>>;

/// 同时缓存的已打开 epub 数量上限; 超出后淘汰一个。
///
/// 被淘汰条目的文件句柄不会立刻关闭 —— 若有正在进行的章节读取仍持有 `Arc`,
/// 要等那次读取结束、引用释放后才关。
const EPUB_ARCHIVE_CACHE_LIMIT: usize = 4;

#[derive(Clone)]
pub struct LocalEpubBookService {
    storage_dir: PathBuf,
    /// 串行化"就地重建索引"。同一本旧书被并发读取时(目录 + 正文 + 封面会各自
    /// 读一次 manifest), 没有它就会重复全量解包。
    ///
    /// 注意它只互斥重建者之间: 读取路径为了不阻塞并不加锁, 因此重建写入必须走
    /// `write_file_atomic`(见该函数), 否则并发读会拿到半截 manifest。
    rebuild_lock: Arc<tokio::sync::Mutex<()>>,
    /// 按 book_url 缓存的已打开 zip。正文改为按需解压后, 每次翻章都要从
    /// `original.epub` 取一个条目; 若每次重新打开, 光解析 1800 多个条目的中央
    /// 目录就要上百毫秒 —— 缓存句柄后单章读取回到毫秒级。
    archives: Arc<tokio::sync::Mutex<HashMap<String, EpubArchiveHandle>>>,
    /// 已解析的 `manifest.json`。
    ///
    /// 每次翻章都要按章号查路径, 而 manifest 含全书章节 + assets 清单(上千条目,
    /// 几百 KB JSON)。逐次读文件 + 反序列化会让本地书换章多出几十毫秒 ——
    /// 正是"本地书翻页应该无感"被打断的那部分。import / rebuild / 删书时失效。
    indexes: Arc<tokio::sync::Mutex<HashMap<String, Arc<StoredEpubIndex>>>>,
}

/// 同时缓存的本地 EPUB 目录数量上限。
const LOCAL_EPUB_INDEX_CACHE_LIMIT: usize = 8;

pub fn is_local_epub_origin(value: &str) -> bool {
    value.trim() == LOCAL_EPUB_ORIGIN
}

pub fn is_local_epub_url(value: &str) -> bool {
    value.trim().starts_with("local-epub:")
}

pub fn build_epub_chapter_url(book_url: &str, index: usize) -> String {
    format!("{}#{}", book_url.trim_end_matches('#'), index)
}

impl LocalEpubBookService {
    pub fn new(storage_dir: impl AsRef<Path>) -> Self {
        Self {
            storage_dir: storage_dir.as_ref().to_path_buf(),
            rebuild_lock: Arc::new(tokio::sync::Mutex::new(())),
            archives: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            indexes: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// 取(必要时打开并缓存)某本书的 zip 句柄。
    async fn archive_for(
        &self,
        book_url: &str,
        book_dir: &Path,
    ) -> Result<EpubArchiveHandle, AppError> {
        if let Some(handle) = self.archives.lock().await.get(book_url) {
            return Ok(handle.clone());
        }
        // 打开 zip 含解析中央目录, 一本 1800 章的书要上百毫秒; 放在锁外做, 免得
        // 卡住其他书的章节读取。并发重复打开最多多一次, 没有正确性问题。
        let file =
            File::open(book_dir.join("original.epub")).map_err(map_local_epub_read_error)?;
        let archive = zip::ZipArchive::new(file)
            .map_err(|_| AppError::BadRequest("EPUB 文件损坏".to_string()))?;
        let handle = Arc::new(tokio::sync::Mutex::new(archive));

        let mut cache = self.archives.lock().await;
        // 期间可能已被另一个请求放进去了, 以先到的为准。
        if let Some(existing) = cache.get(book_url) {
            return Ok(existing.clone());
        }
        if cache.len() >= EPUB_ARCHIVE_CACHE_LIMIT {
            // 只淘汰一个: 整体清空会让"同时打开超过上限本书"时每次切换都重开 zip。
            if let Some(evicted) = cache.keys().next().cloned() {
                cache.remove(&evicted);
            }
        }
        cache.insert(book_url.to_string(), handle.clone());
        Ok(handle)
    }

    /// 书文件被重写/删除后必须丢弃缓存句柄: 否则会读到旧内容, 并让 Windows 上
    /// 的目录删除因句柄占用而失败。
    async fn invalidate_archive(&self, book_url: &str) {
        self.archives.lock().await.remove(book_url);
    }

    /// 从 `original.epub` 解压一章的 XHTML。正文不预先落盘, 因此这一步是导入之后
    /// 唯一的解压开销, 每次只解一章。
    async fn read_chapter_html(
        &self,
        book_url: &str,
        book_dir: &Path,
        chapter_path: &str,
    ) -> Result<String, AppError> {
        let handle = self.archive_for(book_url, book_dir).await?;
        let mut archive = handle.lock().await;
        let bytes = read_zip_entry(&mut archive, chapter_path)?
            .ok_or_else(|| AppError::BadRequest("EPUB 章节不存在".to_string()))?;
        drop(archive);
        Ok(decode_utf8_lossy(&bytes))
    }

    pub async fn import_epub_book(
        &self,
        user_ns: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<Book, AppError> {
        validate_epub_upload(file_name, bytes.len())?;
        let safe_file_name = sanitize_epub_file_name(file_name);
        let hash = md5_bytes_hex(format!("{}:{}:", user_ns, safe_file_name).as_bytes(), bytes);
        let book_url = format!("{}:{}", LOCAL_EPUB_ORIGIN, hash);
        let book_dir = self.book_dir(user_ns, &book_url)?;

        if book_dir.exists() {
            // 同一本书重新导入会复用同一个 book_url, 必须先丢掉旧句柄与旧目录缓存。
            self.invalidate_archive(&book_url).await;
            self.invalidate_index(user_ns, &book_url).await;
            fs::remove_dir_all(&book_dir)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        fs::create_dir_all(book_dir.join("assets"))
            .await
            .map_err(|e| AppError::Internal(e.into()))?;
        fs::write(book_dir.join("original.epub"), bytes)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        let extract = {
            let epub_path = book_dir.join("original.epub");
            tokio::task::spawn_blocking(move || extract_epub(&epub_path))
                .await
                .map_err(|e| AppError::Internal(e.into()))??
        };
        let index = self
            .materialize_index(&book_dir, &book_url, safe_file_name, bytes.len(), extract)
            .await?;
        Ok(book_from_index(index))
    }

    /// 把抽取出来的数据落盘并组装索引(导入与重建共用)。
    async fn materialize_index(
        &self,
        book_dir: &Path,
        book_url: &str,
        file_name: String,
        byte_len: usize,
        extract: EpubExtract,
    ) -> Result<StoredEpubIndex, AppError> {
        let EpubExtract {
            package,
            files,
            title_map,
        } = extract;

        // 图片与样式表落盘(数量少、总价低), 正文不落盘 —— 翻到那一章时才解压。
        let asset_paths = collect_asset_paths(&package, &files);
        let mut stored_assets = Vec::new();
        write_assets(book_dir, &files, &asset_paths, &mut stored_assets).await?;
        write_stylesheets(book_dir, &files, &package, book_url, &asset_paths).await?;

        let chapters = build_chapter_list(&files, &package, book_url, &title_map)?;
        let cover_url = resolve_cover_path(&package)
            .filter(|path| asset_paths.contains(path))
            .map(|path| build_asset_url(book_url, &path));
        let name = package
            .title
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| book_name_from_epub_file_name(&file_name));
        let author = package
            .author
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "本地导入".to_string());
        let index = StoredEpubIndex {
            book_url: book_url.to_string(),
            name,
            author,
            file_name,
            byte_len,
            // 正文按需解压, 导入阶段不再统计字数(0 表示未知)。
            char_len: 0,
            cover_url,
            chapters,
            assets: stored_assets,
            engine_version: CURRENT_EPUB_ENGINE_VERSION,
        };
        let data =
            serde_json::to_string_pretty(&index).map_err(|e| AppError::Internal(e.into()))?;
        write_file_atomic(&book_dir.join("manifest.json"), data.as_bytes()).await?;
        Ok(index)
    }

    pub async fn get_book_info(&self, user_ns: &str, book_url: &str) -> Result<Book, AppError> {
        let index = self.read_index(user_ns, book_url).await?;
        Ok(book_from_index((*index).clone()))
    }

    pub async fn get_chapter_list(
        &self,
        user_ns: &str,
        book_url: &str,
    ) -> Result<Vec<BookChapter>, AppError> {
        let index = self.read_index(user_ns, book_url).await?;
        Ok(index
            .chapters
            .iter()
            .map(|chapter| BookChapter {
                title: chapter.title.clone(),
                url: chapter.url.clone(),
                index: chapter.index,
                ..BookChapter::default()
            })
            .collect())
    }

    pub async fn get_content(&self, user_ns: &str, chapter_url: &str) -> Result<String, AppError> {
        let (book_url, requested_index) = parse_chapter_url(chapter_url)?;
        let index = self.read_index(user_ns, &book_url).await?;
        let chapter = index
            .chapters
            .iter()
            .find(|chapter| chapter.index == requested_index)
            .ok_or_else(|| AppError::BadRequest("章节不存在".to_string()))?;
        let book_dir = self.book_dir(user_ns, &book_url)?;
        // v2 遗留索引(还没被重建成功): 它落盘的章节**已经是清洗过的产物**, 直接
        // 返回即可。再清洗一遍会把里面已改写成绝对地址的 `<img src>` 当成非法
        // 相对路径丢掉, 插图全部消失。
        if chapter.path.is_empty() {
            let Some(file_name) = chapter.file_name.as_deref() else {
                return Err(AppError::BadRequest("章节不存在".to_string()));
            };
            return fs::read_to_string(book_dir.join("chapters").join(file_name))
                .await
                .map_err(map_local_epub_read_error);
        }
        // 正文按需解压 + 清洗 + 注入样式: 这几步以前在导入时对每一章做一遍,
        // 现在只对用户真正翻到的这一章做。
        let html = self
            .read_chapter_html(&book_url, &book_dir, &chapter.path)
            .await?;
        let asset_paths = index
            .assets
            .iter()
            .map(|asset| asset.path.clone())
            .collect::<HashSet<_>>();
        let sanitized = sanitize_chapter_html(&html, &chapter.path, &book_url, &asset_paths);
        let stripped = strip_leading_duplicate_title(&sanitized.html, &chapter.title);
        let css = load_chapter_styles(&book_dir, &sanitized.stylesheet_hrefs, &chapter.path).await;
        if css.trim().is_empty() {
            return Ok(stripped);
        }
        Ok(format!(
            "<div class=\"{CHAPTER_SCOPE_CLASS}\"><style>{css}</style>{stripped}</div>"
        ))
    }

    pub async fn get_asset(
        &self,
        user_ns: &str,
        book_url: &str,
        path: &str,
    ) -> Result<LocalEpubAsset, AppError> {
        let index = self.read_index(user_ns, book_url).await?;
        let path = normalize_strict_zip_path(path)
            .ok_or_else(|| AppError::BadRequest("EPUB 资源地址无效".to_string()))?;
        let asset = index
            .assets
            .iter()
            .find(|asset| asset.path == path)
            .ok_or_else(|| AppError::BadRequest("EPUB 资源不存在".to_string()))?;
        let bytes = fs::read(self.book_dir(user_ns, book_url)?.join("assets").join(&path))
            .await
            .map_err(map_local_epub_read_error)?;
        Ok(LocalEpubAsset {
            bytes,
            content_type: asset.content_type.clone(),
        })
    }

    pub async fn delete_book_files(&self, user_ns: &str, book_url: &str) -> Result<bool, AppError> {
        // 先丢缓存句柄: 否则 Windows 上目录会因文件被占用而删不掉。
        self.invalidate_archive(book_url).await;
        self.invalidate_index(user_ns, book_url).await;
        let book_dir = self.book_dir(user_ns, book_url)?;
        match fs::remove_dir_all(book_dir).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(AppError::Internal(err.into())),
        }
    }

    fn local_root(&self, user_ns: &str) -> PathBuf {
        self.storage_dir
            .join("data")
            .join(user_ns)
            .join(LOCAL_BOOK_DIR)
    }

    fn book_dir(&self, user_ns: &str, book_url: &str) -> Result<PathBuf, AppError> {
        let hash = local_epub_hash_from_url(book_url)?;
        Ok(self.local_root(user_ns).join(hash))
    }

    async fn read_index(
        &self,
        user_ns: &str,
        book_url: &str,
    ) -> Result<Arc<StoredEpubIndex>, AppError> {
        let cache_key = format!("{user_ns}:{book_url}");
        if let Some(hit) = self.indexes.lock().await.get(&cache_key) {
            return Ok(hit.clone());
        }
        let dir = self.book_dir(user_ns, book_url)?;
        let index = Arc::new(self.load_index(&dir, book_url).await?);
        let mut cache = self.indexes.lock().await;
        if cache.len() >= LOCAL_EPUB_INDEX_CACHE_LIMIT {
            if let Some(evicted) = cache.keys().next().cloned() {
                cache.remove(&evicted);
            }
        }
        cache.insert(cache_key, index.clone());
        Ok(index)
    }

    /// 读 manifest; 版本过旧时用原书就地重建一次。
    async fn load_index(&self, dir: &Path, book_url: &str) -> Result<StoredEpubIndex, AppError> {
        let index = self.read_index_file(dir).await?;
        // 旧版本导入的书没有保留原书 CSS, 章节里只剩裸标签, 排版全部丢失。
        // 原始 epub 还留在 book_dir 里, 就地按新规则重建一次, 用户不必重新导入。
        if index.engine_version >= CURRENT_EPUB_ENGINE_VERSION || !dir.join("original.epub").exists()
        {
            return Ok(index);
        }

        let _guard = self.rebuild_lock.lock().await;
        // 等锁期间可能已被另一个读取重建过: 重新读一次再决定, 避免重复解包。
        let latest = self.read_index_file(dir).await.unwrap_or(index);
        if latest.engine_version >= CURRENT_EPUB_ENGINE_VERSION {
            return Ok(latest);
        }
        self.rebuild_index(book_url, dir, latest).await
    }

    /// 丢弃某本书的目录缓存。重新导入 / 重建索引 / 删书都要调用, 否则会继续按
    /// 旧 manifest 的章节路径去解压。
    async fn invalidate_index(&self, user_ns: &str, book_url: &str) {
        self.indexes
            .lock()
            .await
            .remove(&format!("{user_ns}:{book_url}"));
    }

    async fn read_index_file(&self, dir: &Path) -> Result<StoredEpubIndex, AppError> {
        let data = fs::read_to_string(dir.join("manifest.json"))
            .await
            .map_err(map_local_epub_read_error)?;
        serde_json::from_str(&data).map_err(|e| AppError::BadRequest(e.to_string()))
    }

    /// 用留存的 original.epub 重新解包, 返回重建后的索引; 失败时退回旧索引(不可读比不可用更糟)。
    ///
    /// 同一版本内失败过一次就不再重试(留下哨兵文件), 否则每次读章节都会付一次
    /// 全量解包的成本。
    async fn rebuild_index(
        &self,
        book_url: &str,
        dir: &Path,
        fallback: StoredEpubIndex,
    ) -> Result<StoredEpubIndex, AppError> {
        let marker = dir.join(REBUILD_FAILED_MARKER);
        if rebuild_failed_for_current_version(&marker).await {
            return Ok(fallback);
        }
        let rebuilt = match rebuild_from_dir(dir, &fallback).await {
            Ok(index) => index,
            Err(err) => {
                tracing::warn!("rebuild local epub failed, keeping old manifest: {err:?}");
                // 空内容表示"本版本没有失败记录", 因此用版本号写入标记。
                let _ = write_file_atomic(
                    &marker,
                    CURRENT_EPUB_ENGINE_VERSION.to_string().as_bytes(),
                )
                .await;
                return Ok(fallback);
            }
        };
        // 重建重写了 assets/styles, 缓存句柄要丢掉(它指向的仍是原文件, 但语义上
        // 这份索引已换代, 重新打开最省心)。清单同理: 章节路径可能变了。
        self.invalidate_archive(&rebuilt.book_url).await;
        self.invalidate_index_by_url(book_url).await;
        if let Err(err) = write_file_atomic(&marker, b"").await {
            tracing::debug!("clear epub rebuild marker failed: {err:?}");
        }
        match serde_json::to_string_pretty(&rebuilt) {
            Ok(data) => {
                if let Err(err) =
                    write_file_atomic(&dir.join("manifest.json"), data.as_bytes()).await
                {
                    tracing::warn!("write rebuilt epub manifest failed: {err:?}");
                }
            }
            Err(err) => tracing::warn!("serialize rebuilt epub manifest failed: {err:?}"),
        }
        Ok(rebuilt)
    }

    /// 按 book_url 丢弃所有命名空间下的目录缓存(重建索引时手头只有 book_url)。
    async fn invalidate_index_by_url(&self, book_url: &str) {
        let suffix = format!(":{book_url}");
        let mut cache = self.indexes.lock().await;
        cache.retain(|key, _| !key.ends_with(&suffix));
    }
}

/// 重建失败哨兵的文件名(与 `manifest.json` 同目录), 内容为失败时的引擎版本号。
const REBUILD_FAILED_MARKER: &str = ".rebuild-failed";

/// 记录中的失败是否属于当前引擎版本。空文件(或不存在)表示可以尝试重建。
async fn rebuild_failed_for_current_version(marker: &Path) -> bool {
    match fs::read_to_string(marker).await {
        Ok(text) => text.trim() == CURRENT_EPUB_ENGINE_VERSION.to_string(),
        Err(_) => false,
    }
}

/// 原子替换文件内容: 先写同目录的临时文件, 再 rename 覆盖。
///
/// `fs::write` 是"截断 + 全量写", 而读取路径不持 `rebuild_lock`(目录/正文/封面会
/// 各自读一次 manifest) —— 重建一本旧书时并发读会撞上写窗口, 拿到空文件或半截
/// JSON, 前端表现为"书籍信息错误"。rename 之后读者只会看到完整旧版或完整新版。
async fn write_file_atomic(path: &Path, data: impl AsRef<[u8]>) -> Result<(), AppError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("tmp");
    let tmp = path.with_file_name(format!(".{name}.tmp"));
    fs::write(&tmp, data.as_ref())
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    fs::rename(&tmp, path)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}

/// 把 manifest 里的图片落盘到 `assets/`, 同时收集索引条目。
async fn write_assets(
    book_dir: &Path,
    files: &HashMap<String, Vec<u8>>,
    asset_paths: &HashSet<String>,
    stored_assets: &mut Vec<StoredEpubAsset>,
) -> Result<(), AppError> {
    for path in asset_paths {
        let Some(data) = files.get(path) else {
            continue;
        };
        let content_type = content_type_for_path(path);
        let target = safe_child_path(&book_dir.join("assets"), path)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        write_file_atomic(&target, data).await?;
        stored_assets.push(StoredEpubAsset {
            path: path.clone(),
            content_type,
        });
    }
    Ok(())
}

/// 按 spine 顺序建立章节索引。
///
/// 这里**不生成章节正文**, 只记下每章在 `original.epub` 里的路径; 正文由
/// `read_chapter_html` 在翻到那一章时解压。标题优先取目录(nav/ncx), 没有目录
/// 的书用导入时读到的章节开头片段提取。
fn build_chapter_list(
    files: &HashMap<String, Vec<u8>>,
    package: &EpubPackage,
    book_url: &str,
    title_map: &HashMap<String, String>,
) -> Result<Vec<StoredEpubChapter>, AppError> {
    let mut chapters = Vec::new();
    let mut seen_chapter_paths = HashSet::new();
    for idref in &package.spine {
        let Some(item) = package.manifest.get(idref) else {
            continue;
        };
        if !is_html_media_type(&item.media_type) || !seen_chapter_paths.insert(item.path.clone()) {
            continue;
        }
        let toc_title = title_map
            .get(&item.path)
            .map(|value| value.trim())
            .filter(|value| is_meaningful_epub_title(value));
        let title = toc_title
            .map(str::to_string)
            .or_else(|| {
                files
                    .get(&item.path)
                    .and_then(|raw| extract_html_title(&decode_utf8_lossy(raw)))
            })
            .unwrap_or_else(|| format!("第 {} 章", chapters.len() + 1));
        let index = chapters.len();
        chapters.push(StoredEpubChapter {
            title,
            url: build_epub_chapter_url(book_url, index),
            index: index as i32,
            path: item.path.clone(),
            file_name: None,
        });
    }

    if chapters.is_empty() {
        return Err(AppError::BadRequest("EPUB 未找到可阅读章节".to_string()));
    }
    Ok(chapters)
}

/// 读取本章引用到的、已落盘的作用域化样式; 没有 `<link>` 的章节退回通用图片样式。
async fn load_chapter_styles(book_dir: &Path, hrefs: &[String], chapter_path: &str) -> String {
    let styles_dir = book_dir.join("styles");
    let base = parent_zip_dir(chapter_path);
    let mut css = String::new();
    for href in hrefs {
        let Some(path) = join_zip_path(&base, href) else {
            continue;
        };
        let Ok(target) = safe_child_path(&styles_dir, &path) else {
            continue;
        };
        if let Ok(text) = fs::read_to_string(&target).await {
            css.push_str(&text);
        }
    }
    if css.trim().is_empty() {
        if let Ok(text) = fs::read_to_string(styles_dir.join(IMAGE_BASE_FILE)).await {
            css.push_str(&text);
        }
    }
    css
}

/// 把原书样式表作用域化后落盘(整本书共用, 不必每章重算)。
async fn write_stylesheets(
    book_dir: &Path,
    files: &HashMap<String, Vec<u8>>,
    package: &EpubPackage,
    book_url: &str,
    asset_paths: &HashSet<String>,
) -> Result<(), AppError> {
    let scope = format!(".{}", CHAPTER_SCOPE_CLASS);
    let styles_dir = book_dir.join("styles");
    fs::create_dir_all(&styles_dir)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    for path in collect_stylesheet_paths(package, files) {
        let Some(raw) = files.get(&path) else {
            continue;
        };
        let css = scope_stylesheet(
            &decode_utf8_lossy(raw),
            &path,
            book_url,
            &scope,
            asset_paths,
        );
        if css.trim().is_empty() {
            continue;
        }
        let target = safe_child_path(&book_dir.join("styles"), &path)?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }
        write_file_atomic(&target, css.as_bytes()).await?;
    }
    // 兜底样式同样要过作用域改写: 裸 `svg{display:block}` 会命中阅读器界面的
    // 所有内联图标(关闭按钮、翻页箭头), 因为注入的 `<style>` 是全局生效的。
    let image_base = rewrite_css_block(IMAGE_BASE_CSS, &scope, "", book_url, asset_paths);
    write_file_atomic(
        &book_dir.join("styles").join(IMAGE_BASE_FILE),
        image_base.as_bytes(),
    )
    .await?;
    Ok(())
}

/// 用留存的 original.epub 就地重建索引(旧版本导入的书结构不同)。
///
/// 重建同样走轻量抽取: 旧版本留下的逐章 HTML 目录会一并清掉, 否则一本 1800 章
/// 的书白占十几 MB。
async fn rebuild_from_dir(dir: &Path, fallback: &StoredEpubIndex) -> Result<StoredEpubIndex, AppError> {
    let epub_path = dir.join("original.epub");
    let byte_len = fs::metadata(&epub_path)
        .await
        .map_err(map_local_epub_read_error)?
        .len() as usize;
    let extract = {
        let epub_path = epub_path.clone();
        tokio::task::spawn_blocking(move || extract_epub(&epub_path))
            .await
            .map_err(|e| AppError::Internal(e.into()))??
    };
    let book_url = fallback.book_url.clone();
    let EpubExtract {
        package,
        files,
        title_map,
    } = extract;

    let asset_paths = collect_asset_paths(&package, &files);
    let mut stored_assets = Vec::new();
    write_assets(dir, &files, &asset_paths, &mut stored_assets).await?;
    write_stylesheets(dir, &files, &package, &book_url, &asset_paths).await?;

    let chapters = build_chapter_list(&files, &package, &book_url, &title_map)?;
    // 新索引已经建好, 旧版本的逐章 HTML 才可以清(它只由本服务生成)。放在最后
    // 是因为中途失败要回退到旧索引, 那时这些文件还得能用。
    let legacy_chapters = dir.join("chapters");
    if legacy_chapters.exists() {
        if let Err(err) = fs::remove_dir_all(&legacy_chapters).await {
            tracing::warn!("remove legacy epub chapter files failed: {err:?}");
        }
    }

    let cover_url = resolve_cover_path(&package)
        .filter(|path| asset_paths.contains(path))
        .map(|path| build_asset_url(&book_url, &path));

    Ok(StoredEpubIndex {
        book_url,
        name: package
            .title
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| fallback.name.clone()),
        author: package
            .author
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| fallback.author.clone()),
        file_name: fallback.file_name.clone(),
        byte_len,
        char_len: 0,
        cover_url,
        chapters,
        assets: stored_assets,
        engine_version: CURRENT_EPUB_ENGINE_VERSION,
    })
}

fn validate_epub_upload(file_name: &str, byte_len: usize) -> Result<(), AppError> {
    let safe = sanitize_epub_file_name(file_name);
    if !safe.to_lowercase().ends_with(".epub") {
        return Err(AppError::BadRequest("仅支持上传 .epub 文件".to_string()));
    }
    if byte_len == 0 {
        return Err(AppError::BadRequest("EPUB 文件不能为空".to_string()));
    }
    if byte_len > MAX_EPUB_UPLOAD_BYTES {
        return Err(AppError::BadRequest("EPUB 文件不能超过 80MB".to_string()));
    }
    Ok(())
}

fn sanitize_epub_file_name(file_name: &str) -> String {
    let name = Path::new(file_name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("book.epub")
        .trim()
        .to_string();
    if name.is_empty() {
        "book.epub".to_string()
    } else {
        name
    }
}

fn book_name_from_epub_file_name(file_name: &str) -> String {
    let safe = sanitize_epub_file_name(file_name);
    Path::new(&safe)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("本地 EPUB")
        .to_string()
}

/// 导入阶段从 zip 抽取的数据。
///
/// `files` 只包含**真正需要的内容**: container/OPF/nav/ncx、图片、样式表, 以及
/// (仅当书里没有 nav/ncx 时)各章开头一小段用于提取标题。1800 章的正文不再
/// 全量解压 —— 那是导入耗时的大头, 也是 readest 那类阅读器"瞬间导入"的关键:
/// 正文留到翻到那一章时再解压(见 `read_chapter_html`)。
struct EpubExtract {
    package: EpubPackage,
    files: HashMap<String, Vec<u8>>,
    title_map: HashMap<String, String>,
}

/// 同步抽取: 只解压导入必须的那几个条目。调用方放进 blocking 池执行。
fn extract_epub(epub_path: &Path) -> Result<EpubExtract, AppError> {
    let file = File::open(epub_path).map_err(map_local_epub_read_error)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|_| AppError::BadRequest("EPUB 文件损坏".to_string()))?;
    if archive.len() > MAX_EPUB_FILE_COUNT {
        return Err(AppError::BadRequest("EPUB 文件数量过多".to_string()));
    }
    // 体积校验走中央目录的声明大小, 不解压: 既不触发解压开销, 也仍然能挡住 zip bomb。
    let mut declared_bytes = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| AppError::BadRequest("EPUB 文件损坏".to_string()))?;
        if !entry.is_file() {
            continue;
        }
        if entry.enclosed_name().is_none() {
            return Err(AppError::BadRequest("EPUB 包含非法路径".to_string()));
        }
        declared_bytes = declared_bytes.saturating_add(entry.size());
    }
    if declared_bytes > MAX_EPUB_UNPACKED_BYTES {
        return Err(AppError::BadRequest("EPUB 解压后体积过大".to_string()));
    }

    let container = read_zip_entry(&mut archive, "META-INF/container.xml")?
        .ok_or_else(|| AppError::BadRequest("EPUB 缺少 container.xml".to_string()))?;
    let rootfile = parse_container_rootfile(&decode_utf8_lossy(&container))?;
    let opf_bytes = read_zip_entry(&mut archive, &rootfile)?
        .ok_or_else(|| AppError::BadRequest(format!("EPUB 缺少文件 {rootfile}")))?;
    let opf_base = parent_zip_dir(&rootfile);
    let package = parse_opf(&decode_utf8_lossy(&opf_bytes), &opf_base)?;

    let mut files = HashMap::new();
    for path in import_entry_paths(&package) {
        if let Some(data) = read_zip_entry(&mut archive, &path)? {
            files.insert(path, data);
        }
    }

    let mut title_map = load_title_map(&files, &package);

    // 目录里没有标题的章节(封面页、版权页这类常被 nav/ncx 漏掉)要回退到从正文
    // 提取标题; 完全没目录的书则每章都要。两种都只读章节开头一小段, 部分解压。
    let mut head_paths: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for idref in &package.spine {
        let Some(item) = package.manifest.get(idref) else {
            continue;
        };
        if !is_html_media_type(&item.media_type) || !seen.insert(item.path.clone()) {
            continue;
        }
        if title_map.contains_key(&item.path) || files.contains_key(&item.path) {
            continue;
        }
        head_paths.push(item.path.clone());
    }
    for path in head_paths {
        if let Some(head) = read_zip_entry_head(&mut archive, &path, TITLE_SCAN_BYTES)? {
            files.insert(path, head);
        }
    }
    // 标题仍由 `build_chapter_list` 从这些片段里提取, 这里不预填 title_map。
    title_map.shrink_to_fit();

    Ok(EpubExtract {
        package,
        files,
        title_map,
    })
}

/// 导入需要读进内存的条目: 目录、图片、样式表, 以及少量 HTML 用于发现漏登记的 CSS。
fn import_entry_paths(package: &EpubPackage) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for id in [package.nav_id.as_ref(), package.ncx_id.as_ref()]
        .into_iter()
        .flatten()
    {
        if let Some(item) = package.manifest.get(id) {
            paths.push(item.path.clone());
        }
    }
    paths.extend(collect_asset_paths(package, &HashMap::new()));
    paths.extend(
        package
            .manifest
            .values()
            .filter(|item| item.media_type == "text/css" || is_css_path(&item.path))
            .map(|item| item.path.clone()),
    );
    // 样式表引用在整本书里几乎总是一致, 看开头几章就能发现漏登记的 CSS。
    paths.extend(
        package
            .manifest
            .values()
            .filter(|item| is_html_media_type(&item.media_type))
            .take(HTML_STYLE_SCAN_COUNT)
            .map(|item| item.path.clone()),
    );
    paths.sort();
    paths.dedup();
    paths
}

/// 读一个 zip 条目; 不存在时返回 `None`。
fn read_zip_entry(
    archive: &mut zip::ZipArchive<File>,
    name: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };
    if !entry.is_file() {
        return Ok(None);
    }
    // 中央目录里声明的大小是可以谎报的, 所以按**实际**读出的字节数设上限:
    // 超过就中止, 不让一个畸形条目把内存吃光。
    let capacity = entry.size().min(MAX_EPUB_ENTRY_BYTES) as usize;
    let mut data = Vec::with_capacity(capacity);
    (&mut entry)
        .take(MAX_EPUB_ENTRY_BYTES + 1)
        .read_to_end(&mut data)
        .map_err(|_| AppError::BadRequest("EPUB 文件损坏".to_string()))?;
    if data.len() as u64 > MAX_EPUB_ENTRY_BYTES {
        return Err(AppError::BadRequest("EPUB 条目过大".to_string()));
    }
    Ok(Some(data))
}

/// 只读一个 zip 条目的开头 `limit` 字节(deflate 流式解压, 不会解完整章)。
fn read_zip_entry_head(
    archive: &mut zip::ZipArchive<File>,
    name: &str,
    limit: usize,
) -> Result<Option<Vec<u8>>, AppError> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };
    if !entry.is_file() {
        return Ok(None);
    }
    let mut data = Vec::with_capacity(limit.min(entry.size() as usize));
    let mut buf = [0u8; 4096];
    while data.len() < limit {
        let want = (limit - data.len()).min(buf.len());
        let read = entry
            .read(&mut buf[..want])
            .map_err(|_| AppError::BadRequest("EPUB 文件损坏".to_string()))?;
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buf[..read]);
    }
    Ok(Some(data))
}

fn parse_container_rootfile(xml: &str) -> Result<String, AppError> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local_name(e.name().as_ref()) == "rootfile" =>
            {
                if let Some(path) = attr_by_name(&e, "full-path") {
                    return normalize_strict_zip_path(&path)
                        .ok_or_else(|| AppError::BadRequest("EPUB rootfile 路径无效".to_string()));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return Err(AppError::BadRequest("EPUB container.xml 无效".to_string())),
            _ => {}
        }
        buf.clear();
    }
    Err(AppError::BadRequest("EPUB 缺少 rootfile".to_string()))
}

fn parse_opf(xml: &str, opf_base: &str) -> Result<EpubPackage, AppError> {
    let mut package = EpubPackage::default();
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut current_meta: Option<String> = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = local_name(e.name().as_ref());
                match tag.as_str() {
                    "title" | "creator" => current_meta = Some(tag),
                    "item" | "itemref" | "meta" => parse_opf_element(&mut package, &e, opf_base),
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                parse_opf_element(&mut package, &e, opf_base);
            }
            Ok(Event::Text(e)) => {
                if let Some(kind) = current_meta.as_deref() {
                    let text = e.unescape().unwrap_or_default().trim().to_string();
                    if !text.is_empty() {
                        match kind {
                            "title" if package.title.is_none() => package.title = Some(text),
                            "creator" if package.author.is_none() => package.author = Some(text),
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let tag = local_name(e.name().as_ref());
                if current_meta.as_deref() == Some(tag.as_str()) {
                    current_meta = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return Err(AppError::BadRequest("EPUB OPF 文件无效".to_string())),
            _ => {}
        }
        buf.clear();
    }

    if package.manifest.is_empty() || package.spine.is_empty() {
        return Err(AppError::BadRequest(
            "EPUB 缺少 manifest 或 spine".to_string(),
        ));
    }
    Ok(package)
}

fn parse_opf_element(package: &mut EpubPackage, e: &BytesStart<'_>, opf_base: &str) {
    match local_name(e.name().as_ref()).as_str() {
        "item" => {
            let id = attr_by_name(e, "id").unwrap_or_default();
            let href = attr_by_name(e, "href").unwrap_or_default();
            let media_type = attr_by_name(e, "media-type").unwrap_or_default();
            let properties = attr_by_name(e, "properties").unwrap_or_default();
            if id.is_empty() || href.is_empty() {
                return;
            }
            let Some(path) = join_zip_path(opf_base, &href) else {
                return;
            };
            if media_type == "application/x-dtbncx+xml" {
                package.ncx_id = Some(id.clone());
            }
            if properties.split_whitespace().any(|value| value == "nav") {
                package.nav_id = Some(id.clone());
            }
            if properties
                .split_whitespace()
                .any(|value| value == "cover-image")
            {
                package.cover_id = Some(id.clone());
            }
            if media_type.starts_with("image/") || is_image_path(&path) {
                package.image_items.push(id.clone());
            }
            package.manifest.insert(
                id.clone(),
                EpubManifestItem {
                    path,
                    media_type,
                    properties,
                },
            );
        }
        "itemref" => {
            if let Some(idref) = attr_by_name(e, "idref").filter(|value| !value.is_empty()) {
                package.spine.push(idref);
            }
        }
        "meta" => {
            if attr_by_name(e, "name").as_deref() == Some("cover") {
                package.cover_meta = attr_by_name(e, "content");
            }
        }
        _ => {}
    }
}

fn load_title_map(
    files: &HashMap<String, Vec<u8>>,
    package: &EpubPackage,
) -> HashMap<String, String> {
    if let Some(nav_id) = &package.nav_id {
        if let Some(item) = package.manifest.get(nav_id) {
            if let Some(data) = files.get(&item.path) {
                let map = parse_nav_titles(&decode_utf8_lossy(data), &parent_zip_dir(&item.path));
                if !map.is_empty() {
                    return map;
                }
            }
        }
    }

    if let Some(ncx_id) = &package.ncx_id {
        if let Some(item) = package.manifest.get(ncx_id) {
            if let Some(data) = files.get(&item.path) {
                return parse_ncx_titles(&decode_utf8_lossy(data), &parent_zip_dir(&item.path));
            }
        }
    }
    HashMap::new()
}

fn parse_ncx_titles(xml: &str, base: &str) -> HashMap<String, String> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut stack: Vec<NavPointState> = Vec::new();
    let mut capture_text = false;
    let mut titles: HashMap<String, String> = HashMap::new();
    // 记录被多个 navPoint 指向的路径(标题有歧义,不可靠)
    let mut ambiguous: HashSet<String> = HashSet::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match local_name(e.name().as_ref()).as_str() {
                "navPoint" => stack.push(NavPointState::default()),
                "text" if !stack.is_empty() => capture_text = true,
                "content" => {
                    if let (Some(src), Some(state)) = (attr_by_name(&e, "src"), stack.last()) {
                        if let Some(path) = join_zip_path(base, &src) {
                            let label = state.label.trim();
                            if !label.is_empty() {
                                mark_title(&mut titles, &mut ambiguous, path, label);
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(e)) if local_name(e.name().as_ref()) == "content" => {
                if let (Some(src), Some(state)) = (attr_by_name(&e, "src"), stack.last()) {
                    if let Some(path) = join_zip_path(base, &src) {
                        let label = state.label.trim();
                        if !label.is_empty() {
                            mark_title(&mut titles, &mut ambiguous, path, label);
                        }
                    }
                }
            }
            Ok(Event::Text(e)) if capture_text => {
                if let Some(state) = stack.last_mut() {
                    state
                        .label
                        .push_str(e.unescape().unwrap_or_default().as_ref());
                }
            }
            Ok(Event::End(e)) => match local_name(e.name().as_ref()).as_str() {
                "text" => capture_text = false,
                "navPoint" => {
                    stack.pop();
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    // 移除有歧义的路径,让章节构建回退到从 HTML 提取标题
    for path in &ambiguous {
        titles.remove(path);
    }
    titles
}

fn parse_nav_titles(xml: &str, base: &str) -> HashMap<String, String> {
    let mut reader = Reader::from_str(xml);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut current_href: Option<String> = None;
    let mut current_text = String::new();
    let mut titles: HashMap<String, String> = HashMap::new();
    let mut ambiguous: HashSet<String> = HashSet::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == "a" => {
                current_href = attr_by_name(&e, "href");
                current_text.clear();
            }
            Ok(Event::Text(e)) if current_href.is_some() => {
                current_text.push_str(e.unescape().unwrap_or_default().as_ref());
            }
            Ok(Event::End(e)) if local_name(e.name().as_ref()) == "a" => {
                if let Some(href) = current_href.take() {
                    if let Some(path) = join_zip_path(base, &href) {
                        let title = current_text.trim();
                        if !title.is_empty() {
                            mark_title(&mut titles, &mut ambiguous, path, title);
                        }
                    }
                }
                current_text.clear();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    for path in &ambiguous {
        titles.remove(path);
    }
    titles
}

/// 记录标题映射; 同一路径被多个 navPoint 指向时标记为有歧义。
fn mark_title(
    titles: &mut HashMap<String, String>,
    ambiguous: &mut HashSet<String>,
    path: String,
    label: &str,
) {
    if ambiguous.contains(&path) {
        return;
    }
    if let Some(_existing) = titles.get(&path) {
        // 已有标题又来一个不同/相同的 -> 歧义
        ambiguous.insert(path);
    } else {
        titles.insert(path, label.to_string());
    }
}

fn extract_html_title(html: &str) -> Option<String> {
    let mut reader = Reader::from_str(html);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut capture: Option<String> = None;
    let mut captured_text = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = local_name(e.name().as_ref());
                if matches!(tag.as_str(), "title" | "h1" | "h2" | "h3") {
                    capture = Some(tag);
                    captured_text.clear();
                }
            }
            Ok(Event::Text(e)) if capture.is_some() => {
                captured_text.push_str(e.unescape().unwrap_or_default().as_ref());
            }
            Ok(Event::End(e)) => {
                let tag = local_name(e.name().as_ref());
                if capture.as_deref() == Some(tag.as_str()) {
                    let title = collapse_ws(&captured_text);
                    if is_meaningful_epub_title(&title) {
                        return Some(title);
                    }
                    capture = None;
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    None
}

/// 目录/HTML 中常见的占位标题不能当作真正章节名。
fn is_meaningful_epub_title(value: &str) -> bool {
    let title = value.trim();
    !title.is_empty()
        && !matches!(
            title.to_ascii_lowercase().as_str(),
            "未知" | "untitled" | "unknown" | "n/a" | "null"
        )
}

/// 清洗结果: 正文片段 + 本章引用的样式表 href。
///
/// href 在清洗的同一次遍历里收集。此前是清洗完再单独解析一遍 XHTML 去找
/// `<link>` —— 一本 1800 章的书要多解析 1800 次, 导入耗时接近翻倍。
struct SanitizedChapter {
    html: String,
    stylesheet_hrefs: Vec<String>,
}

fn sanitize_chapter_html(
    html: &str,
    chapter_path: &str,
    book_url: &str,
    asset_paths: &HashSet<String>,
) -> SanitizedChapter {
    let content = sanitize_chapter_html_inner(html, chapter_path, book_url, asset_paths, true);
    if content.html.trim().is_empty() {
        sanitize_chapter_html_inner(html, chapter_path, book_url, asset_paths, false)
    } else {
        content
    }
}

fn sanitize_chapter_html_inner(
    html: &str,
    chapter_path: &str,
    book_url: &str,
    asset_paths: &HashSet<String>,
    body_only: bool,
) -> SanitizedChapter {
    let mut reader = Reader::from_str(html);
    reader.trim_text(false);
    let mut buf = Vec::new();
    let mut out = String::new();
    let mut hrefs: Vec<String> = Vec::new();
    let mut in_body = !body_only;
    let mut skip_depth = 0usize;
    let base = parent_zip_dir(chapter_path);

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = local_name(e.name().as_ref());
                // `<link>` 不输出, 但要把样式表引用记下来(它在 head 里, 会被
                // 下面的 skip 分支吃掉)。
                if tag == "link" {
                    if let Some(href) = stylesheet_href_of(&e) {
                        hrefs.push(href);
                    }
                    buf.clear();
                    continue;
                }
                if tag == "body" {
                    in_body = true;
                    buf.clear();
                    continue;
                }
                if should_skip_tag(&tag) {
                    skip_depth += 1;
                    buf.clear();
                    continue;
                }
                if in_body && skip_depth == 0 && is_allowed_html_tag(&tag) {
                    out.push('<');
                    out.push_str(&tag);
                    push_safe_attrs(&mut out, &tag, &e, &base, book_url, asset_paths);
                    out.push('>');
                }
            }
            Ok(Event::Empty(e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "link" {
                    if let Some(href) = stylesheet_href_of(&e) {
                        hrefs.push(href);
                    }
                    buf.clear();
                    continue;
                }
                if in_body && skip_depth == 0 && is_allowed_html_tag(&tag) {
                    out.push('<');
                    out.push_str(&tag);
                    push_safe_attrs(&mut out, &tag, &e, &base, book_url, asset_paths);
                    out.push_str("/>");
                }
            }
            Ok(Event::End(e)) => {
                let tag = local_name(e.name().as_ref());
                if tag == "body" {
                    in_body = false;
                    buf.clear();
                    continue;
                }
                if should_skip_tag(&tag) && skip_depth > 0 {
                    skip_depth -= 1;
                    buf.clear();
                    continue;
                }
                if in_body
                    && skip_depth == 0
                    && is_allowed_html_tag(&tag)
                    && !is_void_html_tag(&tag)
                {
                    out.push_str("</");
                    out.push_str(&tag);
                    out.push('>');
                }
            }
            Ok(Event::Text(e)) if in_body && skip_depth == 0 => {
                out.push_str(&escape_html(e.unescape().unwrap_or_default().as_ref()));
            }
            Ok(Event::CData(e)) if in_body && skip_depth == 0 => {
                out.push_str(&escape_html(&decode_utf8_lossy(e.as_ref())));
            }
            Ok(Event::Eof) => break,
            Err(_) => {
                return SanitizedChapter {
                    html: strip_dangerous_html_fallback(html, chapter_path, book_url, asset_paths),
                    stylesheet_hrefs: Vec::new(),
                }
            }
            _ => {}
        }
        buf.clear();
    }
    SanitizedChapter {
        html: out,
        stylesheet_hrefs: hrefs,
    }
}

/// `<link rel="stylesheet" href="...">` 的 href。
fn stylesheet_href_of(e: &BytesStart<'_>) -> Option<String> {
    let rel = attr_by_name(e, "rel")?.to_ascii_lowercase();
    if !rel.split_whitespace().any(|value| value == "stylesheet") {
        return None;
    }
    attr_by_name(e, "href").filter(|value| !value.trim().is_empty())
}

fn push_safe_attrs(
    out: &mut String,
    tag: &str,
    e: &BytesStart<'_>,
    base: &str,
    book_url: &str,
    asset_paths: &HashSet<String>,
) {
    for attr in e.attributes().flatten() {
        let raw_key = std::str::from_utf8(attr.key.as_ref()).unwrap_or_default();
        // SVG 的 `xlink:href` 必须原样保留(连同它的命名空间声明): 缩成 `href`
        // 在 HTML 解析下通常仍能取到图, 但丢掉 `xmlns:xlink` 会让严格的
        // XML/SVG 解析路径直接失败。
        let key = if raw_key.eq_ignore_ascii_case("xlink:href")
            || raw_key.eq_ignore_ascii_case("xmlns:xlink")
        {
            raw_key.to_ascii_lowercase()
        } else {
            local_name(attr.key.as_ref())
        };
        if key.starts_with("on") {
            continue;
        }
        let Some(value) = attr_value(&attr) else {
            continue;
        };
        let safe_value = match key.as_str() {
            "src" if tag == "img" => {
                let Some(path) = join_zip_path(base, &value) else {
                    continue;
                };
                if !asset_paths.contains(&path) {
                    continue;
                }
                build_asset_url(book_url, &path)
            }
            // 封面页常用 `<svg><image xlink:href="cover.jpg"/></svg>` 而不是 `<img>`;
            // 只认 `src` 会让整页封面变成空白。
            "xlink:href" | "href" if tag == "image" => {
                let Some(path) = join_zip_path(base, &value) else {
                    continue;
                };
                if !asset_paths.contains(&path) {
                    continue;
                }
                build_asset_url(book_url, &path)
            }
            "src" if tag == "image" => {
                let Some(path) = join_zip_path(base, &value) else {
                    continue;
                };
                if !asset_paths.contains(&path) {
                    continue;
                }
                build_asset_url(book_url, &path)
            }
            "href" if tag == "a" => {
                if value.trim_start().starts_with('#') {
                    value
                } else {
                    continue;
                }
            }
            // 图片与 SVG 的尺寸/对齐属性, 丢了会让插图被拉满整屏或比例错乱。
            "width" | "height" | "viewBox" | "preserveAspectRatio" | "x" | "y" | "xmlns"
            | "xmlns:xlink"
                if matches!(tag, "img" | "svg" | "image") =>
            {
                value
            }
            "alt" | "title" | "class" | "id" | "colspan" | "rowspan" => value,
            "style" => sanitize_inline_style(&value, base, book_url, asset_paths),
            _ => continue,
        };
        if safe_value.is_empty() {
            continue;
        }
        out.push(' ');
        out.push_str(&key);
        out.push_str("=\"");
        out.push_str(&escape_attr(&safe_value));
        out.push('"');
    }
}

/// 行内 `style` 只保留排版相关声明, 且把其中的相对 `url()` 改写成资源地址。
fn sanitize_inline_style(
    value: &str,
    base: &str,
    book_url: &str,
    asset_paths: &HashSet<String>,
) -> String {
    let rewritten = rewrite_css_declarations(value, base, book_url, asset_paths);
    rewritten.trim_end_matches(';').to_string()
}

fn strip_dangerous_html_fallback(
    html: &str,
    chapter_path: &str,
    book_url: &str,
    asset_paths: &HashSet<String>,
) -> String {
    let mut without_scripts = html.to_string();
    for pattern in [
        r"(?is)<\s*script[^>]*>.*?<\s*/\s*script\s*>",
        r"(?is)<\s*style[^>]*>.*?<\s*/\s*style\s*>",
    ] {
        if let Ok(re) = Regex::new(pattern) {
            without_scripts = re.replace_all(&without_scripts, "").into_owned();
        }
    }
    let img_re = Regex::new(r#"(?is)<img\b[^>]*\bsrc=["']([^"']+)["'][^>]*>"#).ok();
    if let Some(re) = img_re {
        let base = parent_zip_dir(chapter_path);
        re.replace_all(&without_scripts, |captures: &regex::Captures<'_>| {
            let Some(src) = captures.get(1).map(|m| m.as_str()) else {
                return String::new();
            };
            let Some(path) = join_zip_path(&base, src) else {
                return String::new();
            };
            if !asset_paths.contains(&path) {
                return String::new();
            }
            format!(
                "<img src=\"{}\"/>",
                escape_attr(&build_asset_url(book_url, &path))
            )
        })
        .into_owned()
    } else {
        without_scripts
    }
}

fn collect_asset_paths(
    package: &EpubPackage,
    _files: &HashMap<String, Vec<u8>>,
) -> HashSet<String> {
    let mut assets = HashSet::new();
    for item in package.manifest.values() {
        if item.media_type.starts_with("image/") || is_image_path(&item.path) {
            assets.insert(item.path.clone());
        }
    }
    assets
}

const IMAGE_BASE_CSS: &str = "\
img,svg{max-width:100%;height:auto;}\
svg{display:block;margin:0 auto;}\
";

/// 章节正文的包裹类名。
///
/// 前端 `ReaderView.vue` 的 `isEpubChapterHtml` 按这个类名判断"本章自带排版",
/// 两处必须一致; 原书 CSS 的作用域前缀也是它。
pub const CHAPTER_SCOPE_CLASS: &str = "epub-chapter-content";

/// 收集原书的样式表: manifest 里声明为 `text/css` 的项, 以及被 XHTML 以
/// `<link rel="stylesheet">` 引用、但漏登记的 CSS 文件。
///
/// 第二步要解析 XHTML, 而一本 1800 章的书有 1800 个 XHTML —— 全扫一遍是导入
/// 耗时的大头。样式表引用在整本书里几乎总是一致, 看开头几章就够发现漏登记的文件。
///
/// 上限固定为 `HTML_STYLE_SCAN_COUNT`: 无 nav 的书里 `files` 还装着 1800 个
/// "章节开头片段", 若在 manifest 无 CSS 时放开成"全扫", 这些片段会被逐个解析,
/// 导入又慢回去。
fn collect_stylesheet_paths(
    package: &EpubPackage,
    files: &HashMap<String, Vec<u8>>,
) -> Vec<String> {
    let mut paths: Vec<String> = package
        .manifest
        .values()
        .filter(|item| item.media_type == "text/css" || is_css_path(&item.path))
        .map(|item| item.path.clone())
        .collect();

    let html_scan_limit = HTML_STYLE_SCAN_COUNT;
    let mut scanned = 0usize;
    for item in package.manifest.values() {
        if !is_html_media_type(&item.media_type) {
            continue;
        }
        if scanned >= html_scan_limit {
            break;
        }
        scanned += 1;
        let Some(raw) = files.get(&item.path) else {
            continue;
        };
        let base = parent_zip_dir(&item.path);
        for href in collect_stylesheet_links(&decode_utf8_lossy(raw)) {
            let Some(path) = join_zip_path(&base, &href) else {
                continue;
            };
            if files.contains_key(&path) && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

/// 从 XHTML 里取出 `<link rel="stylesheet" href="...">` 的 href, 保持书写顺序。
fn collect_stylesheet_links(html: &str) -> Vec<String> {
    let mut reader = Reader::from_str(html);
    reader.trim_text(true);
    let mut buf = Vec::new();
    let mut hrefs = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf);
        let start = match event {
            Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e))
                if local_name(e.name().as_ref()) == "link" =>
            {
                e
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {
                buf.clear();
                continue;
            }
        };
        let rel = attr_by_name(start, "rel").unwrap_or_default().to_ascii_lowercase();
        if rel.split_whitespace().any(|value| value == "stylesheet") {
            if let Some(href) = attr_by_name(start, "href").filter(|value| !value.trim().is_empty()) {
                hrefs.push(href);
            }
        }
        buf.clear();
    }
    hrefs
}

/// 把一份 CSS 改写成可以安全内联进阅读页的形态:
/// - 选择器统一加作用域前缀, 避免 `body{}`/`div{}` 这类规则波及阅读器界面;
/// - `url(...)` 改写成资源接口地址;
/// - 丢掉 `@import`(内容已单独收集)与 `@font-face`(字体文件不在资源白名单里);
/// - 剥掉 `duokan-`/`-epub-`/`-webkit-` 之类的私有属性, 只留浏览器认识的。
fn scope_stylesheet(
    css: &str,
    css_path: &str,
    book_url: &str,
    scope: &str,
    asset_paths: &HashSet<String>,
) -> String {
    let base = parent_zip_dir(css_path);
    let mut out = String::with_capacity(css.len());
    let mut rest = strip_css_comments(css);

    while let Some(at) = rest.find('@') {
        // `@` 之前是普通规则, 交给 `rewrite_css_rule`。
        let head = rest[..at].to_string();
        out.push_str(&rewrite_css_block(&head, scope, &base, book_url, asset_paths));

        let tail = &rest[at..];
        // 先分清这个 at-rule 有没有块体。`find_css_at_rule_block` 找的是"第一个
        // `{`", 而 `@charset "...";` / `@import url(...);` 后面紧跟着的普通规则
        // 也有 `{` —— 直接拿它当块体边界, 会把紧随其后的第一条规则(样本里正是
        // `body{}`)当成 at-rule 的内容一起丢掉。判据是分号与 `{` 谁在前。
        let semi = tail.find(';');
        let brace = tail.find('{');
        let has_block = match (semi, brace) {
            (Some(semi), Some(brace)) => brace < semi,
            (None, Some(_)) => true,
            _ => false,
        };
        if !has_block {
            // 无块体的 at-rule(如 `@charset`): 连同分号一起丢弃。
            let end = semi.map(|index| index + 1).unwrap_or(tail.len());
            rest = tail[end..].to_string();
            continue;
        }
        let Some(open) = find_css_at_rule_block(tail) else {
            rest = String::new();
            continue;
        };
        let name = css_at_rule_name(tail);
        let body = &tail[open.0 + 1..open.1];
        match name.as_str() {
            // 媒体查询/文档条件: 保留条件, 但里面的选择器同样要加作用域。
            name if name.starts_with("media") || name.starts_with("supports") => {
                out.push_str(&tail[..open.0 + 1]);
                out.push_str(&rewrite_css_block(body, scope, &base, book_url, asset_paths));
                out.push('}');
            }
            // `@import` 的内容由 `collect_stylesheet_paths` 单独收集; `@font-face`
            // 指向的字体文件不在资源白名单里, 内联后只会产生 404 请求。
            _ => {}
        }
        rest = tail[open.1 + 1..].to_string();
    }

    out.push_str(&rewrite_css_block(&rest, scope, &base, book_url, asset_paths));
    out
}

/// 逐条改写普通规则块。容错处理不配对的括号, 不让一份畸形 CSS 拖垮整章导入。
fn rewrite_css_block(
    block: &str,
    scope: &str,
    base: &str,
    book_url: &str,
    asset_paths: &HashSet<String>,
) -> String {
    let mut out = String::with_capacity(block.len());
    let mut rest = block;

    while let Some(open) = rest.find('{') {
        let Some(close) = find_matching_brace(rest, open) else {
            break;
        };
        let selector = rest[..open].trim();
        let declarations = &rest[open + 1..close];
        if !selector.is_empty() && !declarations.trim().is_empty() {
            let scoped = scope_selector_list(selector, scope);
            if !scoped.is_empty() {
                out.push_str(&scoped);
                out.push('{');
                out.push_str(&rewrite_css_declarations(declarations, base, book_url, asset_paths));
                out.push('}');
            }
        }
        rest = &rest[close + 1..];
    }

    out
}

/// 逗号分隔的选择器组逐个加前缀; 无法安全前缀的选择器(如 `@` 开头的残留)被丢弃。
fn scope_selector_list(selector: &str, scope: &str) -> String {
    let parts: Vec<String> = selector
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty() && !part.starts_with('@'))
        // 关键帧里的 `from`/`to`/`50%` 不是选择器, 加了前缀会生成无效规则。
        .filter(|part| !part.starts_with("from") && !part.starts_with("to") && !part.ends_with('%'))
        .map(|part| {
            if part == "*" {
                return format!("{scope} *");
            }
            // `body`/`html` 描述的是「整篇文档」, 在这里就是章节容器本身。
            // 加前缀写成 `.scope body` 会选中不存在的后代元素, 样式整体失效。
            let lower = part.to_ascii_lowercase();
            if matches!(lower.as_str(), "body" | "html" | "body,html" | "html,body") {
                return scope.to_string();
            }
            if let Some(rest) = lower
                .strip_prefix("body ")
                .or_else(|| lower.strip_prefix("html "))
            {
                // `body p{...}` 等价于容器内的 `p`; 用原始大小写拼接其余选择器。
                let offset = part.len() - rest.len();
                return format!("{scope} {}", &part[offset..]);
            }
            format!("{scope} {part}")
        })
        .collect();
    parts.join(",")
}

fn rewrite_css_declarations(
    declarations: &str,
    base: &str,
    book_url: &str,
    asset_paths: &HashSet<String>,
) -> String {
    let mut out = String::with_capacity(declarations.len());
    for declaration in split_css_declarations(declarations) {
        let Some((property, value)) = declaration.split_once(':') else {
            continue;
        };
        let property = property.trim();
        if property.is_empty()
            || property.starts_with("duokan-")
            || property.starts_with("-epub-")
            || property.starts_with("-webkit-")
            || property.starts_with("-moz-")
            || property.starts_with("-ms-")
            || property.starts_with("-o-")
            // 老 IE 的脚本执行入口, 不依赖前缀规则, 显式拒绝。
            || property.eq_ignore_ascii_case("behavior")
        {
            continue;
        }
        // 字体由阅读器的「字体」设置决定。原书的 font-family 若留着, 会以更高优先级
        // 命中正文元素(它在作用域选择器里, 而阅读器设置写在容器上), 用户换字体就没有
        // 任何反应。`font` 简写必然含字体族, 一并丢弃 —— 代价是它自带的字号也失效,
        // 但保留它就等于字体改不动。
        if property.eq_ignore_ascii_case("font-family") || property.eq_ignore_ascii_case("font") {
            continue;
        }
        let value = value.trim();
        let lowered = value.to_ascii_lowercase();
        if lowered.contains("expression(") || lowered.contains("javascript:") {
            continue;
        }
        let value = rewrite_css_urls(value, base, book_url, asset_paths);
        if value.is_empty() {
            continue;
        }
        out.push_str(property);
        out.push(':');
        out.push_str(&value);
        out.push(';');
    }
    out
}

/// 把 `url(...)` 里的相对路径改写成资源接口地址; 解析不到的引用整条声明由调用方丢弃。
fn rewrite_css_urls(value: &str, base: &str, book_url: &str, asset_paths: &HashSet<String>) -> String {
    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let re = URL_RE.get_or_init(|| {
        Regex::new(r#"(?i)url\(\s*(?:'([^']*)'|"([^"]*)"|([^)'"]*))\s*\)"#)
            .expect("css url regex")
    });

    let mut out = String::with_capacity(value.len());
    let mut last = 0usize;
    for captures in re.captures_iter(value) {
        let whole = captures.get(0).expect("whole match");
        let raw = captures
            .get(1)
            .or_else(|| captures.get(2))
            .or_else(|| captures.get(3))
            .map(|m| m.as_str().trim())
            .unwrap_or_default();
        out.push_str(&value[last..whole.start()]);
        match join_zip_path(base, raw).filter(|path| asset_paths.contains(path)) {
            Some(path) => {
                out.push_str(&format!("url(\"{}\")", build_asset_url(book_url, &path)));
            }
            None => {
                // 指向字体/音频等未收录资源的 url(): 整条声明已无意义, 用占位值标记后由调用方丢弃。
                return String::new();
            }
        }
        last = whole.end();
    }
    out.push_str(&value[last..]);
    out
}

fn split_css_declarations(declarations: &str) -> Vec<&str> {
    declarations
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn find_matching_brace(css: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, ch) in css[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// 返回 at-rule 块体的 `{` 位置与配对的 `}` 位置。
fn find_css_at_rule_block(tail: &str) -> Option<(usize, usize)> {
    let open = tail.find('{')?;
    let close = find_matching_brace(tail, open)?;
    Some((open, close))
}

fn css_at_rule_name(rule: &str) -> String {
    rule.trim_start_matches('@')
        .split(|ch: char| ch == '{' || ch == ';' || ch.is_whitespace())
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

fn strip_css_comments(css: &str) -> String {
    static COMMENT_RE: OnceLock<Regex> = OnceLock::new();
    let re = COMMENT_RE.get_or_init(|| Regex::new(r"(?s)/\*.*?\*/").expect("css comment regex"));
    re.replace_all(css, "").into_owned()
}

fn resolve_cover_path(package: &EpubPackage) -> Option<String> {
    // `<meta name="cover">` 的值可能是 manifest id, 也可能直接是文件名/相对路径。
    // 只看 id 会让后一种写法的封面整本丢失(表现为书库无封面、首屏空白)。
    if let Some(raw) = package.cover_meta.as_deref().filter(|v| !v.trim().is_empty()) {
        let raw = raw.trim();
        if let Some(path) = package.manifest.get(raw).map(|item| item.path.clone()) {
            return Some(path);
        }
        if let Some(path) = find_manifest_path_by_href(package, raw) {
            return Some(path);
        }
    }

    package
        .cover_id
        .as_ref()
        .and_then(|id| package.manifest.get(id))
        .map(|item| item.path.clone())
        .or_else(|| {
            package
                .manifest
                .values()
                .find(|item| {
                    item.properties
                        .split_whitespace()
                        .any(|value| value == "cover-image")
                })
                .map(|item| item.path.clone())
        })
        .or_else(|| {
            package
                .manifest
                .values()
                .find(|item| {
                    item.path.to_lowercase().contains("cover") && is_image_path(&item.path)
                })
                .map(|item| item.path.clone())
        })
        .or_else(|| {
            package
                .image_items
                .iter()
                .filter_map(|id| package.manifest.get(id))
                .map(|item| item.path.clone())
                .next()
        })
}

/// 按 manifest 里的 href 文件名反查路径, 容忍 `Images/cover.jpg` 与 `cover.jpg` 两种写法。
fn find_manifest_path_by_href(package: &EpubPackage, href: &str) -> Option<String> {
    let wanted = href.trim().trim_start_matches("./");
    let wanted_name = wanted.rsplit('/').next().unwrap_or(wanted);
    package
        .manifest
        .values()
        .filter(|item| is_image_path(&item.path))
        .find(|item| {
            let path_name = item.path.rsplit('/').next().unwrap_or(item.path.as_str());
            item.path == wanted || path_name.eq_ignore_ascii_case(wanted_name)
        })
        .map(|item| item.path.clone())
}

fn book_from_index(index: StoredEpubIndex) -> Book {
    Book {
        name: index.name,
        author: index.author,
        book_url: index.book_url.clone(),
        origin: LOCAL_EPUB_ORIGIN.to_string(),
        origin_name: Some(LOCAL_EPUB_ORIGIN_NAME.to_string()),
        cover_url: index.cover_url,
        toc_url: Some(index.book_url),
        can_update: Some(false),
        dur_chapter_index: Some(0),
        dur_chapter_pos: Some(0),
        total_chapter_num: Some(index.chapters.len() as i32),
        latest_chapter_title: index.chapters.last().map(|chapter| chapter.title.clone()),
        kind: Some("本地EPUB".to_string()),
        // 正文按需解压, 导入阶段不统计字数: 0 表示未知, 不显示。
        word_count: (index.char_len > 0).then(|| format!("{}字", index.char_len)),
        ..Book::default()
    }
}

fn parse_chapter_url(chapter_url: &str) -> Result<(String, i32), AppError> {
    let (book_url, raw_index) = chapter_url
        .rsplit_once('#')
        .ok_or_else(|| AppError::BadRequest("章节地址无效".to_string()))?;
    if !is_local_epub_url(book_url) {
        return Err(AppError::BadRequest("章节地址无效".to_string()));
    }
    let index = raw_index
        .parse::<i32>()
        .map_err(|_| AppError::BadRequest("章节序号无效".to_string()))?;
    Ok((book_url.to_string(), index))
}

fn local_epub_hash_from_url(book_url: &str) -> Result<&str, AppError> {
    let hash = book_url
        .strip_prefix("local-epub:")
        .filter(|value| {
            value.len() == LOCAL_EPUB_HASH_LEN && value.chars().all(|ch| ch.is_ascii_hexdigit())
        })
        .ok_or_else(|| AppError::BadRequest("本地 EPUB 地址无效".to_string()))?;
    Ok(hash)
}

fn map_local_epub_read_error(err: std::io::Error) -> AppError {
    if err.kind() == std::io::ErrorKind::NotFound {
        AppError::BadRequest("本地 EPUB 不存在".to_string())
    } else {
        AppError::Internal(err.into())
    }
}

fn md5_bytes_hex(prefix: &[u8], bytes: &[u8]) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(prefix);
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn normalize_strict_zip_path(path: &str) -> Option<String> {
    let value = path.replace('\\', "/");
    if value.starts_with('/') || value.contains('\0') {
        return None;
    }
    let mut parts = Vec::new();
    for part in value.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        parts.push(part);
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn join_zip_path(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty()
        || href.starts_with('#')
        || href.starts_with("http:")
        || href.starts_with("https:")
        || href.starts_with("data:")
        || href.starts_with("javascript:")
    {
        return None;
    }
    let href = href.split('#').next().unwrap_or(href);
    let href = href.split('?').next().unwrap_or(href);
    let decoded = urlencoding::decode(href).unwrap_or(Cow::Borrowed(href));
    let raw = if base.is_empty() {
        decoded.into_owned()
    } else {
        format!("{base}/{decoded}")
    };
    normalize_joined_zip_path(&raw)
}

fn normalize_joined_zip_path(path: &str) -> Option<String> {
    let value = path.replace('\\', "/");
    if value.starts_with('/') || value.contains('\0') {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in value.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop()?;
        } else {
            parts.push(part);
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn safe_child_path(root: &Path, relative: &str) -> Result<PathBuf, AppError> {
    let path = normalize_strict_zip_path(relative)
        .ok_or_else(|| AppError::BadRequest("EPUB 资源路径无效".to_string()))?;
    let mut out = root.to_path_buf();
    for part in path.split('/') {
        out.push(part);
    }
    Ok(out)
}

fn parent_zip_dir(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

fn local_name(name: &[u8]) -> String {
    let value = std::str::from_utf8(name).unwrap_or_default();
    value
        .rsplit_once(':')
        .map(|(_, local)| local)
        .unwrap_or(value)
        .to_string()
}

fn attr_by_name(e: &BytesStart<'_>, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|attr| local_name(attr.key.as_ref()) == name)
        .and_then(|attr| attr_value(&attr))
}

fn attr_value(attr: &Attribute<'_>) -> Option<String> {
    let value = std::str::from_utf8(attr.value.as_ref()).ok()?;
    Some(unescape_xml_attr(value))
}

fn is_html_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "application/xhtml+xml" | "text/html" | "application/xml" | "text/xml"
    )
}

fn is_image_path(path: &str) -> bool {
    matches!(
        path.rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "svg" | "bmp"
    )
}

fn is_css_path(path: &str) -> bool {
    path.rsplit('.')
        .next()
        .unwrap_or_default()
        .eq_ignore_ascii_case("css")
}

fn content_type_for_path(path: &str) -> String {
    match path
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn build_asset_url(book_url: &str, path: &str) -> String {
    format!(
        "/reader3/localEpubAsset?bookUrl={}&path={}",
        urlencoding::encode(book_url),
        urlencoding::encode(path)
    )
}

fn is_allowed_html_tag(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "article"
            | "aside"
            | "b"
            | "blockquote"
            | "br"
            | "caption"
            | "code"
            | "div"
            | "em"
            | "figcaption"
            | "figure"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "i"
            | "img"
            | "image"
            | "li"
            | "ol"
            | "p"
            | "pre"
            | "rp"
            | "rt"
            | "ruby"
            | "s"
            | "section"
            | "span"
            | "strong"
            | "sub"
            | "sup"
            | "svg"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "tr"
            | "u"
            | "ul"
    )
}

fn is_void_html_tag(tag: &str) -> bool {
    matches!(tag, "br" | "hr" | "img" | "image")
}

fn should_skip_tag(tag: &str) -> bool {
    matches!(tag, "script" | "style" | "head")
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attr(value: &str) -> String {
    escape_html(value).replace('"', "&quot;")
}

fn unescape_xml_attr(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn collapse_ws(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_utf8_lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// EPUB 的章节文件通常自带 `<h1>`/`<h2>` 章节名，而阅读页会统一渲染目录标题。
/// 只移除正文开头那个与目录标题相同的标题块，避免"标题显示两次"；正文中间的
/// 小标题保持原样。
///
/// 判定方式是"标题之前没有可见文字"而不是"标题必须是第一个元素"：多看的排版
/// 常把 logo 图、空 div 放在标题前面，旧实现因此整段放弃去重。
fn strip_leading_duplicate_title(html: &str, chapter_title: &str) -> String {
    let expected = normalize_title_text(chapter_title);
    if expected.is_empty() {
        return html.to_string();
    }

    for (start, tag) in leading_title_candidates(html) {
        let open_end = match html[start..].find('>') {
            Some(index) => start + index + 1,
            None => continue,
        };
        let Some(close_len) = find_matching_title_close(&html[start..], &tag, open_end - start)
        else {
            continue;
        };
        let block_end = start + close_len;

        // 标题之前只能是空标签和空白: 一旦出现真正的文字, 说明正文已经开始,
        // 这个标题就是正文里的小标题, 不能删。
        if !has_visible_text(&html[..start]) {
            if leading_title_matches(&html[start..block_end], &expected) {
                let mut result = String::with_capacity(html.len());
                result.push_str(&html[..start]);
                result.push_str(html[block_end..].trim_start());
                return result;
            }
        }
    }

    html.to_string()
}

/// 片段里是否有可见文字。图片/分隔线等空元素不算: 多看排版会在标题前放 logo。
fn has_visible_text(fragment: &str) -> bool {
    !normalize_title_text(&html_unescape(&strip_html_tags_without_regex(fragment))).is_empty()
}

/// 正文开头允许出现的章节标题元素位置，按出现顺序返回 `(起始偏移, 标签名)`。
///
/// 只在开头一小段里找：正文中段的 `<h2>` 小标题不该被当成章节标题处理。
fn leading_title_candidates(html: &str) -> Vec<(usize, String)> {
    static TITLE_TAG_RE: OnceLock<Regex> = OnceLock::new();
    let re = TITLE_TAG_RE.get_or_init(|| {
        Regex::new(r#"(?i)<h[1-6]\b[^>]*>|<p\b[^>]*\bclass\s*=\s*(?:"[^"]*"|'[^']*')[^>]*>"#)
            .expect("title tag regex")
    });

    // 章节标题几乎总在正文最前面：封面 logo + 标题 + 少量说明都在 4KB 内。
    let limit = html.len().min(4096);
    let head = &html[..floor_char_boundary(html, limit)];
    re.find_iter(head)
        .filter_map(|m| {
            let tag_html = m.as_str();
            let tag = tag_name_of(tag_html);
            if tag.is_empty() {
                return None;
            }
            // `<p>` 只有 class 里带 title 才算标题候选。这里按空白切词比对,
            // 不能用 `(?:^|\s)title` 那种正则: `^` 在字符类消费之后永远不成立,
            // `class="title"`(整个值就是 title)会被漏掉。
            if tag == "p" && !class_has_title_token(tag_html) {
                return None;
            }
            Some((m.start(), tag))
        })
        .take(4)
        .collect()
}

fn class_has_title_token(tag_html: &str) -> bool {
    static CLASS_RE: OnceLock<Regex> = OnceLock::new();
    let re = CLASS_RE.get_or_init(|| {
        Regex::new(r#"(?i)\bclass\s*=\s*(?:"([^"]*)"|'([^']*)')"#).expect("class attr regex")
    });
    let Some(class) = re
        .captures(tag_html)
        .and_then(|captures| captures.get(1).or_else(|| captures.get(2)))
        .map(|m| m.as_str())
    else {
        return false;
    };
    class
        .split_whitespace()
        .any(|token| token.eq_ignore_ascii_case("title") || token.eq_ignore_ascii_case("chapter-title"))
}

/// 取不超过 `limit` 的最大字符边界，避免在多字节字符中间切片。
fn floor_char_boundary(value: &str, limit: usize) -> usize {
    if limit >= value.len() {
        return value.len();
    }
    let mut index = limit;
    while index > 0 && !value.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn tag_name_of(opening: &str) -> String {
    opening
        .trim_start_matches('<')
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_end_matches('/')
        .trim_end_matches('>')
        .to_ascii_lowercase()
}

/// 标题块是否与目录标题一致。先把 `<br/>` 视作分隔符拆出所有片段, 用「整段去空白」
/// 与「按 br 分段后拼接」两种口径比较。
fn leading_title_matches(title_html: &str, expected: &str) -> bool {
    let whole = normalize_title_text(&html_unescape(&strip_br_as_space(title_html)));
    if whole == expected {
        return true;
    }

    let fragments = split_br_fragments(title_html);
    if fragments.len() > 1 {
        let joined = normalize_title_text(&fragments.concat());
        if joined == expected {
            return true;
        }
        // 目录标题常把「第001章」与正文标题用空格连起来: 去掉空白后逐段包含也要认。
        let joined_no_space: String = fragments.concat();
        if !joined_no_space.is_empty() && expected.contains(&joined) {
            return true;
        }
    }

    // 目录标题可能只是正文标题的一部分(或反之), 允许一个是另一个的前缀式包含。
    if !whole.is_empty() && expected.contains(&whole) && whole.chars().count() >= 2 {
        return true;
    }
    false
}

fn strip_br_as_space(html: &str) -> String {
    static BR_RE: OnceLock<Regex> = OnceLock::new();
    let re = BR_RE.get_or_init(|| Regex::new(r"(?i)<\s*br\s*/?\s*>").expect("br regex"));
    let with_space = re.replace_all(html, " ").into_owned();
    html_unescape(&strip_html_tags_without_regex(&with_space))
}

/// 按 `<br/>` 把标题块切成若干纯文本片段。
fn split_br_fragments(title_html: &str) -> Vec<String> {
    static BR_RE: OnceLock<Regex> = OnceLock::new();
    let re = BR_RE.get_or_init(|| Regex::new(r"(?i)<\s*br\s*/?\s*>").expect("br regex"));
    re.split(title_html)
        .map(|fragment| normalize_title_text(&html_unescape(&strip_html_tags_without_regex(fragment))))
        .filter(|fragment| !fragment.is_empty())
        .collect()
}

fn strip_html_tags_without_regex(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let chars: Vec<char> = html.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '<' {
            if let Some(offset) = chars[index + 1..].iter().position(|ch| *ch == '>') {
                index += offset + 2;
                continue;
            }
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

fn find_matching_title_close(html: &str, tag: &str, open_end: usize) -> Option<usize> {
    let close = format!("</{tag}>");
    let lower = html.to_ascii_lowercase();
    lower[open_end..]
        .find(&close)
        .map(|offset| open_end + offset + close.len())
}

fn normalize_title_text(value: &str) -> String {
    collapse_ws(value)
        .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '“' | '”' | '‘' | '’'))
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

fn html_unescape(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::{extract_html_title, is_meaningful_epub_title, strip_leading_duplicate_title};

    #[test]
    fn removes_matching_leading_heading_only() {
        let html = "<h1>第一章 起点</h1><p>正文</p><h2>文中小标题</h2><p>后续</p>";
        let result = strip_leading_duplicate_title(html, "第一章 起点");
        assert_eq!(result, "<p>正文</p><h2>文中小标题</h2><p>后续</p>");
    }

    #[test]
    fn keeps_body_heading_when_it_differs_from_chapter_title() {
        let html = "<h1>另一个标题</h1><p>正文</p>";
        assert_eq!(
            strip_leading_duplicate_title(html, "第一章 起点"),
            html,
        );
    }

    #[test]
    fn handles_whitespace_and_class_title_paragraph() {
        let html = "\n  <p class=\"chapter-title\"> 第一章   起点 </p><p>正文</p>";
        let result = strip_leading_duplicate_title(html, "第一章 起点");
        assert_eq!(result, "\n  <p>正文</p>");
    }

    /// 多看/杜坎排版: 标题前有 logo 容器, 标题本身被 `<br/>` 拆成两段。
    #[test]
    fn removes_heading_split_by_br_behind_leading_logo() {
        let html = "<div>\n  <div class=\"logo\"><img src=\"logo.png\"/></div>\n  \
<h1 class=\"head\">第001章<br/>\n    <b>星空中的青铜巨棺</b></h1>\n  <p>正文</p>\n</div>";
        let result = strip_leading_duplicate_title(html, "第001章 星空中的青铜巨棺");
        assert!(!result.contains("<h1"));
        assert!(result.contains("class=\"logo\""));
        assert!(result.contains("<p>正文</p>"));
    }

    /// 正文已经开始后才出现的小标题不能被当成章节标题删掉。
    #[test]
    fn keeps_heading_that_follows_body_text() {
        let html = "<p>正文在前</p><h1>第一章 起点</h1>";
        assert_eq!(strip_leading_duplicate_title(html, "第一章 起点"), html);
    }

    #[test]
    fn rejects_unknown_toc_placeholder() {
        assert!(!is_meaningful_epub_title("未知"));
        assert!(!is_meaningful_epub_title("Untitled"));
        assert!(is_meaningful_epub_title("示例章节标题"));
    }

    #[test]
    fn extracts_real_heading_instead_of_unknown_html_title() {
        let html = "<html><head><title>未知</title></head><body><h1>示例章节标题</h1></body></html>";
        assert_eq!(extract_html_title(html).as_deref(), Some("示例章节标题"));
    }

    /// `class="title"`(整个 class 值就是 title) 也要认出来。
    #[test]
    fn removes_leading_paragraph_whose_class_is_exactly_title() {
        let html = "<p class=\"title\">第一章 起点</p><p>正文</p>";
        assert_eq!(
            strip_leading_duplicate_title(html, "第一章 起点"),
            "<p>正文</p>",
        );
    }
}
