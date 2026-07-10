//! 登录相关包结构（基于 RO 20190530 main 版本）。

use bytes::{Buf, BufMut, BytesMut};

pub const HEADER_CA_LOGIN: u16 = 0x0064;
pub const HEADER_AC_ACCEPT_LOGIN: u16 = 0x0069;
pub const HEADER_AC_REFUSE_LOGIN: u16 = 0x006a;

const MAX_CHARSERVER_NAME_SIZE: usize = 20;
const AUTH_TOKEN_SIZE: usize = 16;

/// 客户端登录请求 CA_LOGIN（包 ID 0x0064）。
#[derive(Debug, Clone)]
pub struct CaLogin {
    pub version: u32,
    pub id: String,
    pub password: String,
    pub clienttype: u8,
}

impl CaLogin {
    /// 从 payload 解码（不含 packet_id）。
    pub fn decode(payload: &[u8]) -> anyhow::Result<Self> {
        if payload.len() != 53 {
            anyhow::bail!("CA_LOGIN payload 长度应为 53，实际 {}", payload.len());
        }
        let mut buf = BytesMut::from(payload);
        let version = buf.get_u32_le();
        let id = read_fixed_string(&buf.split_to(24));
        let password = read_fixed_string(&buf.split_to(24));
        let clienttype = buf.get_u8();
        Ok(Self {
            version,
            id,
            password,
            clienttype,
        })
    }
}

/// 登录失败 AC_REFUSE_LOGIN（包 ID 0x006a）。
#[derive(Debug, Clone)]
pub struct AcRefuseLogin {
    pub error_code: u8,
    pub block_date: String,
}

impl AcRefuseLogin {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(23);
        buf.put_u16_le(HEADER_AC_REFUSE_LOGIN);
        buf.put_u8(self.error_code);
        write_fixed_string(&mut buf, &self.block_date, 20);
        buf.to_vec()
    }

    /// 编码为 PacketCodec 使用的 payload（不含 packet_id）。
    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

/// 一个 char-server 列表项。
#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub ip: u32,
    pub port: i16,
    pub name: String,
    pub usercount: u16,
    pub state: u16,
    pub property: u16,
}

/// 登录成功 AC_ACCEPT_LOGIN（包 ID 0x0069，可变长度）。
#[derive(Debug, Clone)]
pub struct AcAcceptLogin {
    pub auth_code: i32,
    pub aid: u32,
    pub user_level: u32,
    pub last_login_ip: u32,
    pub last_login_time: String,
    pub sex: u8,
    pub auth_token: [u8; AUTH_TOKEN_SIZE],
    pub twitter_flag: u8,
    pub server_list: Vec<ServerInfo>,
}

impl AcAcceptLogin {
    /// 编码为完整包字节流。
    pub fn encode(&self) -> Vec<u8> {
        // 基础大小：packet_id(2) + packet_len(2) + auth_code(4) + aid(4) +
        // user_level(4) + last_login_ip(4) + last_login_time(26) + sex(1) +
        // auth_token(16) + twitter_flag(1) = 64
        let server_size = 4 + 2 + MAX_CHARSERVER_NAME_SIZE + 2 + 2 + 2 + 128; // 160
        let total = 64 + self.server_list.len() * server_size;

        let mut buf = BytesMut::with_capacity(total);
        buf.put_u16_le(HEADER_AC_ACCEPT_LOGIN);
        buf.put_u16_le(total as u16);
        buf.put_i32_le(self.auth_code);
        buf.put_u32_le(self.aid);
        buf.put_u32_le(self.user_level);
        buf.put_u32_le(self.last_login_ip);
        write_fixed_string(&mut buf, &self.last_login_time, 26);
        buf.put_u8(self.sex);
        buf.extend_from_slice(&self.auth_token);
        buf.put_u8(self.twitter_flag);

        for s in &self.server_list {
            buf.put_u32_le(s.ip);
            buf.put_i16_le(s.port);
            write_fixed_string(&mut buf, &s.name, MAX_CHARSERVER_NAME_SIZE);
            buf.put_u16_le(s.usercount);
            buf.put_u16_le(s.state);
            buf.put_u16_le(s.property);
            buf.extend_from_slice(&[0u8; 128]); // unknown2
        }

        buf.to_vec()
    }

    /// 编码为 PacketCodec 使用的 payload（不含 packet_id，但包含 packet_len）。
    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

fn read_fixed_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn write_fixed_string(buf: &mut BytesMut, s: &str, len: usize) {
    let bytes = s.as_bytes();
    let n = bytes.len().min(len);
    buf.extend_from_slice(&bytes[..n]);
    if n < len {
        buf.extend_from_slice(&vec![0u8; len - n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ca_login_roundtrip() {
        let mut payload = Vec::with_capacity(53);
        payload.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // version
        write_fixed_string_to_vec(&mut payload, "admin", 24);
        write_fixed_string_to_vec(&mut payload, "admin", 24);
        payload.push(1); // clienttype

        let login = CaLogin::decode(&payload).unwrap();
        assert_eq!(login.id, "admin");
        assert_eq!(login.password, "admin");
        assert_eq!(login.clienttype, 1);
    }

    #[test]
    fn ac_accept_login_size() {
        let accept = AcAcceptLogin {
            auth_code: 123,
            aid: 1,
            user_level: 99,
            last_login_ip: 0,
            last_login_time: "0".to_string(),
            sex: 1,
            auth_token: [0; AUTH_TOKEN_SIZE],
            twitter_flag: 0,
            server_list: vec![ServerInfo {
                ip: 0x0100007f, // 127.0.0.1
                port: 6121,
                name: "Morroc-Char".to_string(),
                usercount: 0,
                state: 0,
                property: 0,
            }],
        };
        let bytes = accept.encode();
        // 64 + 160 = 224
        assert_eq!(bytes.len(), 224);
    }

    fn write_fixed_string_to_vec(v: &mut Vec<u8>, s: &str, len: usize) {
        let bytes = s.as_bytes();
        let n = bytes.len().min(len);
        v.extend_from_slice(&bytes[..n]);
        v.extend_from_slice(&vec![0u8; len - n]);
    }
}
