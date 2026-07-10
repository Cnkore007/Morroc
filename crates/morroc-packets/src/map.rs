//! 地图服务器（Map Server）相关包结构。
//!
//! 同时保留经典客户端（0x0072/0x0073）与 2019 main 混淆协议（0x0436/0x02eb）的
//! 进入包，并提供移动、战斗、实体刷新等核心地图协议。

use bytes::{Buf, BufMut, BytesMut};

// 经典登录流程（保留兼容）
pub const HEADER_CZ_ENTER: u16 = 0x0072;
pub const HEADER_ZC_ACCEPT_ENTER: u16 = 0x0073;

// 2019 main 混淆后的地图进入流程
pub const HEADER_CZ_ENTER_2019: u16 = 0x0436;
pub const HEADER_ZC_ACCEPT_ENTER_2019: u16 = 0x02eb;

// 移动与战斗（2019 main）
pub const HEADER_CZ_REQUEST_MOVE: u16 = 0x035f;
pub const HEADER_CZ_REQUEST_ACT: u16 = 0x0437;
pub const HEADER_CZ_USE_SKILL: u16 = 0x0438;
pub const HEADER_ZC_NOTIFY_PLAYERMOVE: u16 = 0x0087;
pub const HEADER_ZC_NOTIFY_MOVE: u16 = 0x0086;
pub const HEADER_ZC_NOTIFY_ACT: u16 = 0x08c8;
pub const HEADER_ZC_NOTIFY_VANISH: u16 = 0x0080;

// 实体刷新（2019 main 动态长度包）
pub const HEADER_ZC_NOTIFY_STANDENTRY: u16 = 0x09ff;
pub const HEADER_ZC_NOTIFY_SPAWNENTRY: u16 = 0x09fe;
pub const HEADER_ZC_NOTIFY_MOVEENTRY: u16 = 0x09fd;

// 地图跳转确认
pub const HEADER_ZC_NPCACK_MAP: u16 = 0x0091;

const NAME_LENGTH: usize = 24;

/// 经典客户端进入地图服务器请求 CZ_ENTER（0x0072）。
///
/// RO 2019 主长度表定义该包总长度为 22，payload 为 20 字节。
/// 字段顺序：account_id, char_id, auth_code, client_tick, sex；sex 后存在 3 字节填充。
#[derive(Debug, Clone)]
pub struct CzEnter {
    pub account_id: u32,
    pub char_id: u32,
    pub auth_code: i32,
    pub client_tick: u32,
    pub sex: u8,
}

impl CzEnter {
    /// 从 payload（不含 packet_id）解码。
    pub fn decode(payload: &[u8]) -> anyhow::Result<Self> {
        if payload.len() < 17 {
            anyhow::bail!("CZ_ENTER payload 长度至少为 17，实际 {}", payload.len());
        }
        let mut buf = BytesMut::from(payload);
        let account_id = buf.get_u32_le();
        let char_id = buf.get_u32_le();
        let auth_code = buf.get_i32_le();
        let client_tick = buf.get_u32_le();
        let sex = buf.get_u8();
        Ok(Self {
            account_id,
            char_id,
            auth_code,
            client_tick,
            sex,
        })
    }
}

/// 经典地图服务器进入确认 ZC_ACCEPT_ENTER（0x0073）。
///
/// 总长度 11，payload 9 字节：服务器时间戳、出生 X/Y 坐标、朝向。
#[derive(Debug, Clone)]
pub struct ZcAcceptEnter {
    pub server_tick: u32,
    pub pos_x: i16,
    pub pos_y: i16,
    pub dir: u8,
}

impl ZcAcceptEnter {
    /// 编码为完整包字节流。
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(11);
        buf.put_u16_le(HEADER_ZC_ACCEPT_ENTER);
        buf.put_u32_le(self.server_tick);
        buf.put_i16_le(self.pos_x);
        buf.put_i16_le(self.pos_y);
        buf.put_u8(self.dir);
        buf.to_vec()
    }

    /// 编码为 PacketCodec 使用的 payload（不含 packet_id）。
    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

