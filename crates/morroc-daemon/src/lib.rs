//! Morroc 服务端守护进程。
//!
//! 负责在 headless / standalone 模式下启动所有内部服务，
//! 并向 Tauri UI 暴露共享状态与命令。

pub mod agent;
pub mod broker;
mod dsl;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tracing::{error, info, warn};

use agent::{default_tools, AgentContext};
use dsl::ScriptRuntime;
use morroc_agent::{LlmClient, LlmConfig};
use morroc_db::LocalSessionStore;
use morroc_map::data::GameData;

const DATABASE_PATH: &str = "data/morroc.db";
const SCRIPTS_DIR: &str = "scripts";
const CHAR_LISTEN_ADDR: &str = "127.0.0.1:6121";
const MAP_LISTEN_ADDR: &str = "127.0.0.1:5121";
const AGENT_LISTEN_ADDR: &str = "127.0.0.1:3000";

/// 运行中服务的共享状态，供 Tauri UI 命令使用。
#[derive(Clone)]
pub struct AppState {
    db: morroc_db::Database,
    scripts: Arc<Mutex<ScriptRuntime>>,
    scripts_dir: std::path::PathBuf,
    sessions: Arc<dyn morroc_db::SessionStore>,
    agent: morroc_agent::Agent,
    map_server: morroc_map::MapServer,
    listen_addrs: Vec<String>,
}

impl AppState {
    /// 返回服务端状态摘要。
    pub async fn status_summary(&self) -> anyhow::Result<serde_json::Value> {
        let accounts = self.db.account_count().await?;
        let sessions = self.sessions.session_count().await;
        Ok(serde_json::json!({
            "status": "running",
            "accounts": accounts,
            "sessions": sessions,
            "listen_addresses": self.listen_addrs,
        }))
    }

    /// 账户数量。
    pub async fn account_count(&self) -> anyhow::Result<i64> {
        self.db.account_count().await
    }

    /// 当前会话数量。
    pub async fn session_count(&self) -> i64 {
        self.sessions.session_count().await as i64
    }

    /// 监听地址列表。
    pub fn listen_addresses(&self) -> Vec<String> {
        self.listen_addrs.clone()
    }

    /// 列出所有账户用户名。
    pub async fn list_accounts(&self) -> anyhow::Result<Vec<String>> {
        self.db.list_accounts().await
    }

    /// 创建新账户。
    pub async fn create_account(
        &self,
        userid: &str,
        password: &str,
        sex: &str,
    ) -> anyhow::Result<i64> {
        self.db.create_account(userid, password, sex).await
    }

    /// 列出所有 `.ro` 脚本文件名。
    pub fn list_scripts(&self) -> anyhow::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&self.scripts_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("ro") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        Ok(names)
    }

    /// 读取脚本内容。
    pub fn load_script(&self, name: &str) -> anyhow::Result<Option<String>> {
        let path = self.scripts_dir.join(format!("{}.ro", name));
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(std::fs::read_to_string(&path)?))
    }

    /// 保存脚本并触发热重载。
    pub fn save_script(&self, name: &str, content: &str) -> anyhow::Result<()> {
        let path = self.scripts_dir.join(format!("{}.ro", name));
        std::fs::write(&path, content)?;
        self.scripts.lock().unwrap().reload(&self.scripts_dir)?;
        Ok(())
    }

    /// 调用 DSL 函数。
    pub fn call_dsl(
        &self,
        name: &str,
        args: &[morroc_dsl::Value],
    ) -> Result<morroc_dsl::Value, morroc_dsl::RuntimeError> {
        self.scripts.lock().unwrap().call(name, args)
    }

    /// 与 Agent 对话一次。
    pub async fn agent_chat(&self, message: &str) -> anyhow::Result<morroc_agent::ChatResponse> {
        self.agent
            .chat(vec![morroc_agent::LlmMessage {
                role: "user".to_string(),
                content: message.to_string(),
            }])
            .await
    }

    /// 返回当前地图状态快照。
    pub fn map_state(&self) -> serde_json::Value {
        self.map_server.map.snapshot()
    }
}

/// 服务管理器，持有所有后台任务的 abort handle。
pub struct ServiceManager {
    pub state: AppState,
    login_abort: tokio::task::AbortHandle,
    char_abort: tokio::task::AbortHandle,
    map_abort: tokio::task::AbortHandle,
    agent_abort: tokio::task::AbortHandle,
}

impl ServiceManager {
    /// 关闭所有后台服务。
    pub fn shutdown(&self) {
        self.login_abort.abort();
        self.char_abort.abort();
        self.map_abort.abort();
        self.agent_abort.abort();
    }
}

