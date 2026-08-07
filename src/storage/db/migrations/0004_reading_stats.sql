CREATE TABLE IF NOT EXISTS reading_stats (
    user_ns TEXT NOT NULL,
    date TEXT NOT NULL,
    seconds INTEGER NOT NULL DEFAULT 0,
    characters INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_ns, date)
);

CREATE INDEX IF NOT EXISTS idx_reading_stats_user_ns_date
ON reading_stats(user_ns, date);
