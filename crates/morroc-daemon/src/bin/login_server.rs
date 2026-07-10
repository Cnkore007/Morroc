//! 分布式登录服务器入口。
//!
//! 独立启动 Login 服务，会话通过共享的 SQLite 数据库与 Char/Map 同步。

use morroc_core::config::Config;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, warn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    morroc_core::panic::install_hook();
    morroc_core::logging::init();

    let cfg = Config::load()?;
    let db = morroc_db::Database::connect(&cfg.database_path).await?;
    db.migrate().await?;

    // 连接分布式消息总线。
    let broker_addr: SocketAddr = cfg.broker.listen.parse()?;
    match morroc_daemon::broker::start_service_client("login", broker_addr).await {
        Ok(_client) => info!("Login 服务已连接消息总线"),
        Err(e) => warn!("无法连接消息总线: {}", e),
    }

    let char_server = morroc_packets::login::ServerInfo {
        ip: Config::ip_to_u32(&cfg.char.ip).unwrap_or(0x0100007f),
        port: cfg.char.port as i16,
        name: "Morroc-Char".to_string(),
        usercount: 0,
        state: 0,
        property: 0,
    };

    // distributed 模式下使用数据库作为共享会话存储。
    let sessions: Arc<dyn morroc_db::SessionStore> = Arc::new(db.clone());
    let login_server = morroc_login::LoginServer::with_session_store(db, char_server, sessions);
    login_server.run(cfg.login.listen.parse()?).await?;

    Ok(())
}
