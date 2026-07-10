//! 分布式模式启动集成测试：验证各独立服务二进制能够启动并监听各自端口。
//!
//! 本测试通过 `CARGO_BIN_EXE_*` 定位 morroc-daemon 提供的四个二进制文件，
//! 在工作区根目录下启动它们，然后尝试连接 login/char/map 端口。

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::process::Child;
use tokio::time::{sleep, timeout};

struct ProcessGuard(Child);

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        let _ = self.0.start_kill();
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(std::env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("应能定位工作区根目录")
}

fn bin_path(name: &str) -> Option<PathBuf> {
    let env_key = format!("CARGO_BIN_EXE_{}", name);
    std::env::var_os(&env_key).map(PathBuf::from).or_else(|| {
        let p = workspace_root().join("target").join("debug").join(name);
        Some(p).filter(|p| p.exists())
    })
}

async fn wait_for_tcp(addr: &str, deadline: Duration) -> bool {
    timeout(deadline, async {
        loop {
            match TcpStream::connect(addr).await {
                Ok(_) => return true,
                Err(_) => sleep(Duration::from_millis(50)).await,
            }
        }
    })
    .await
    .unwrap_or(false)
}

#[tokio::test]
async fn distributed_servers_start_and_listen() {
    // 如果二进制未构建，则跳过（而不是失败），避免在只运行库测试时出错。
    let broker_path = match bin_path("message_broker") {
        Some(p) => p,
        None => {
            eprintln!("跳过分布式启动测试：未找到 message_broker 二进制");
            return;
        }
    };
    let login_path = bin_path("login_server").expect("应存在 login_server 二进制");
    let char_path = bin_path("char_server").expect("应存在 char_server 二进制");
    let map_path = bin_path("map_server").expect("应存在 map_server 二进制");

    let root = workspace_root();

    let mut broker = ProcessGuard(
        tokio::process::Command::new(&broker_path)
            .current_dir(&root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("启动 message_broker 失败"),
    );
    // 等待消息总线就绪。
    sleep(Duration::from_millis(300)).await;

    let mut login = ProcessGuard(
        tokio::process::Command::new(&login_path)
            .current_dir(&root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("启动 login_server 失败"),
    );
    let mut char_server = ProcessGuard(
        tokio::process::Command::new(&char_path)
            .current_dir(&root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("启动 char_server 失败"),
    );
    let mut map_server = ProcessGuard(
        tokio::process::Command::new(&map_path)
            .current_dir(&root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("启动 map_server 失败"),
    );

    // 给各服务足够时间完成数据库迁移、连接总线并监听端口。
    sleep(Duration::from_millis(800)).await;

    let login_ok = wait_for_tcp("127.0.0.1:6900", Duration::from_secs(3)).await;
    let char_ok = wait_for_tcp("127.0.0.1:6121", Duration::from_secs(3)).await;
    let map_ok = wait_for_tcp("127.0.0.1:5121", Duration::from_secs(3)).await;

    // 显式停止所有子进程，避免测试结束后残留。
    let _ = broker.0.start_kill();
    let _ = login.0.start_kill();
    let _ = char_server.0.start_kill();
    let _ = map_server.0.start_kill();

    assert!(login_ok, "login_server 应在 127.0.0.1:6900 监听");
    assert!(char_ok, "char_server 应在 127.0.0.1:6121 监听");
    assert!(map_ok, "map_server 应在 127.0.0.1:5121 监听");
}
