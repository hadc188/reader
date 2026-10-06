use std::sync::Arc;

use tokio::sync::Mutex;

use crate::error::error::AppError;
use crate::model::book_group::BookGroup;
use crate::service::json_document_service::JsonDocumentService;

pub struct BookGroupService {
    docs: Arc<JsonDocumentService>,
    /// 串行化所有「读列表 → 改 → 写回」的操作。
    ///
    /// save_group / delete_group / delete_groups 都是读改写, 而 set_raw 只是
    /// INSERT ... ON CONFLICT DO UPDATE —— 它保证单条写入原子, 但不保证读改写原子。
    /// 两个并发请求会各自读到同一份旧列表再先后写回, 后者覆盖前者:
    /// A 读 [1,2,3]、B 读 [1,2,3]、A 写 [2,3]、B 写 [1,3] → 分组 1 复活。
    /// 备份恢复曾用 Promise.all 并发逐个删分组, 正好撞上这条路径, 表现为
    /// 「恢复后旧分组残留」。BookService 早有 bookshelf_write_lock 保护同一类操作,
    /// 这里补上同一约定。
    write_lock: Arc<Mutex<()>>,
}

impl BookGroupService {
    pub fn new(docs: Arc<JsonDocumentService>) -> Self {
        Self {
            docs,
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn get_groups(&self, user_ns: &str) -> Result<Vec<BookGroup>, AppError> {
        self.docs.read_list(user_ns, "book_groups.json").await
    }

    /// 整表覆盖。调用方已完成读改写, 所以这里只需排他写 —— 但**仍然持锁**,
    /// 否则一次整表覆盖可能与另一个读改写交错。
    pub async fn save_groups(
        &self,
        user_ns: &str,
        groups: &Vec<BookGroup>,
    ) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().await;
        self.docs
            .write_list(user_ns, "book_groups.json", groups)
            .await
    }

    pub async fn save_group(&self, user_ns: &str, mut group: BookGroup) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().await;
        let mut groups = self.get_groups(user_ns).await?;
        if group.group_id == 0 {
            let max_id = groups.iter().map(|g| g.group_id).max().unwrap_or(0);
            group.group_id = max_id + 1;
        }
        let mut found = false;
        for g in &mut groups {
            if g.group_id == group.group_id {
                g.group_name = group.group_name.clone();
                g.order_no = group.order_no;
                found = true;
                break;
            }
        }
        if !found {
            groups.push(group);
        }
        self.write_groups_locked(user_ns, &groups).await
    }

