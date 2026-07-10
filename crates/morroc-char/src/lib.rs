//! Morroc 角色服务器（Char Server）。
//!
//! 负责接收原版 RO 客户端的角色选择请求，返回角色列表与地图服务器信息。
//! 当前为里程碑骨架实现：不连接真实角色数据库，返回固定角色数据。

use futures::{SinkExt, StreamExt};
use morroc_db::SessionStore;
use morroc_net::{serve_with, FramedSession, Packet};
use morroc_packets::char::{
    ChEnter, ChSelectChar, CharInfo, HcAcceptEnter, HcNotifyZoneSvr, HEADER_CH_ENTER,
    HEADER_CH_SELECT_CHAR, HEADER_HC_ACCEPT_ENTER, HEADER_HC_NOTIFY_ZONESVR,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, warn};

const CHAR_NAME_SIZE: usize = 24;
const DEFAULT_CHAR_ID: u32 = 1;
const MAP_SERVER_IP: u32 = 0x0100007f; // 127.0.0.1 小端序整型
const MAP_SERVER_PORT: i16 = 5121;

/// 角色服务器状态。
#[derive(Clone)]
pub struct CharServer {
    sessions: Arc<dyn SessionStore>,
}

impl CharServer {
    /// 使用共享的会话管理器创建角色服务器。
    pub fn new(sessions: Arc<dyn SessionStore>) -> Self {
        Self { sessions }
    }

    /// 在指定地址启动监听。
    pub async fn run(&self, listen_addr: SocketAddr) -> anyhow::Result<()> {
        let sessions = self.sessions.clone();
        serve_with(listen_addr, move |framed, peer| {
            let sessions = sessions.clone();
            async move { handle_client(framed, peer, sessions).await }
        })
        .await
    }
}

async fn handle_client(
    mut framed: FramedSession,
    peer: SocketAddr,
    sessions: Arc<dyn SessionStore>,
) -> anyhow::Result<()> {
    info!("角色客户端连接: {}", peer);

    while let Some(packet) = framed.next().await {
        let packet = packet?;
        match packet.packet_id {
            HEADER_CH_ENTER => match ChEnter::decode(&packet.payload) {
                Ok(req) => {
                    info!("角色进入请求: account_id={}", req.account_id);
                    let valid = sessions
                        .get_session(req.account_id)
                        .await
                        .ok()
                        .flatten()
                        .map(|code| code == req.auth_code)
                        .unwrap_or(false);
                    let chars = if valid {
                        vec![default_char_info()]
                    } else {
                        warn!("会话验证失败: account_id={}", req.account_id);
                        vec![]
                    };
                    framed
                        .send(Packet::new(
                            HEADER_HC_ACCEPT_ENTER,
                            HcAcceptEnter { chars }.encode_payload(),
                        ))
                        .await?;
                }
                Err(e) => warn!("无法解析 CH_ENTER 来自 {}: {}", peer, e),
            },
            HEADER_CH_SELECT_CHAR => match ChSelectChar::decode(&packet.payload) {
                Ok(req) => {
                    info!("角色选择请求: char_num={}", req.char_num);
                    // 骨架阶段仅有一个固定角色，直接返回地图服务器信息。
                    framed
                        .send(Packet::new(
                            HEADER_HC_NOTIFY_ZONESVR,
                            HcNotifyZoneSvr {
                                char_id: DEFAULT_CHAR_ID,
                                map_name: map_name_bytes("prontera"),
                                ip: MAP_SERVER_IP,
                                port: MAP_SERVER_PORT,
                            }
                            .encode_payload(),
                        ))
                        .await?;
                }
                Err(e) => warn!("无法解析 CH_SELECT_CHAR 来自 {}: {}", peer, e),
            },
            _ => warn!("角色服务器收到未处理包 0x{:04x}", packet.packet_id),
        }
    }

    info!("角色客户端断开: {}", peer);
    Ok(())
}

