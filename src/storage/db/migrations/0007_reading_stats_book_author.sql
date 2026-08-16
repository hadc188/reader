-- 为 reading_book_stats 增加 book_author 列, 用于区分不同作者的同名书。
-- 旧数据 book_author 为空串, 分组时回退为 '' 参与(同名无作者的书仍会合并, 可接受)。
ALTER TABLE reading_book_stats ADD COLUMN book_author TEXT NOT NULL DEFAULT '';
