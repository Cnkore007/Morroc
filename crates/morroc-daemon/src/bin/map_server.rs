//! 分布式地图服务器入口。

use morroc_core::config::Config;
use morroc_map::data::GameData;
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
    match morroc_daemon::broker::start_service_client("map", broker_addr).await {
        Ok(_client) => info!("Map 服务已连接消息总线"),
        Err(e) => warn!("无法连接消息总线: {}", e),
    }

    let sessions: Arc<dyn morroc_db::SessionStore> = Arc::new(db.clone());
    let game_data = GameData::load_from_json("data/database.json")?;
    let map_server = morroc_map::MapServer::new(cfg.map.listen.parse()?, sessions, game_data, true, true);
    map_server.run(None).await?;

    Ok(())
}