fn default_char_info() -> CharInfo {
    CharInfo {
        char_id: DEFAULT_CHAR_ID,
        exp: 0,
        money: 0,
        jobexp: 0,
        joblevel: 1,
        hp: 100,
        max_hp: 100,
        sp: 50,
        max_sp: 50,
        speed: 150,
        job: 0,
        head: 1,
        weapon: 0,
        level: 1,
        name: name_bytes("Newbie"),
        str: 1,
        agi: 1,
        vit: 1,
        int_: 1,
        dex: 1,
        luk: 1,
        char_num: 0,
        hair_color: 1,
    }
}

fn name_bytes(s: &str) -> [u8; CHAR_NAME_SIZE] {
    let mut name = [0u8; CHAR_NAME_SIZE];
    let bytes = s.as_bytes();
    let n = bytes.len().min(CHAR_NAME_SIZE);
    name[..n].copy_from_slice(&bytes[..n]);
    name
}

fn map_name_bytes(s: &str) -> [u8; 16] {
    let mut map = [0u8; 16];
    let bytes = s.as_bytes();
    let n = bytes.len().min(16);
    map[..n].copy_from_slice(&bytes[..n]);
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn start_server() -> SocketAddr {
        let temp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = temp.local_addr().unwrap();
        drop(temp);

        let store = morroc_db::LocalSessionStore::new();
        store.insert_session(123, 456).await.unwrap();
        let server = CharServer::new(Arc::new(store));
        tokio::spawn(async move { server.run(addr).await });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        addr
    }

    fn build_ch_enter(account_id: u32, auth_code: i32) -> Vec<u8> {
        let mut buf = Vec::with_capacity(17);
        buf.extend_from_slice(&HEADER_CH_ENTER.to_le_bytes());
        buf.extend_from_slice(&account_id.to_le_bytes());
        buf.extend_from_slice(&auth_code.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes()); // login_id2
        buf.extend_from_slice(&0u16.to_le_bytes()); // client_tick
        buf.push(1); // sex
        buf
    }

    fn build_ch_select_char(char_num: u8) -> Vec<u8> {
        let mut buf = Vec::with_capacity(3);
        buf.extend_from_slice(&HEADER_CH_SELECT_CHAR.to_le_bytes());
        buf.push(char_num);
        buf
    }

    async fn read_response(stream: &mut TcpStream) -> (u16, Vec<u8>) {
        let mut header = [0u8; 2];
        stream.read_exact(&mut header).await.unwrap();
        let packet_id = u16::from_le_bytes(header);

        let table_len = morroc_packets::packet_len(packet_id).expect("已知包长度");
        let payload = if table_len == -1 {
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
            let mut payload = vec![0u8; table_len as usize - 2];
            stream.read_exact(&mut payload).await.unwrap();
            payload
        };

        (packet_id, payload)
    }

    #[tokio::test]
    async fn char_enter_and_select() {
        let addr = start_server().await;
        let mut stream = TcpStream::connect(addr).await.unwrap();

        stream.write_all(&build_ch_enter(123, 456)).await.unwrap();
        let (packet_id, payload) = read_response(&mut stream).await;
        assert_eq!(packet_id, HEADER_HC_ACCEPT_ENTER);
        assert!(!payload.is_empty());
        let packet_len = u16::from_le_bytes([payload[0], payload[1]]);
        assert!(packet_len >= 4);

        stream.write_all(&build_ch_select_char(0)).await.unwrap();
        let (packet_id, payload) = read_response(&mut stream).await;
        assert_eq!(packet_id, HEADER_HC_NOTIFY_ZONESVR);
        assert!(!payload.is_empty());
        assert_eq!(payload.len(), 26);
    }

    #[tokio::test]
    async fn invalid_auth_returns_empty_char_list() {
        let addr = start_server().await;
        let mut stream = TcpStream::connect(addr).await.unwrap();

        stream.write_all(&build_ch_enter(999, 0)).await.unwrap();
        let (packet_id, payload) = read_response(&mut stream).await;
        assert_eq!(packet_id, HEADER_HC_ACCEPT_ENTER);
        assert_eq!(u16::from_le_bytes([payload[0], payload[1]]), 4);
    }
}
