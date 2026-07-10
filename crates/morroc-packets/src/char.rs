//! 角色服务器（Char Server）相关包结构。
//!
//! 基于经典 RO 客户端协议，用于角色选择、进入地图等流程。

use bytes::{Buf, BufMut, BytesMut};

pub const HEADER_CH_ENTER: u16 = 0x0065;
pub const HEADER_CH_SELECT_CHAR: u16 = 0x0066;
pub const HEADER_HC_ACCEPT_ENTER: u16 = 0x006b;
pub const HEADER_HC_NOTIFY_ZONESVR: u16 = 0x0071;

const CHAR_BLOCK_SIZE: usize = 132;
const CHAR_NAME_SIZE: usize = 24;

/// 客户端进入角色服务器请求 CH_ENTER（0x0065）。
///
/// 注意：Hercules 20190530 长度表定义该包总长度为 17，因此 payload 为 15 字节，
/// client_tick 使用 u16 以匹配该长度。
#[derive(Debug, Clone)]
pub struct ChEnter {
    pub account_id: u32,
    pub auth_code: i32,
    pub login_id2: u32,
    pub client_tick: u16,
    pub sex: u8,
}

impl ChEnter {
    /// 从 payload（不含 packet_id）解码。
    pub fn decode(payload: &[u8]) -> anyhow::Result<Self> {
        if payload.len() != 15 {
            anyhow::bail!("CH_ENTER payload 长度应为 15，实际 {}", payload.len());
        }
        let mut buf = BytesMut::from(payload);
        let account_id = buf.get_u32_le();
        let auth_code = buf.get_i32_le();
        let login_id2 = buf.get_u32_le();
        let client_tick = buf.get_u16_le();
        let sex = buf.get_u8();
        Ok(Self {
            account_id,
            auth_code,
            login_id2,
            client_tick,
            sex,
        })
    }
}

/// 客户端选择角色 CH_SELECT_CHAR（0x0066）。
#[derive(Debug, Clone)]
pub struct ChSelectChar {
    pub char_num: u8,
}

impl ChSelectChar {
    /// 从 payload（不含 packet_id）解码。
    pub fn decode(payload: &[u8]) -> anyhow::Result<Self> {
        if payload.len() != 1 {
            anyhow::bail!("CH_SELECT_CHAR payload 长度应为 1，实际 {}", payload.len());
        }
        Ok(Self {
            char_num: payload[0],
        })
    }
}

/// 角色信息条目（用于 HC_ACCEPT_ENTER 列表）。
#[derive(Debug, Clone)]
pub struct CharInfo {
    pub char_id: u32,
    pub exp: i32,
    pub money: i32,
    pub jobexp: i32,
    pub joblevel: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub sp: i32,
    pub max_sp: i32,
    pub speed: i16,
    pub job: i16,
    pub head: i16,
    pub weapon: i16,
    pub level: i16,
    pub name: [u8; CHAR_NAME_SIZE],
    pub str: u8,
    pub agi: u8,
    pub vit: u8,
    pub int_: u8,
    pub dex: u8,
    pub luk: u8,
    pub char_num: u8,
    pub hair_color: u8,
}

impl CharInfo {
    /// 编码为固定 132 字节的角色数据块。
    fn encode(&self, buf: &mut BytesMut) {
        buf.put_u32_le(self.char_id);
        buf.put_i32_le(self.exp);
        buf.put_i32_le(self.money);
        buf.put_i32_le(self.jobexp);
        buf.put_i32_le(self.joblevel);
        buf.put_i32_le(self.hp);
        buf.put_i32_le(self.max_hp);
        buf.put_i32_le(self.sp);
        buf.put_i32_le(self.max_sp);
        buf.put_i16_le(self.speed);
        buf.put_i16_le(self.job);
        buf.put_i16_le(self.head);
        buf.put_i16_le(self.weapon);
        buf.put_i16_le(self.level);
        buf.extend_from_slice(&self.name);
        buf.put_u8(self.str);
        buf.put_u8(self.agi);
        buf.put_u8(self.vit);
        buf.put_u8(self.int_);
        buf.put_u8(self.dex);
        buf.put_u8(self.luk);
        buf.put_u8(self.char_num);
        buf.put_u8(self.hair_color);
        // 填充到 132 字节。
        let written = 4 * 9 + 2 * 5 + CHAR_NAME_SIZE + 6 + 2;
        let padding = CHAR_BLOCK_SIZE - written;
        buf.extend_from_slice(&vec![0u8; padding]);
    }
}