/// 2019 main 客户端进入地图服务器请求 CZ_ENTER（0x0436）。
///
/// 总长度 19，payload 17 字节。
#[derive(Debug, Clone)]
pub struct CzEnter2019 {
    pub account_id: u32,
    pub char_id: u32,
    pub auth_code: i32,
    pub client_tick: u32,
    pub sex: u8,
}

impl CzEnter2019 {
    pub fn decode(payload: &[u8]) -> anyhow::Result<Self> {
        if payload.len() < 17 {
            anyhow::bail!(
                "CZ_ENTER_2019 payload 长度至少为 17，实际 {}",
                payload.len()
            );
        }
        let mut buf = BytesMut::from(payload);
        let account_id = buf.get_u32_le();
        let char_id = buf.get_u32_le();
        let auth_code = buf.get_i32_le();
        let client_tick = buf.get_u32_le();
        let sex = buf.get_u8();
        Ok(Self {
            account_id,
            char_id,
            auth_code,
            client_tick,
            sex,
        })
    }
}

/// 2019 main 地图服务器进入确认 ZC_ACCEPT_ENTER（0x02eb）。
///
/// 总长度 13，payload 11 字节。
#[derive(Debug, Clone)]
pub struct ZcAcceptEnter2019 {
    pub server_tick: u32,
    pub pos_x: i16,
    pub pos_y: i16,
    pub dir: u8,
    pub x_size: u8,
    pub y_size: u8,
    pub font: i16,
}

impl ZcAcceptEnter2019 {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(13);
        buf.put_u16_le(HEADER_ZC_ACCEPT_ENTER_2019);
        buf.put_u32_le(self.server_tick);
        buf.extend_from_slice(&encode_pos_dir(self.pos_x, self.pos_y, self.dir));
        buf.put_u8(self.x_size);
        buf.put_u8(self.y_size);
        buf.put_i16_le(self.font);
        buf.to_vec()
    }

    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

/// 2019 main 客户端移动请求 CZ_REQUEST_MOVE（0x035f）。
///
/// 总长度 5，payload 3 字节，为压缩的 PosDir。
#[derive(Debug, Clone)]
pub struct CzRequestMove {
    pub x: i16,
    pub y: i16,
    pub dir: u8,
}

impl CzRequestMove {
    pub fn decode(payload: &[u8]) -> anyhow::Result<Self> {
        let (x, y, dir) = decode_pos_dir(payload)?;
        Ok(Self { x, y, dir })
    }
}

/// 2019 main 客户端动作/攻击请求 CZ_REQUEST_ACT（0x0437）。
///
/// 总长度 7，payload 5 字节。
#[derive(Debug, Clone)]
pub struct CzRequestAct {
    pub target_id: u32,
    pub action: u8,
}

impl CzRequestAct {
    pub fn decode(payload: &[u8]) -> anyhow::Result<Self> {
        if payload.len() < 5 {
            anyhow::bail!(
                "CZ_REQUEST_ACT payload 长度至少为 5，实际 {}",
                payload.len()
            );
        }
        let mut buf = BytesMut::from(payload);
        let target_id = buf.get_u32_le();
        let action = buf.get_u8();
        Ok(Self { target_id, action })
    }
}

/// 技能使用请求 CZ_USE_SKILL2（0x0438）。
///
/// 总长度 10，payload 8 字节：skill_id(u16), skill_lv(u16), target_id(u32)。
#[derive(Debug, Clone)]
pub struct CzUseSkill {
    pub skill_id: u16,
    pub skill_lv: u16,
    pub target_id: u32,
}

impl CzUseSkill {
    pub fn decode(payload: &[u8]) -> anyhow::Result<Self> {
        if payload.len() < 8 {
            anyhow::bail!("CZ_USE_SKILL payload 长度至少为 8，实际 {}", payload.len());
        }
        let mut buf = BytesMut::from(payload);
        let skill_id = buf.get_u16_le();
        let skill_lv = buf.get_u16_le();
        let target_id = buf.get_u32_le();
        Ok(Self {
            skill_id,
            skill_lv,
            target_id,
        })
    }
}

