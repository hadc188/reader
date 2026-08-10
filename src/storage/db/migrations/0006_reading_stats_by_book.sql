CREATE TABLE IF NOT EXISTS reading_book_stats (
    user_ns TEXT NOT NULL,
    date TEXT NOT NULL,
    book_url TEXT NOT NULL,
    book_name TEXT NOT NULL DEFAULT '',
    seconds INTEGER NOT NULL DEFAULT 0,
    characters INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_ns, date, book_url)
);

CREATE INDEX IF NOT EXISTS idx_reading_book_stats_user_ns_date
ON reading_book_stats(user_ns, date);
