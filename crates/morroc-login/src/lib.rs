//! Morroc 登录服务器。
//!
//! 负责接收原版 RO 客户端的登录请求，验证账户，并返回 char-server 列表。
//! 当前支持 CA_LOGIN（0x0064）以及 AC_ACCEPT_LOGIN / AC_REFUSE_LOGIN。

use futures::{SinkExt, StreamExt};
use morroc_db::{Database, SessionStore};
use morroc_net::{serve_with, FramedSession, Packet};
use morroc_packets::login::{
    AcAcceptLogin, AcRefuseLogin, CaLogin, HEADER_AC_ACCEPT_LOGIN, HEADER_AC_REFUSE_LOGIN,
    HEADER_CA_LOGIN,
};
use rand::Rng;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, warn};

pub use morroc_packets::login::ServerInfo;

/// 登录服务器配置。
#[derive(Debug, Clone)]
pub struct Config {
    pub listen_addr: SocketAddr,
    pub char_server: ServerInfo,
}

/// 登录服务器状态。
#[derive(Clone)]
pub struct LoginServer {
    db: Arc<Database>,
    char_server: ServerInfo,
    sessions: Option<Arc<dyn SessionStore>>,
}

impl LoginServer {
    /// 创建登录服务器。
    pub fn new(db: Database, char_server: ServerInfo) -> Self {
        Self {
            db: Arc::new(db),
            char_server,
            sessions: None,
        }
    }

    /// 创建登录服务器并关联共享会话管理器。
    pub fn with_session_store(
        db: Database,
        char_server: ServerInfo,
        sessions: Arc<dyn SessionStore>,
    ) -> Self {
        Self {
            db: Arc::new(db),
            char_server,
            sessions: Some(sessions),
        }
    }

    /// 启动监听。
    pub async fn run(&self, listen_addr: SocketAddr) -> anyhow::Result<()> {
        let db = self.db.clone();
        let char_server = self.char_server.clone();
        let sessions = self.sessions.clone();

        serve_with(listen_addr, move |framed, peer| {
            let db = db.clone();
            let char_server = char_server.clone();
            let sessions = sessions.clone();
            async move { handle_client(framed, peer, db, char_server, sessions).await }
        })
        .await
    }
}

async fn handle_client(
    mut framed: FramedSession,
    peer: SocketAddr,
    db: Arc<Database>,
    char_server: ServerInfo,
    sessions: Option<Arc<dyn SessionStore>>,
) -> anyhow::Result<()> {
    info!("登录客户端连接: {}", peer);

    while let Some(packet) = framed.next().await {
        let packet = packet?;
        match packet.packet_id {
            HEADER_CA_LOGIN => match CaLogin::decode(&packet.payload) {
                Ok(req) => {
                    info!("账户登录请求: {}", req.id);
                    match authenticate(&req, &db, &char_server).await {
                        Ok(accept) => {
                            if let Some(sessions) = &sessions {
                                sessions
                                    .insert_session(accept.aid, accept.auth_code)
                                    .await
                                    .ok();
                            }
                            framed
                                .send(Packet::new(HEADER_AC_ACCEPT_LOGIN, accept.encode_payload()))
                                .await?;
                        }
                        Err(code) => {
                            framed
                                .send(Packet::new(
                                    HEADER_AC_REFUSE_LOGIN,
                                    AcRefuseLogin {
                                        error_code: code,
                                        block_date: "0".to_string(),
                                    }
                                    .encode_payload(),
                                ))
                                .await?;
                        }
                    }
                }
                Err(e) => warn!("无法解析 CA_LOGIN 来自 {}: {}", peer, e),
            },
            _ => warn!("登录服务器收到未处理包 0x{:04x}", packet.packet_id),
        }
    }

    info!("登录客户端断开: {}", peer);
    Ok(())
}