/// 3 字节 PosDir 编码（x/y/dir）。
pub fn encode_pos_dir(x: i16, y: i16, dir: u8) -> [u8; 3] {
    let x = x as u16;
    let y = y as u16;
    let d = (dir & 0x0f) as u16;
    let mut p = [0u8; 3];
    p[0] = ((x >> 2) & 0xff) as u8;
    p[1] = (((x << 6) & 0xc0) | ((y >> 4) & 0x3f)) as u8;
    p[2] = (((y << 4) & 0xf0) | d) as u8;
    p
}

/// 3 字节 PosDir 解码。
pub fn decode_pos_dir(p: &[u8]) -> anyhow::Result<(i16, i16, u8)> {
    if p.len() < 3 {
        anyhow::bail!("PosDir 需要 3 字节，实际 {}", p.len());
    }
    let x = (((p[0] as u16) << 2) | ((p[1] as u16) >> 6)) as i16;
    let y = ((((p[1] & 0x3f) as u16) << 4) | ((p[2] as u16) >> 4)) as i16;
    let dir = p[2] & 0x0f;
    Ok((x, y, dir))
}

/// 6 字节 MoveData 编码（起点/终点坐标 + 子格）。
pub fn encode_move_data(x0: i16, y0: i16, x1: i16, y1: i16, sx0: u8, sy0: u8) -> [u8; 6] {
    let mut p = [0u8; 6];
    p[0] = ((x0 >> 2) & 0xff) as u8;
    p[1] = (((x0 << 6) & 0xc0) | ((y0 >> 4) & 0x3f)) as u8;
    p[2] = (((y0 << 4) & 0xf0) | ((x1 >> 6) & 0x0f)) as u8;
    p[3] = (((x1 << 2) & 0xfc) | ((y1 >> 8) & 0x03)) as u8;
    p[4] = (y1 & 0xff) as u8;
    p[5] = ((sx0 & 0x0f) << 4) | (sy0 & 0x0f);
    p
}

/// 6 字节 MoveData 解码。
pub fn decode_move_data(p: &[u8]) -> anyhow::Result<(i16, i16, i16, i16, u8, u8)> {
    if p.len() < 6 {
        anyhow::bail!("MoveData 需要 6 字节，实际 {}", p.len());
    }
    let x0 = (((p[0] as u16) << 2) | ((p[1] as u16) >> 6)) as i16;
    let y0 = ((((p[1] & 0x3f) as u16) << 4) | ((p[2] as u16) >> 4)) as i16;
    let x1 = ((((p[2] & 0x0f) as u16) << 6) | ((p[3] as u16) >> 2)) as i16;
    let y1 = ((((p[3] & 0x03) as u16) << 8) | (p[4] as u16)) as i16;
    let sx0 = (p[5] & 0xf0) >> 4;
    let sy0 = p[5] & 0x0f;
    Ok((x0, y0, x1, y1, sx0, sy0))
}

/// 玩家移动确认 ZC_NOTIFY_PLAYERMOVE（0x0087）。
#[derive(Debug, Clone)]
pub struct ZcNotifyPlayerMove {
    pub start_time: u32,
    pub from_x: i16,
    pub from_y: i16,
    pub to_x: i16,
    pub to_y: i16,
}

impl ZcNotifyPlayerMove {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(12);
        buf.put_u16_le(HEADER_ZC_NOTIFY_PLAYERMOVE);
        buf.put_u32_le(self.start_time);
        buf.extend_from_slice(&encode_move_data(
            self.from_x,
            self.from_y,
            self.to_x,
            self.to_y,
            8,
            8,
        ));
        buf.to_vec()
    }

    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

/// 其他单位移动通知 ZC_NOTIFY_MOVE（0x0086）。
#[derive(Debug, Clone)]
pub struct ZcNotifyMove {
    pub id: u32,
    pub start_time: u32,
    pub from_x: i16,
    pub from_y: i16,
    pub to_x: i16,
    pub to_y: i16,
}

impl ZcNotifyMove {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(16);
        buf.put_u16_le(HEADER_ZC_NOTIFY_MOVE);
        buf.put_u32_le(self.id);
        buf.extend_from_slice(&encode_move_data(
            self.from_x,
            self.from_y,
            self.to_x,
            self.to_y,
            8,
            8,
        ));
        buf.put_u32_le(self.start_time);
        buf.to_vec()
    }

    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

