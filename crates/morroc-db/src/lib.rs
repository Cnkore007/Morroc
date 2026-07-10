//! Morroc SQLite 数据库层。
//!
//! 提供连接池、迁移执行，以及游戏持久化所需的常用查询接口。

use async_trait::async_trait;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Pool, Sqlite};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{error, info};

pub use sqlx::Error as SqlxError;

/// 账户记录。
#[derive(Debug, Clone)]
pub struct Account {
    pub account_id: i64,
    pub userid: String,
    pub user_pass: String,
    pub group_id: i64,
    pub sex: String,
}

/// 数据库句柄。
#[derive(Debug, Clone)]
pub struct Database {
    pool: Pool<Sqlite>,
}

impl Database {
    /// 连接到 SQLite 数据库。
    ///
    /// 如果 `path` 所在目录不存在，会自动创建。数据库文件不存在时也会自动创建。
    /// 使用 `:memory:` 可连接内存数据库（主要用于测试）。
    pub async fn connect(path: &str) -> anyhow::Result<Self> {
        let url = if path == ":memory:" {
            "sqlite::memory:".to_string()
        } else {
            let path = std::env::current_dir()?.join(path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if !path.exists() {
                std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(false)
                    .open(&path)?;
            }
            format!("sqlite://{}", path.display())
        };
        info!("正在连接 SQLite 数据库: {}", url);

        let pool = SqlitePoolOptions::new()
            .max_connections(16)
            .connect(&url)
            .await?;

        Ok(Self { pool })
    }

    /// 执行嵌入的 SQL 迁移脚本。
    pub async fn migrate(&self) -> anyhow::Result<()> {
        info!("正在应用数据库迁移...");
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(|e| {
                error!("数据库迁移失败: {}", e);
                anyhow::anyhow!("数据库迁移失败: {}", e)
            })?;
        info!("数据库迁移完成。");
        Ok(())
    }

    /// 获取内部连接池。
    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    /// 统计当前账户数量。
    pub async fn account_count(&self) -> anyhow::Result<i64> {
        let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM login")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }

    /// 按用户名查找账户。
    pub async fn find_account_by_userid(&self, userid: &str) -> anyhow::Result<Option<Account>> {
        let row: Option<(i64, String, String, i64, String)> = sqlx::query_as(
            "SELECT account_id, userid, user_pass, group_id, sex FROM login WHERE userid = ?1",
        )
        .bind(userid)
        .fetch_optional(&self.pool)
        .await?;

        Ok(
            row.map(|(account_id, userid, user_pass, group_id, sex)| Account {
                account_id,
                userid,
                user_pass,
                group_id,
                sex,
            }),
        )
    }

    /// 创建新账户。
    pub async fn create_account(
        &self,
        userid: &str,
        user_pass: &str,
        sex: &str,
    ) -> anyhow::Result<i64> {
        let sex = match sex {
            "M" | "F" => sex,
            _ => "M",
        };
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO login (userid, user_pass, sex, group_id) VALUES (?1, ?2, ?3, 0) RETURNING account_id"
        )
        .bind(userid)
        .bind(user_pass)
        .bind(sex)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    /// 列出所有账户用户名。
    pub async fn list_accounts(&self) -> anyhow::Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as("SELECT userid FROM login ORDER BY account_id")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}

/// 跨模块共享的登录会话存储。
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// 保存或更新 account_id 对应的 auth_code。
    async fn insert_session(&self, account_id: u32, auth_code: i32) -> anyhow::Result<()>;
    /// 查询 account_id 对应的 auth_code。
    async fn get_session(&self, account_id: u32) -> anyhow::Result<Option<i32>>;
    /// 删除 account_id 对应的会话。
    async fn remove_session(&self, account_id: u32) -> anyhow::Result<()>;
    /// 返回当前会话数量。
    async fn session_count(&self) -> usize;
}

#[async_trait]
impl SessionStore for Database {
    async fn insert_session(&self, account_id: u32, auth_code: i32) -> anyhow::Result<()> {
        sqlx::query("INSERT OR REPLACE INTO session (account_id, auth_code) VALUES (?1, ?2)")
            .bind(account_id as i64)
            .bind(auth_code)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn get_session(&self, account_id: u32) -> anyhow::Result<Option<i32>> {
        let row: Option<(i32,)> =
            sqlx::query_as("SELECT auth_code FROM session WHERE account_id = ?1")
                .bind(account_id as i64)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|r| r.0))
    }

    async fn remove_session(&self, account_id: u32) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM session WHERE account_id = ?1")
            .bind(account_id as i64)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn session_count(&self) -> usize {
        let row: Option<(i64,)> = sqlx::query_as("SELECT COUNT(*) FROM session")
            .fetch_optional(&self.pool)
            .await
            .unwrap_or(None);
        row.map(|r| r.0 as usize).unwrap_or(0)
    }
}

/// 单机内存会话存储，用于 standalone 模式减少数据库写入。
#[derive(Debug, Clone)]
pub struct LocalSessionStore {
    inner: Arc<Mutex<HashMap<u32, i32>>>,
}

impl LocalSessionStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn from_map(map: Arc<Mutex<HashMap<u32, i32>>>) -> Self {
        Self { inner: map }
    }
}

impl Default for LocalSessionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionStore for LocalSessionStore {
    async fn insert_session(&self, account_id: u32, auth_code: i32) -> anyhow::Result<()> {
        self.inner.lock().unwrap().insert(account_id, auth_code);
        Ok(())
    }

    async fn get_session(&self, account_id: u32) -> anyhow::Result<Option<i32>> {
        Ok(self.inner.lock().unwrap().get(&account_id).copied())
    }

    async fn remove_session(&self, account_id: u32) -> anyhow::Result<()> {
        self.inner.lock().unwrap().remove(&account_id);
        Ok(())
    }

    async fn session_count(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connect_and_migrate() {
        // 使用内存数据库测试迁移。
        let db = Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        let count = db.account_count().await.unwrap();
        assert_eq!(count, 1); // 默认管理员账户
    }

    #[tokio::test]
    async fn test_find_account() {
        let db = Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        let acc = db.find_account_by_userid("admin").await.unwrap().unwrap();
        assert_eq!(acc.userid, "admin");
        assert_eq!(acc.user_pass, "admin");
        assert_eq!(acc.group_id, 99);
    }
}