    pub async fn delete_group(&self, user_ns: &str, group_id: i64) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().await;
        let mut groups = self.get_groups(user_ns).await?;
        groups.retain(|g| g.group_id != group_id);
        self.write_groups_locked(user_ns, &groups).await
    }

    /// 一次删除多个分组。
    ///
    /// 只读一次、只写一次 —— 逐个调用 `delete_group` 会对同一份列表反复读改写,
    /// 删 N 个分组就是 N 次全量重写, 其间失败还会留下删了一半的状态。
    /// 返回实际删掉的条数。
    pub async fn delete_groups(
        &self,
        user_ns: &str,
        group_ids: &[i64],
    ) -> Result<usize, AppError> {
        if group_ids.is_empty() {
            return Ok(0);
        }
        // 整个读改写过程持锁: 并发调用时若各自读到同一份旧列表, 后写的会覆盖先写的。
        let _guard = self.write_lock.lock().await;
        let mut groups = self.get_groups(user_ns).await?;
        let before = groups.len();
        groups.retain(|g| !group_ids.contains(&g.group_id));
        let removed = before - groups.len();
        // 一个都没匹配上时不必重写文件。
        if removed > 0 {
            // 注意用 _locked 版本: save_groups 也拿同一把锁, 在持锁状态下调它会死锁。
            self.write_groups_locked(user_ns, &groups).await?;
        }
        Ok(removed)
    }

    /// 落盘分组列表。**要求调用方已持有 write_lock** —— 读改写方法持锁后必须走
    /// 这个入口, 不能再调 save_groups(它会重复加锁而死锁)。
    async fn write_groups_locked(
        &self,
        user_ns: &str,
        groups: &Vec<BookGroup>,
    ) -> Result<(), AppError> {
        self.docs
            .write_list(user_ns, "book_groups.json", groups)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // 测试夹具专用的依赖放在测试模块里: 放在文件顶部会让 `cargo build` 报
    // unused import(非测试构建不编译这个模块)。
    use std::path::PathBuf;
    use crate::storage::db;

    async fn create_service() -> (BookGroupService, PathBuf) {
        // 每个测试必须用**独立**目录: 同一进程内 process::id() 是一样的, 并行执行时
        // 会共用同一个 reader.db 与 book_groups.json, 互相覆盖导致随机失败。
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp_dir = std::env::temp_dir().join(format!(
            "reader-rust-group-service-{}-{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let database_url = format!("sqlite:{}?mode=rwc", temp_dir.join("reader.db").display());
        let pool = db::init_pool(&database_url).await.unwrap();
        let docs = Arc::new(JsonDocumentService::new(pool, &temp_dir.to_string_lossy()));
        (BookGroupService::new(docs), temp_dir)
    }

    fn group(id: i64, name: &str) -> BookGroup {
        BookGroup {
            group_id: id,
            group_name: name.to_string(),
            order_no: 0,
        }
    }

    fn ids(list: &[BookGroup]) -> Vec<i64> {
        list.iter().map(|g| g.group_id).collect()
    }

    #[tokio::test]
    async fn deletes_every_requested_group_in_one_pass() {
        let (service, dir) = create_service().await;
        service
            .save_groups("default", &vec![group(1, "玄幻"), group(2, "都市"), group(3, "历史")])
            .await
            .unwrap();

        let removed = service.delete_groups("default", &[1, 3]).await.unwrap();

        assert_eq!(removed, 2);
        assert_eq!(ids(&service.get_groups("default").await.unwrap()), vec![2]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn reports_only_ids_that_actually_existed() {
        // 计数必须反映真实删除数, 否则前端提示会虚报。
        let (service, dir) = create_service().await;
        service
            .save_groups("default", &vec![group(1, "玄幻")])
            .await
            .unwrap();

        let removed = service
            .delete_groups("default", &[1, 99, 100])
            .await
            .unwrap();

        assert_eq!(removed, 1);
        assert!(service.get_groups("default").await.unwrap().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn empty_selection_is_a_noop() {
        let (service, dir) = create_service().await;
        service
            .save_groups("default", &vec![group(1, "玄幻")])
            .await
            .unwrap();

        assert_eq!(service.delete_groups("default", &[]).await.unwrap(), 0);
        assert_eq!(ids(&service.get_groups("default").await.unwrap()), vec![1]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn deleting_all_groups_leaves_an_empty_list() {
        let (service, dir) = create_service().await;
        service
            .save_groups("default", &vec![group(1, "玄幻"), group(2, "都市")])
            .await
            .unwrap();

        assert_eq!(service.delete_groups("default", &[1, 2]).await.unwrap(), 2);
        assert!(service.get_groups("default").await.unwrap().is_empty());
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn keeps_legado_negative_ids_distinguishable() {
        // Legado 用负数作保留分组(IdLocal=-2 等)。批量删除必须能按原值精确匹配,
        // 不能因为「负数看起来像无效值」就被顺手清掉。
        let (service, dir) = create_service().await;
        service
            .save_groups("default", &vec![group(-2, "本地"), group(1, "玄幻"), group(-1, "全部")])
            .await
            .unwrap();

        let removed = service.delete_groups("default", &[-2]).await.unwrap();

        assert_eq!(removed, 1);
        assert_eq!(
            ids(&service.get_groups("default").await.unwrap()),
            vec![1, -1]
        );
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn concurrent_group_deletions_do_not_resurrect_removed_groups() {
        // 回归测试: 读改写无锁时, 两个并发删除会各自读到同一份旧列表再先后写回,
        // 后写的覆盖先写的 —— 被先删掉的分组「复活」。
        //   A 读 [1,2,3]、B 读 [1,2,3]、A 写 [2,3]、B 写 [1,3]  → 分组 1 又回来了
        // 备份恢复曾用 Promise.all 并发逐个删分组, 正好撞上这条路径, 表现为
        // 「恢复后旧分组残留」。这里并发删 1 和 2, 断言两者都真的消失。
        let (service, dir) = create_service().await;
        service
            .save_groups(
                "default",
                &vec![group(1, "玄幻"), group(2, "都市"), group(3, "历史")],
            )
            .await
            .unwrap();

        // 交替并发删 1 与 2 多轮, 提高交错概率。
        for _ in 0..12 {
            service.save_groups(
                "default",
                &vec![group(1, "玄幻"), group(2, "都市"), group(3, "历史")],
            )
            .await
            .unwrap();
            let (a, b) = tokio::join!(
                service.delete_group("default", 1),
                service.delete_group("default", 2),
            );
            a.unwrap();
            b.unwrap();

            let left = ids(&service.get_groups("default").await.unwrap());
            assert!(
                !left.contains(&1),
                "分组 1 被并发删除后复活了, 剩余: {left:?}"
            );
            assert!(
                !left.contains(&2),
                "分组 2 被并发删除后复活了, 剩余: {left:?}"
            );
            assert_eq!(left, vec![3], "只应剩编号 3 的分组");
        }
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn bulk_and_single_deletion_mixed_concurrently_stay_consistent() {
        // 另一条交错路径: 批量删 与 单个删 同时发出。
        let (service, dir) = create_service().await;
        for _ in 0..12 {
            service
                .save_groups(
                    "default",
                    &vec![group(1, "A"), group(2, "B"), group(3, "C"), group(4, "D")],
                )
                .await
                .unwrap();
            let (bulk, single) = tokio::join!(
                service.delete_groups("default", &[1, 2]),
                service.delete_group("default", 3),
            );
            bulk.unwrap();
            single.unwrap();

            let left = ids(&service.get_groups("default").await.unwrap());
            assert_eq!(left, vec![4], "应只剩编号 4 的分组, 实际: {left:?}");
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