/// 单位消失通知 ZC_NOTIFY_VANISH（0x0080）。
#[derive(Debug, Clone)]
pub struct ZcNotifyVanish {
    pub id: u32,
    pub vanish_type: u8,
}

impl ZcNotifyVanish {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(7);
        buf.put_u16_le(HEADER_ZC_NOTIFY_VANISH);
        buf.put_u32_le(self.id);
        buf.put_u8(self.vanish_type);
        buf.to_vec()
    }

    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

/// 战斗动作通知 ZC_NOTIFY_ACT（0x08c8）。
#[derive(Debug, Clone)]
pub struct ZcNotifyAct {
    pub source_id: u32,
    pub target_id: u32,
    pub start_time: u32,
    pub attack_mt: i32,
    pub attacked_mt: i32,
    pub damage: i32,
    pub count: i16,
    pub action: u8,
    pub left_damage: i32,
}

impl ZcNotifyAct {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(34);
        buf.put_u16_le(HEADER_ZC_NOTIFY_ACT);
        buf.put_u32_le(self.source_id);
        buf.put_u32_le(self.target_id);
        buf.put_u32_le(self.start_time);
        buf.put_i32_le(self.attack_mt);
        buf.put_i32_le(self.attacked_mt);
        buf.put_i32_le(self.damage);
        buf.put_u8(0); // is_sp_damaged
        buf.put_i16_le(self.count);
        buf.put_u8(self.action);
        buf.put_i32_le(self.left_damage);
        buf.to_vec()
    }

    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

/// 地图跳转确认 ZC_NPCACK_MAP（0x0091）。
#[derive(Debug, Clone)]
pub struct ZcNpcAckMap {
    pub map_name: [u8; 16],
    pub x: i16,
    pub y: i16,
}

impl ZcNpcAckMap {
    pub fn encode(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(22);
        buf.put_u16_le(HEADER_ZC_NPCACK_MAP);
        buf.extend_from_slice(&self.map_name);
        buf.put_i16_le(self.x);
        buf.put_i16_le(self.y);
        buf.to_vec()
    }

    pub fn encode_payload(&self) -> Vec<u8> {
        self.encode()[2..].to_vec()
    }
}

/// 实体外观，用于生成 0x09ff/0x09fe/0x09fd 刷新包。
#[derive(Debug, Clone)]
pub struct EntityAppearance {
    pub object_type: u8,
    pub aid: u32,
    pub gid: u32,
    pub speed: i16,
    pub job: i16,
    pub head: u16,
    pub x: i16,
    pub y: i16,
    pub dir: u8,
    pub hp: i32,
    pub max_hp: i32,
    pub level: i16,
    pub sex: u8,
    pub name: [u8; NAME_LENGTH],
    pub body: i16,
}

impl EntityAppearance {
    /// 站立实体刷新包 ZC_NOTIFY_STANDENTRY（0x09ff）。
    pub fn encode_stand_entry(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(108);
        buf.put_u16_le(HEADER_ZC_NOTIFY_STANDENTRY);
        buf.put_i16_le(108); // PacketLength
        buf.put_u8(self.object_type);
        buf.put_u32_le(self.aid);
        buf.put_u32_le(self.gid);
        buf.put_i16_le(self.speed);
        buf.put_i16_le(0); // bodyState
        buf.put_i16_le(0); // healthState
        buf.put_i32_le(0); // effectState
        buf.put_i16_le(self.job);
        buf.put_u16_le(self.head);
        buf.put_u32_le(0); // weapon
        buf.put_u32_le(0); // shield
        buf.put_u16_le(0); // accessory
        buf.put_u16_le(0); // accessory2
        buf.put_u16_le(0); // accessory3
        buf.put_i16_le(0); // headpalette
        buf.put_i16_le(0); // bodypalette
        buf.put_i16_le(0); // headDir
        buf.put_u16_le(0); // robe
        buf.put_u32_le(0); // GUID
        buf.put_i16_le(0); // GEmblemVer
        buf.put_i16_le(0); // honor
        buf.put_i32_le(0); // virtue
        buf.put_u8(0); // isPKModeON
        buf.put_u8(self.sex);
        buf.extend_from_slice(&encode_pos_dir(self.x, self.y, self.dir));
        buf.put_u8(0); // xSize
        buf.put_u8(0); // ySize
        buf.put_u8(0); // state
        buf.put_i16_le(self.level); // clevel
        buf.put_i16_le(0); // font
        buf.put_i32_le(self.max_hp);
        buf.put_i32_le(self.hp);
        buf.put_u8(0); // isBoss
        buf.put_i16_le(self.body);
        buf.extend_from_slice(&self.name);
        buf.to_vec()
    }