/// 角色列表响应 HC_ACCEPT_ENTER（0x006b，可变长度）。
#[derive(Debug, Clone)]
pub struct HcAcceptEnter {
    pub chars: Vec<CharInfo>,
}

impl HcAcceptEnter {
    /// 编码为完整包字节流（包含 packet_id 与 packet_len）。
    pub fn encode(&self) -> Vec<u8> {
        let total = 4 + self.chars.len() * CHAR_BLOCK_SIZE;
        let mut buf = BytesMut::with_capacity(total);
        buf.put_u16_le(HEADER_HC_ACCEPT_ENTER);
        buf.put_u16_le(total as u16);
        for c in &self.chars {
            c.encode(&mut buf);
        }
        buf.to_vec()
    }

    /// 编码为 PacketCodec 使用的 payload（不含 packet_id，但包含 packet_len）。
    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

/// 角色选择后返回的地图服务器信息 HC_NOTIFY_ZONESVR（0x0071）。
#[derive(Debug, Clone)]
pub struct HcNotifyZoneSvr {
    pub char_id: u32,
    pub map_name: [u8; 16],
    pub ip: u32,
    pub port: i16,
}

impl HcNotifyZoneSvr {
    /// 编码为完整包字节流。
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(28);
        buf.put_u16_le(HEADER_HC_NOTIFY_ZONESVR);
        buf.put_u32_le(self.char_id);
        buf.extend_from_slice(&self.map_name);
        buf.put_u32_le(self.ip);
        buf.put_i16_le(self.port);
        buf.to_vec()
    }

    /// 编码为 PacketCodec 使用的 payload（不含 packet_id）。
    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_from_str(s: &str) -> [u8; CHAR_NAME_SIZE] {
        let mut name = [0u8; CHAR_NAME_SIZE];
        let bytes = s.as_bytes();
        let n = bytes.len().min(CHAR_NAME_SIZE);
        name[..n].copy_from_slice(&bytes[..n]);
        name
    }

    #[test]
    fn ch_enter_roundtrip() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&2i32.to_le_bytes());
        payload.extend_from_slice(&3u32.to_le_bytes());
        payload.extend_from_slice(&4u16.to_le_bytes());
        payload.push(1);

        let req = ChEnter::decode(&payload).unwrap();
        assert_eq!(req.account_id, 1);
        assert_eq!(req.auth_code, 2);
        assert_eq!(req.login_id2, 3);
        assert_eq!(req.client_tick, 4);
        assert_eq!(req.sex, 1);
    }

    #[test]
    fn hc_accept_enter_size() {
        let info = CharInfo {
            char_id: 1,
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
            name: name_from_str("Newbie"),
            str: 1,
            agi: 1,
            vit: 1,
            int_: 1,
            dex: 1,
            luk: 1,
            char_num: 0,
            hair_color: 1,
        };
        let hc = HcAcceptEnter { chars: vec![info] };
        let bytes = hc.encode();
        assert_eq!(bytes.len(), 136);
        assert_eq!(
            u16::from_le_bytes([bytes[0], bytes[1]]),
            HEADER_HC_ACCEPT_ENTER
        );
        assert_eq!(u16::from_le_bytes([bytes[2], bytes[3]]), 136);
    }

    #[test]
    fn hc_notify_zonesvr_size() {
        let mut map_name = [0u8; 16];
        map_name[..8].copy_from_slice(b"prontera");
        let notify = HcNotifyZoneSvr {
            char_id: 1,
            map_name,
            ip: 0x0100007f,
            port: 5121,
        };
        assert_eq!(notify.encode().len(), 28);
    }
}
