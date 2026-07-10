-- 会话认证表，用于 standalone 与 distributed 模式共享登录/角色/地图会话。
CREATE TABLE IF NOT EXISTS session (
    account_id INTEGER PRIMARY KEY NOT NULL,
    auth_code INTEGER NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);