    /// 出生实体刷新包 ZC_NOTIFY_SPAWNENTRY（0x09fe）。
    pub fn encode_spawn_entry(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(107);
        buf.put_u16_le(HEADER_ZC_NOTIFY_SPAWNENTRY);
        buf.put_i16_le(107); // PacketLength
        buf.put_u8(self.object_type);
        buf.put_u32_le(self.aid);
        buf.put_u32_le(self.gid);
        buf.put_i16_le(self.speed);
        buf.put_i16_le(0); // bodyState
        buf.put_i16_le(0); // healthState
        buf.put_i32_le(0); // effectState
        buf.put_i16_le(self.job);
        buf.put_u16_le(self.head);
        buf.put_u32_le(0); // weapon
        buf.put_u32_le(0); // shield
        buf.put_u16_le(0); // accessory
        buf.put_u16_le(0); // accessory2
        buf.put_u16_le(0); // accessory3
        buf.put_i16_le(0); // headpalette
        buf.put_i16_le(0); // bodypalette
        buf.put_i16_le(0); // headDir
        buf.put_u16_le(0); // robe
        buf.put_u32_le(0); // GUID
        buf.put_i16_le(0); // GEmblemVer
        buf.put_i16_le(0); // honor
        buf.put_i32_le(0); // virtue
        buf.put_u8(0); // isPKModeON
        buf.put_u8(self.sex);
        buf.extend_from_slice(&encode_pos_dir(self.x, self.y, self.dir));
        buf.put_u8(0); // xSize
        buf.put_u8(0); // ySize
        buf.put_i16_le(self.level); // clevel
        buf.put_i16_le(0); // font
        buf.put_i32_le(self.max_hp);
        buf.put_i32_le(self.hp);
        buf.put_u8(0); // isBoss
        buf.put_i16_le(self.body);
        buf.extend_from_slice(&self.name);
        buf.to_vec()
    }

    /// 移动中实体刷新包 ZC_NOTIFY_MOVEENTRY（0x09fd）。
    #[allow(clippy::too_many_arguments)]
    pub fn encode_move_entry(
        &self,
        move_start_time: u32,
        from_x: i16,
        from_y: i16,
        to_x: i16,
        to_y: i16,
    ) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(114);
        buf.put_u16_le(HEADER_ZC_NOTIFY_MOVEENTRY);
        buf.put_i16_le(114); // PacketLength
        buf.put_u8(self.object_type);
        buf.put_u32_le(self.aid);
        buf.put_u32_le(self.gid);
        buf.put_i16_le(self.speed);
        buf.put_i16_le(0); // bodyState
        buf.put_i16_le(0); // healthState
        buf.put_i32_le(0); // effectState
        buf.put_i16_le(self.job);
        buf.put_u16_le(self.head);
        buf.put_u32_le(0); // weapon
        buf.put_u32_le(0); // shield
        buf.put_u16_le(0); // accessory
        buf.put_u32_le(move_start_time);
        buf.put_u16_le(0); // accessory2
        buf.put_u16_le(0); // accessory3
        buf.put_i16_le(0); // headpalette
        buf.put_i16_le(0); // bodypalette
        buf.put_i16_le(0); // headDir
        buf.put_u16_le(0); // robe
        buf.put_u32_le(0); // GUID
        buf.put_i16_le(0); // GEmblemVer
        buf.put_i16_le(0); // honor
        buf.put_i32_le(0); // virtue
        buf.put_u8(0); // isPKModeON
        buf.put_u8(self.sex);
        buf.extend_from_slice(&encode_move_data(from_x, from_y, to_x, to_y, 8, 8));
        buf.put_u8(0); // xSize
        buf.put_u8(0); // ySize
        buf.put_i16_le(self.level); // clevel
        buf.put_i16_le(0); // font
        buf.put_i32_le(self.max_hp);
        buf.put_i32_le(self.hp);
        buf.put_u8(0); // isBoss
        buf.put_i16_le(self.body);
        buf.extend_from_slice(&self.name);
        buf.to_vec()
    }
}

