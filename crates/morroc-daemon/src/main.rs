//! Morroc 单可执行文件入口。
//!
//! 统一启动 login/char/map/agent 服务，并接入彩色日志与中文崩溃报告。

use morroc_daemon::run_headless;

fn main() -> anyhow::Result<()> {
    // 中文崩溃报告：捕获 panic 并写入 crashes/ 目录。
    morroc_core::panic::install_hook();

    // 彩色聚合日志：初始化 tracing 订阅器，按模块标签输出。
    morroc_core::logging::init();

    // 启动所有服务，阻塞直到收到 Ctrl-C 信号。
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run_headless())
}
