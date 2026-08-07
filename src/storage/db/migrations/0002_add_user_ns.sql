CREATE TABLE book_sources_new (
    user_ns TEXT NOT NULL DEFAULT 'default',
    book_source_url TEXT NOT NULL,
    book_source_name TEXT NOT NULL,
    json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_ns, book_source_url)
);

INSERT INTO book_sources_new (user_ns, book_source_url, book_source_name, json, updated_at)
SELECT 'default', book_source_url, book_source_name, json, updated_at FROM book_sources;

DROP TABLE book_sources;
ALTER TABLE book_sources_new RENAME TO book_sources;
