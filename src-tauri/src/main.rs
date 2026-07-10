// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "morroc")]
#[command(about = "Morroc - 高性能 RO 服务端")]
struct Cli {
    /// 以无窗口 headless 模式运行服务端。
    #[arg(long)]
    headless: bool,
}

fn main() {
    morroc_core::panic::install_hook();

    let cli = Cli::parse();

    if cli.headless {
        morroc_core::logging::init();
        let runtime = tokio::runtime::Runtime::new().expect("创建 Tokio runtime 失败");
        let _ = runtime.block_on(morroc_daemon::run_headless());
    } else {
        morroc_lib::run();
    }
}
