//! 分布式角色服务器入口。

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
    match morroc_daemon::broker::start_service_client("char", broker_addr).await {
        Ok(_client) => info!("Char 服务已连接消息总线"),
        Err(e) => warn!("无法连接消息总线: {}", e),
    }

    let sessions: Arc<dyn morroc_db::SessionStore> = Arc::new(db.clone());
    let char_server = morroc_char::CharServer::new(sessions);
    char_server.run(cfg.char.listen.parse()?).await?;

    Ok(())
}
