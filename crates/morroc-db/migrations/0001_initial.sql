-- Morroc 初始数据库结构（SQLite）
-- 基于 Hercules 实体设计，但使用 SQLite 原生语法。

-- 账户表
CREATE TABLE IF NOT EXISTS login (
    account_id INTEGER PRIMARY KEY AUTOINCREMENT,
    userid TEXT NOT NULL,
    user_pass TEXT NOT NULL,
    sex TEXT NOT NULL DEFAULT 'M',
    email TEXT DEFAULT '',
    group_id INTEGER NOT NULL DEFAULT 0,
    state INTEGER NOT NULL DEFAULT 0,
    unban_time INTEGER NOT NULL DEFAULT 0,
    expiration_time INTEGER NOT NULL DEFAULT 0,
    logincount INTEGER NOT NULL DEFAULT 0,
    lastlogin INTEGER,
    last_ip TEXT DEFAULT '',
    birthdate TEXT,
    pincode TEXT DEFAULT '',
    pincode_change INTEGER NOT NULL DEFAULT 0,
    vip_time INTEGER NOT NULL DEFAULT 0,
    old_group INTEGER NOT NULL DEFAULT 0,
    web_auth_token TEXT DEFAULT '',
    web_auth_token_enabled INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_login_userid ON login(userid);

-- 角色表
CREATE TABLE IF NOT EXISTS char (
    char_id INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id INTEGER NOT NULL,
    char_num INTEGER NOT NULL DEFAULT 0,
    name TEXT NOT NULL,
    class INTEGER NOT NULL DEFAULT 0,
    base_level INTEGER NOT NULL DEFAULT 1,
    job_level INTEGER NOT NULL DEFAULT 1,
    base_exp INTEGER NOT NULL DEFAULT 0,
    job_exp INTEGER NOT NULL DEFAULT 0,
    zeny INTEGER NOT NULL DEFAULT 0,
    str INTEGER NOT NULL DEFAULT 1,
    agi INTEGER NOT NULL DEFAULT 1,
    vit INTEGER NOT NULL DEFAULT 1,
    int INTEGER NOT NULL DEFAULT 1,
    dex INTEGER NOT NULL DEFAULT 1,
    luk INTEGER NOT NULL DEFAULT 1,
    max_hp INTEGER NOT NULL DEFAULT 40,
    hp INTEGER NOT NULL DEFAULT 40,
    max_sp INTEGER NOT NULL DEFAULT 0,
    sp INTEGER NOT NULL DEFAULT 0,
    status_point INTEGER NOT NULL DEFAULT 0,
    skill_point INTEGER NOT NULL DEFAULT 0,
    option INTEGER NOT NULL DEFAULT 0,
    karma INTEGER NOT NULL DEFAULT 0,
    manner INTEGER NOT NULL DEFAULT 0,
    party_id INTEGER NOT NULL DEFAULT 0,
    guild_id INTEGER NOT NULL DEFAULT 0,
    pet_id INTEGER NOT NULL DEFAULT 0,
    homun_id INTEGER NOT NULL DEFAULT 0,
    elemental_id INTEGER NOT NULL DEFAULT 0,
    current_map TEXT DEFAULT '',
    current_x INTEGER NOT NULL DEFAULT 0,
    current_y INTEGER NOT NULL DEFAULT 0,
    save_map TEXT DEFAULT '',
    save_x INTEGER NOT NULL DEFAULT 0,
    save_y INTEGER NOT NULL DEFAULT 0,
    renamed INTEGER NOT NULL DEFAULT 0,
    slotchange INTEGER NOT NULL DEFAULT 0,
    weapon INTEGER NOT NULL DEFAULT 1,
    shield INTEGER NOT NULL DEFAULT 0,
    head_top INTEGER NOT NULL DEFAULT 0,
    head_mid INTEGER NOT NULL DEFAULT 0,
    head_bottom INTEGER NOT NULL DEFAULT 0,
    robe INTEGER NOT NULL DEFAULT 0,
    delete_date INTEGER,
    unread_msg INTEGER NOT NULL DEFAULT 0,
    hotkey_rowshift INTEGER NOT NULL DEFAULT 0,
    clan_id INTEGER NOT NULL DEFAULT 0,
    title_id INTEGER NOT NULL DEFAULT 0,
    fame_points INTEGER NOT NULL DEFAULT 0,
    unique_trait_status INTEGER NOT NULL DEFAULT 0,
    honor_token INTEGER NOT NULL DEFAULT 0,
    body_direction INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (strftime('%s','now')),
    updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_char_account_num ON char(account_id, char_num);
CREATE INDEX IF NOT EXISTS idx_char_name ON char(name);

-- 默认管理员账户（用户名 admin / 密码 admin，仅供首次启动使用）
INSERT OR IGNORE INTO login (userid, user_pass, sex, group_id, email)
VALUES ('admin', 'admin', 'M', 99, 'admin@morroc.local');