/// 启动所有内部服务，返回共享状态与管理器。
///
/// 供 headless 模式与 Tauri UI 模式共用。
pub async fn start_services() -> anyhow::Result<ServiceManager> {
    info!("Morroc 服务端服务正在启动...");

    let db = morroc_db::Database::connect(DATABASE_PATH).await?;
    db.migrate().await?;

    match db.account_count().await {
        Ok(count) => info!("当前账户数量: {}", count),
        Err(e) => error!("查询账户数量失败: {}", e),
    }

    let char_server = morroc_login::ServerInfo {
        ip: 0x0100007f,
        port: 6121,
        name: "Morroc-Char".to_string(),
        usercount: 0,
        state: 0,
        property: 0,
    };
    let login_config = morroc_login::Config {
        listen_addr: "127.0.0.1:6900".parse()?,
        char_server,
    };

    let scripts = Arc::new(Mutex::new(
        ScriptRuntime::load(Path::new(SCRIPTS_DIR)).await?,
    ));
    if let Ok(result) = scripts.lock().unwrap().call("on_server_init", &[]) {
        info!("on_server_init 返回值: {}", result);
    }

    let sessions: Arc<dyn morroc_db::SessionStore> = Arc::new(LocalSessionStore::new());

    let agent_addr: SocketAddr = AGENT_LISTEN_ADDR.parse()?;
    let map_addr: SocketAddr = MAP_LISTEN_ADDR.parse()?;
    let game_data = GameData::load_from_json("data/database.json")?;
    let map_server = morroc_map::MapServer::new(map_addr, sessions.clone(), game_data, true, true);

    let listen_addrs = vec![
        login_config.listen_addr.to_string(),
        CHAR_LISTEN_ADDR.to_string(),
        MAP_LISTEN_ADDR.to_string(),
        agent_addr.to_string(),
    ];

    let agent_ctx = Arc::new(AgentContext::new(
        db.clone(),
        Arc::clone(&scripts),
        SCRIPTS_DIR,
        listen_addrs.clone(),
        sessions.clone(),
        map_server.clone(),
    ));
    let agent: morroc_agent::Agent = morroc_agent::Agent::new(
        default_tools(),
        agent_ctx,
        LlmClient::new(LlmConfig::from_env()),
    );

    let agent_for_http = Arc::new(agent.clone());
    let agent_task = tokio::spawn(async move { agent_for_http.run_http(agent_addr).await });
    let agent_abort = agent_task.abort_handle();
    info!("Agent 服务已启动于 {}", agent_addr);

    let login_server = morroc_login::LoginServer::with_session_store(
        db.clone(),
        login_config.char_server,
        sessions.clone(),
    );
    let login_task = tokio::spawn(async move { login_server.run(login_config.listen_addr).await });
    let login_abort = login_task.abort_handle();
    info!("登录服务器已启动于 {}", login_config.listen_addr);

    let char_addr: SocketAddr = CHAR_LISTEN_ADDR.parse()?;
    let char_server = morroc_char::CharServer::new(sessions.clone());
    let char_task = tokio::spawn(async move { char_server.run(char_addr).await });
    let char_abort = char_task.abort_handle();
    info!("角色服务器已启动于 {}", char_addr);

    let map_server_for_task = map_server.clone();
    let map_task = tokio::spawn(async move { map_server_for_task.run(None).await });
    let map_abort = map_task.abort_handle();
    info!("地图服务器已启动于 {}", map_addr);

    // 在后台任务上持续运行；当 manager 被 drop 或 shutdown 时它们会被取消。
    tokio::spawn(async move {
        let _ = login_task.await;
    });
    tokio::spawn(async move {
        let _ = char_task.await;
    });
    tokio::spawn(async move {
        let _ = map_task.await;
    });
    tokio::spawn(async move {
        let _ = agent_task.await;
    });

    let state = AppState {
        db,
        scripts,
        scripts_dir: std::path::PathBuf::from(SCRIPTS_DIR),
        sessions,
        agent,
        map_server: map_server.clone(),
        listen_addrs,
    };

    Ok(ServiceManager {
        state,
        login_abort,
        char_abort,
        map_abort,
        agent_abort,
    })
}

/// 运行 headless 服务端。
pub async fn run_headless() -> anyhow::Result<()> {
    info!("Morroc 服务端正在以 headless 模式启动...");

    let manager = start_services().await?;

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            warn!("收到中断信号，正在关闭...");
            manager.shutdown();
        }
    }

    info!("Morroc 服务端已安全退出。");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_headless_starts() {
        // 仅验证不 panic，不阻塞测试。
        let _ = tokio::time::timeout(Duration::from_millis(10), run_headless()).await;
    }
}