async fn authenticate(
    req: &CaLogin,
    db: &Database,
    char_server: &ServerInfo,
) -> Result<AcAcceptLogin, u8> {
    let account = db
        .find_account_by_userid(&req.id)
        .await
        .map_err(|e| {
            tracing::error!("数据库查询失败: {}", e);
            0u8
        })?
        .ok_or(0u8)?;

    if account.user_pass != req.password {
        return Err(1u8);
    }

    let sex = match account.sex.as_str() {
        "M" => 0,
        "F" => 1,
        _ => 2,
    };

    let auth_code: i32 = rand::thread_rng().gen();

    Ok(AcAcceptLogin {
        auth_code,
        aid: account.account_id as u32,
        user_level: account.group_id as u32,
        last_login_ip: 0,
        last_login_time: "0".to_string(),
        sex,
        auth_token: [0u8; 16],
        twitter_flag: 0,
        server_list: vec![char_server.clone()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn fresh_db() -> Database {
        let db = Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        db
    }

    fn build_ca_login(userid: &str, password: &str) -> Vec<u8> {
        let mut buf = Vec::with_capacity(55);
        buf.extend_from_slice(&[0x64, 0x00]); // packet_id
        buf.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // version
        write_fixed_string(&mut buf, userid, 24);
        write_fixed_string(&mut buf, password, 24);
        buf.push(1); // clienttype
        buf
    }

    fn write_fixed_string(v: &mut Vec<u8>, s: &str, len: usize) {
        let bytes = s.as_bytes();
        let n = bytes.len().min(len);
        v.extend_from_slice(&bytes[..n]);
        v.extend_from_slice(&vec![0u8; len - n]);
    }

    async fn start_server() -> (SocketAddr, Database) {
        // 先绑定到一个随机端口获取地址，再让登录服务器监听同一端口。
        let temp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = temp.local_addr().unwrap();
        drop(temp);

        let db = fresh_db().await;
        let server = LoginServer::new(
            db.clone(),
            ServerInfo {
                ip: 0x0100007f,
                port: 6121,
                name: "Test-Char".to_string(),
                usercount: 0,
                state: 0,
                property: 0,
            },
        );
        tokio::spawn(async move { server.run(addr).await });

        // 给服务器一点时间完成绑定。
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        (addr, db)
    }

    async fn read_response(stream: &mut TcpStream) -> (u16, Vec<u8>) {
        let mut header = [0u8; 2];
        stream.read_exact(&mut header).await.unwrap();
        let packet_id = u16::from_le_bytes(header);

        let table_len = morroc_packets::packet_len(packet_id).expect("已知包长度");
        let payload = if table_len == -1 {
            // 动态长度包：payload 以 packet_len 开头。
            let mut len_bytes = [0u8; 2];
            stream.read_exact(&mut len_bytes).await.unwrap();
            let packet_len = u16::from_le_bytes(len_bytes) as usize;
            assert!(packet_len >= 4);

            let mut rest = vec![0u8; packet_len - 4];
            stream.read_exact(&mut rest).await.unwrap();

            let mut payload = Vec::with_capacity(packet_len - 2);
            payload.extend_from_slice(&len_bytes);
            payload.extend_from_slice(&rest);
            payload
        } else {
            // 静态长度包：payload 不包含长度字段。
            let mut payload = vec![0u8; table_len as usize - 2];
            stream.read_exact(&mut payload).await.unwrap();
            payload
        };

        (packet_id, payload)
    }

    #[tokio::test]
    async fn config_parse() {
        let addr: SocketAddr = "127.0.0.1:6900".parse().unwrap();
        assert_eq!(addr.port(), 6900);
    }

    #[tokio::test]
    async fn login_success_returns_accept() {
        let (addr, _db) = start_server().await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(&build_ca_login("admin", "admin"))
            .await
            .unwrap();

        let (packet_id, payload) = read_response(&mut stream).await;
        assert_eq!(packet_id, HEADER_AC_ACCEPT_LOGIN);
        assert!(!payload.is_empty());
        assert!(payload.len() > 2);
    }

    #[tokio::test]
    async fn login_wrong_password_returns_refuse() {
        let (addr, _db) = start_server().await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(&build_ca_login("admin", "wrong"))
            .await
            .unwrap();

        let (packet_id, payload) = read_response(&mut stream).await;
        assert_eq!(packet_id, HEADER_AC_REFUSE_LOGIN);
        assert!(!payload.is_empty());
        assert_eq!(payload[0], 1); // error_code
    }

    #[tokio::test]
    async fn login_unknown_user_returns_refuse() {
        let (addr, _db) = start_server().await;
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(&build_ca_login("nobody", "admin"))
            .await
            .unwrap();

        let (packet_id, payload) = read_response(&mut stream).await;
        assert_eq!(packet_id, HEADER_AC_REFUSE_LOGIN);
        assert_eq!(payload[0], 0); // error_code
    }
}
