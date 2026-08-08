CREATE TABLE IF NOT EXISTS users (
    username TEXT NOT NULL PRIMARY KEY,
    password TEXT NOT NULL,
    salt TEXT NOT NULL,
    token TEXT NOT NULL DEFAULT '',
    last_login_at INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT 0,
    enable_webdav INTEGER NOT NULL DEFAULT 0,
    enable_local_store INTEGER NOT NULL DEFAULT 0,
    enable_ai_model INTEGER NOT NULL DEFAULT 0,
    is_admin INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS user_sessions (
    username TEXT NOT NULL,
    token TEXT NOT NULL,
    expire_at INTEGER NOT NULL,
    PRIMARY KEY (username, token),
    FOREIGN KEY (username) REFERENCES users(username) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_user_sessions_expire_at
ON user_sessions(expire_at);

CREATE TABLE IF NOT EXISTS json_documents (
    namespace TEXT NOT NULL,
    name TEXT NOT NULL,
    json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (namespace, name)
);

CREATE TABLE IF NOT EXISTS ai_book_memories (
    user_ns TEXT NOT NULL,
    book_key TEXT NOT NULL,
    book_url TEXT NOT NULL,
    json TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (user_ns, book_key)
);

CREATE INDEX IF NOT EXISTS idx_ai_book_memories_user_ns
ON ai_book_memories(user_ns);
