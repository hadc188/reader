CREATE TABLE IF NOT EXISTS book_sources (
    book_source_url TEXT PRIMARY KEY,
    book_source_name TEXT NOT NULL,
    json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS book_cache (
    book_url TEXT PRIMARY KEY,
    json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS chapter_cache (
    book_url TEXT NOT NULL,
    chapter_index INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (book_url, chapter_index)
);
