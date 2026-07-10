//! 分布式消息总线独立启动入口。
//!
//! 在 distributed 模式下先于 login/char/map 启动，负责进程间消息转发。

use morroc_core::config::Config;
use morroc_daemon::broker::MessageBroker;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    morroc_core::panic::install_hook();
    morroc_core::logging::init();

    let cfg = Config::load()?;
    let addr: SocketAddr = cfg.broker.listen.parse()?;
    let broker = MessageBroker::new(addr).await?;
    broker.run().await?;

    Ok(())
}