/// 将字符串转换为固定 24 字节的角色名。
pub fn name_bytes(s: &str) -> [u8; NAME_LENGTH] {
    let mut name = [0u8; NAME_LENGTH];
    let bytes = s.as_bytes();
    let n = bytes.len().min(NAME_LENGTH);
    name[..n].copy_from_slice(&bytes[..n]);
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cz_enter_roundtrip() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(&3i32.to_le_bytes());
        payload.extend_from_slice(&4u32.to_le_bytes());
        payload.push(1);
        payload.extend_from_slice(&[0u8; 3]);

        let req = CzEnter::decode(&payload).unwrap();
        assert_eq!(req.account_id, 1);
        assert_eq!(req.char_id, 2);
        assert_eq!(req.auth_code, 3);
        assert_eq!(req.client_tick, 4);
        assert_eq!(req.sex, 1);
    }

    #[test]
    fn zc_accept_enter_size() {
        let resp = ZcAcceptEnter {
            server_tick: 12345,
            pos_x: 150,
            pos_y: 180,
            dir: 0,
        };
        let bytes = resp.encode();
        assert_eq!(bytes.len(), 11);
        assert_eq!(
            u16::from_le_bytes([bytes[0], bytes[1]]),
            HEADER_ZC_ACCEPT_ENTER
        );
    }

    #[test]
    fn cz_enter_2019_roundtrip() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(&3i32.to_le_bytes());
        payload.extend_from_slice(&4u32.to_le_bytes());
        payload.push(1);

        let req = CzEnter2019::decode(&payload).unwrap();
        assert_eq!(req.account_id, 1);
        assert_eq!(req.char_id, 2);
        assert_eq!(req.auth_code, 3);
        assert_eq!(req.client_tick, 4);
        assert_eq!(req.sex, 1);
    }

    #[test]
    fn zc_accept_enter_2019_size() {
        let resp = ZcAcceptEnter2019 {
            server_tick: 12345,
            pos_x: 150,
            pos_y: 180,
            dir: 0,
            x_size: 0,
            y_size: 0,
            font: 0,
        };
        let bytes = resp.encode();
        assert_eq!(bytes.len(), 13);
        assert_eq!(
            u16::from_le_bytes([bytes[0], bytes[1]]),
            HEADER_ZC_ACCEPT_ENTER_2019
        );
    }

    #[test]
    fn pos_dir_roundtrip() {
        let (x, y, dir) = (150i16, 180i16, 3u8);
        let enc = encode_pos_dir(x, y, dir);
        let (dx, dy, ddir) = decode_pos_dir(&enc).unwrap();
        assert_eq!(dx, x);
        assert_eq!(dy, y);
        assert_eq!(ddir, dir);
    }

    #[test]
    fn move_data_roundtrip() {
        let (x0, y0, x1, y1, sx0, sy0) = (150i16, 180i16, 160i16, 190i16, 8u8, 8u8);
        let enc = encode_move_data(x0, y0, x1, y1, sx0, sy0);
        let (dx0, dy0, dx1, dy1, dsx0, dsy0) = decode_move_data(&enc).unwrap();
        assert_eq!(dx0, x0);
        assert_eq!(dy0, y0);
        assert_eq!(dx1, x1);
        assert_eq!(dy1, y1);
        assert_eq!(dsx0, sx0);
        assert_eq!(dsy0, sy0);
    }

    #[test]
    fn stand_entry_size() {
        let e = EntityAppearance {
            object_type: 0,
            aid: 1,
            gid: 1,
            speed: 150,
            job: 0,
            head: 0,
            x: 150,
            y: 180,
            dir: 0,
            hp: 100,
            max_hp: 100,
            level: 1,
            sex: 1,
            name: name_bytes("Test"),
            body: 0,
        };
        assert_eq!(e.encode_stand_entry().len(), 108);
        assert_eq!(e.encode_spawn_entry().len(), 107);
        assert_eq!(e.encode_move_entry(123, 150, 180, 160, 190).len(), 114);
    }
}
