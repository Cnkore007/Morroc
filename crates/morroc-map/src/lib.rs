//! Morroc 地图服务器（Map Server）。
//!
//! 支持 2019 main 客户端协议，处理进入地图、移动、攻击、实体刷新等流程。
//! 内部通过定时游戏循环更新单位位置，并采用 channel 将包异步推送给各客户端。

pub mod combat;
pub mod data;
pub mod status;
pub mod woe;

use futures::{SinkExt, StreamExt};
use morroc_db::SessionStore;
use morroc_net::{serve_with_listener, FramedSession, Packet};
use morroc_packets::map::{
    name_bytes, CzEnter, CzEnter2019, CzRequestAct, CzRequestMove, CzUseSkill, EntityAppearance,
    ZcAcceptEnter, ZcAcceptEnter2019, ZcNotifyAct, ZcNotifyMove, ZcNotifyPlayerMove,
    ZcNotifyVanish, HEADER_CZ_ENTER, HEADER_CZ_ENTER_2019, HEADER_CZ_REQUEST_ACT,
    HEADER_CZ_REQUEST_MOVE, HEADER_CZ_USE_SKILL, HEADER_ZC_ACCEPT_ENTER,
    HEADER_ZC_ACCEPT_ENTER_2019, HEADER_ZC_NOTIFY_ACT, HEADER_ZC_NOTIFY_MOVE,
    HEADER_ZC_NOTIFY_PLAYERMOVE, HEADER_ZC_NOTIFY_SPAWNENTRY, HEADER_ZC_NOTIFY_STANDENTRY,
    HEADER_ZC_NOTIFY_VANISH,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval, Duration};
use tracing::{info, warn};

use crate::combat::{compute_damage, CombatStats};
use crate::data::GameData;
use crate::status::StatusState;
use crate::woe::GuildState;

const DEFAULT_SPAWN_X: i16 = 150;
const DEFAULT_SPAWN_Y: i16 = 180;
const DEFAULT_DIR: u8 = 0;
const DEFAULT_SPEED: i16 = 150;
const TICK_INTERVAL_MS: u64 = 50;
const EMPERIUM_NPC_ID: u32 = 9000;

/// 地图上的通用单位实体。
#[derive(Debug, Clone)]
pub struct Entity {
    pub id: u32,
    pub aid: u32,
    pub gid: u32,
    pub object_type: u8,
    pub name: [u8; 24],
    pub job: i16,
    pub head: u16,
    pub speed: i16,
    pub x: i16,
    pub y: i16,
    pub dir: u8,
    pub hp: i32,
    pub max_hp: i32,
    pub level: i16,
    pub sex: u8,
    pub body: i16,
    pub str: i16,
    pub dex: i16,
    pub vit: i16,
    pub int: i16,
    pub def: i16,
    pub weapon_atk: i16,
    pub matk: i16,
}

impl Entity {
    /// 转换为实体外观包使用的结构。
    fn appearance(&self) -> EntityAppearance {
        EntityAppearance {
            object_type: self.object_type,
            aid: self.aid,
            gid: self.gid,
            speed: self.speed,
            job: self.job,
            head: self.head,
            x: self.x,
            y: self.y,
            dir: self.dir,
            hp: self.hp,
            max_hp: self.max_hp,
            level: self.level,
            sex: self.sex,
            name: self.name,
            body: self.body,
        }
    }
}

/// 玩家移动状态。
#[derive(Debug, Clone)]
pub(crate) struct MoveState {
    from_x: i16,
    from_y: i16,
    to_x: i16,
    to_y: i16,
    start_time: u32,
    move_time: u32,
}

/// 玩家。
#[derive(Debug, Clone)]
pub struct Player {
    pub account_id: u32,
    pub char_id: u32,
    pub entity: Entity,
    pub(crate) move_state: Option<MoveState>,
    pub sender: mpsc::UnboundedSender<Packet>,
}

/// NPC。
#[derive(Debug, Clone)]
pub struct Npc {
    pub entity: Entity,
}

/// 怪物。
#[derive(Debug, Clone)]
pub struct Monster {
    pub entity: Entity,
}

/// 地图状态。
pub struct MapState {
    pub name: String,
    pub players: HashMap<u32, Player>,
    pub npcs: HashMap<u32, Npc>,
    pub monsters: HashMap<u32, Monster>,
    pub guild_state: GuildState,
    pub emperium_id: Option<u32>,
    pub player_guilds: HashMap<u32, u32>, // account_id -> guild_id
    /// 每个实体（玩家/NPC/怪物）的当前状态集合，key 为实体 ID。
    pub statuses: HashMap<u32, StatusState>,
}

impl MapState {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            players: HashMap::new(),
            npcs: HashMap::new(),
            monsters: HashMap::new(),
            guild_state: GuildState::default(),
            emperium_id: None,
            player_guilds: HashMap::new(),
            statuses: HashMap::new(),
        }
    }
}

/// 地图实例，线程间共享句柄。
#[derive(Clone)]
pub struct MapInstance {
    inner: Arc<Mutex<MapState>>,
    game_data: Arc<RwLock<GameData>>,
}

impl MapInstance {
    /// 创建一张空地图，并生成默认 NPC 与怪物。
    ///
    /// 若 `spawn_db_mobs` 为 `true`，还会从 `game_data.mobs` 中注入前 100 个怪物。
    /// 若 `spawn_woe` 为 `true`，会生成最小 WoE 攻城器（Emperium）并注册一个 WoE 区域。
    pub fn new(
        name: impl Into<String>,
        game_data: GameData,
        spawn_db_mobs: bool,
        spawn_woe: bool,
    ) -> Self {
        let mut state = MapState::new(name);
        let game_data = Arc::new(RwLock::new(game_data));
        {
            let db = game_data.write().unwrap();
            // 默认 NPC
            state.npcs.insert(
                1000,
                Npc {
                    entity: Entity {
                        id: 1000,
                        aid: 1000,
                        gid: 1000,
                        object_type: 1, // BL_NPC
                        name: name_bytes("Kafra"),
                        job: 4,
                        head: 0,
                        speed: 200,
                        x: 152,
                        y: 182,
                        dir: 0,
                        hp: 100,
                        max_hp: 100,
                        level: 1,
                        sex: 1,
                        body: 0,
                        str: 1,
                        dex: 1,
                        vit: 10,
                        int: 1,
                        def: 0,
                        weapon_atk: 0,
                        matk: 0,
                    },
                },
            );
            state.npcs.insert(
                1001,
                Npc {
                    entity: Entity {
                        id: 1001,
                        aid: 1001,
                        gid: 1001,
                        object_type: 1,
                        name: name_bytes("Guide"),
                        job: 5,
                        head: 0,
                        speed: 200,
                        x: 148,
                        y: 178,
                        dir: 0,
                        hp: 100,
                        max_hp: 100,
                        level: 1,
                        sex: 1,
                        body: 0,
                        str: 1,
                        dex: 1,
                        vit: 10,
                        int: 1,
                        def: 0,
                        weapon_atk: 0,
                        matk: 0,
                    },
                },
            );
            // 默认怪物
            state.monsters.insert(
                2000,
                Monster {
                    entity: Entity {
                        id: 2000,
                        aid: 2000,
                        gid: 2000,
                        object_type: 2, // BL_MOB
                        name: name_bytes("Poring"),
                        job: 1002,
                        head: 0,
                        speed: 300,
                        x: 155,
                        y: 185,
                        dir: 0,
                        hp: 50,
                        max_hp: 50,
                        level: 1,
                        sex: 0,
                        body: 0,
                        str: 3,
                        dex: 2,
                        vit: 5,
                        int: 1,
                        def: 5,
                        weapon_atk: 5,
                        matk: 0,
                    },
                },
            );
            state.monsters.insert(
                2001,
                Monster {
                    entity: Entity {
                        id: 2001,
                        aid: 2001,
                        gid: 2001,
                        object_type: 2,
                        name: name_bytes("Lunatic"),
                        job: 1003,
                        head: 0,
                        speed: 300,
                        x: 145,
                        y: 175,
                        dir: 0,
                        hp: 60,
                        max_hp: 60,
                        level: 2,
                        sex: 0,
                        body: 0,
                        str: 4,
                        dex: 3,
                        vit: 6,
                        int: 1,
                        def: 6,
                        weapon_atk: 6,
                        matk: 0,
                    },
                },
            );

            if spawn_db_mobs {
                spawn_mobs_from_database(&mut state, &db);
            }

            // 从转换后的 NPC 脚本生成当前地图的 NPC。
            let map_name = state.name.clone();
            spawn_npcs_from_database(&mut state, &db, &map_name);

            if spawn_woe {
                let emperium_id = EMPERIUM_NPC_ID;
                state.npcs.insert(
                    emperium_id,
                    Npc {
                        entity: Entity {
                            id: emperium_id,
                            aid: emperium_id,
                            gid: emperium_id,
                            object_type: 1, // BL_NPC
                            name: name_bytes("Emperium"),
                            job: 1000,
                            head: 0,
                            speed: 0,
                            x: 160,
                            y: 180,
                            dir: 0,
                            hp: 1000,
                            max_hp: 1000,
                            level: 1,
                            sex: 0,
                            body: 0,
                            str: 1,
                            dex: 1,
                            vit: 50,
                            int: 1,
                            def: 50,
                            weapon_atk: 0,
                            matk: 0,
                        },
                    },
                );
                state.emperium_id = Some(emperium_id);
                state.guild_state.register_zone(
                    1,
                    "prtg_cas01",
                    150,
                    170,
                    170,
                    190,
                    emperium_id,
                    1000,
                );
            }
        }

        Self {
            inner: Arc::new(Mutex::new(state)),
            game_data,
        }
    }

    /// 读取运行时游戏数据库。
    pub fn game_data(&self) -> std::sync::RwLockReadGuard<'_, GameData> {
        self.game_data.read().unwrap()
    }

    /// 写入运行时游戏数据库。
    pub fn game_data_mut(&self) -> std::sync::RwLockWriteGuard<'_, GameData> {
        self.game_data.write().unwrap()
    }

    /// 玩家进入地图，返回 (发给新玩家的包, 发给其他玩家的包)。
    pub fn player_enter(&self, player: Player) -> (Vec<Packet>, Vec<Packet>) {
        let mut state = self.inner.lock().unwrap();
        let appearance = player.entity.appearance();
        let spawn_packet = Packet::new(
            HEADER_ZC_NOTIFY_SPAWNENTRY,
            appearance.encode_spawn_entry()[2..].to_vec(),
        );

        // 发给新玩家：地图上已有实体
        let mut to_self = Vec::new();
        for npc in state.npcs.values() {
            let bytes = npc.entity.appearance().encode_stand_entry();
            to_self.push(Packet::new(
                HEADER_ZC_NOTIFY_STANDENTRY,
                bytes[2..].to_vec(),
            ));
        }
        for mob in state.monsters.values() {
            let bytes = mob.entity.appearance().encode_spawn_entry();
            to_self.push(Packet::new(
                HEADER_ZC_NOTIFY_SPAWNENTRY,
                bytes[2..].to_vec(),
            ));
        }
        for existing in state.players.values() {
            let bytes = existing.entity.appearance().encode_stand_entry();
            to_self.push(Packet::new(
                HEADER_ZC_NOTIFY_STANDENTRY,
                bytes[2..].to_vec(),
            ));
        }

        let aid = player.account_id;
        state.players.insert(aid, player);

        (to_self, vec![spawn_packet])
    }

    /// 移除玩家。
    pub fn remove_player(&self, account_id: u32) -> Option<Player> {
        let mut state = self.inner.lock().unwrap();
        state.players.remove(&account_id)
    }

    /// 玩家移动请求。
    pub fn player_move(
        &self,
        account_id: u32,
        to_x: i16,
        to_y: i16,
        dir: u8,
        tick: u32,
    ) -> Option<(Packet, Packet)> {
        let mut state = self.inner.lock().unwrap();
        if state
            .statuses
            .get(&account_id)
            .map(|s| s.blocks_movement())
            .unwrap_or(false)
        {
            return None;
        }
        let player = state.players.get_mut(&account_id)?;
        let from_x = player.entity.x;
        let from_y = player.entity.y;
        let dx = (to_x - from_x) as f64;
        let dy = (to_y - from_y) as f64;
        let distance = (dx * dx + dy * dy).sqrt();
        let speed = player.entity.speed.max(1) as f64;
        let move_time = (distance * speed * 10.0) as u32;

        player.move_state = Some(MoveState {
            from_x,
            from_y,
            to_x,
            to_y,
            start_time: tick,
            move_time,
        });
        player.entity.dir = dir;

        let to_self = Packet::new(
            HEADER_ZC_NOTIFY_PLAYERMOVE,
            ZcNotifyPlayerMove {
                start_time: tick,
                from_x,
                from_y,
                to_x,
                to_y,
            }
            .encode_payload(),
        );
        let to_others = Packet::new(
            HEADER_ZC_NOTIFY_MOVE,
            ZcNotifyMove {
                id: account_id,
                start_time: tick,
                from_x,
                from_y,
                to_x,
                to_y,
            }
            .encode_payload(),
        );
        Some((to_self, to_others))
    }

    /// 玩家发起动作/攻击，可选使用技能。
    ///
    /// `skill` 为 `Some((skill_id, skill_level))` 时表示技能攻击，否则为普通攻击。
    pub fn player_action(
        &self,
        account_id: u32,
        target_id: u32,
        action: u8,
        tick: u32,
        skill: Option<(u16, i16)>,
    ) -> Option<Vec<Packet>> {
        let mut state = self.inner.lock().unwrap();
        if state
            .statuses
            .get(&account_id)
            .map(|s| s.blocks_attack())
            .unwrap_or(false)
        {
            return None;
        }
        let player_entity = state.players.get(&account_id)?.entity.clone();
        let source = player_entity.aid;

        let source_modifier = state.statuses.get(&account_id).map(|s| s.modifier());
        let source_stats = entity_to_combat_stats(&player_entity, source_modifier.as_ref());

        let skill_info = skill
            .as_ref()
            .and_then(|(id, _)| self.game_data.read().unwrap().skills.get(*id).cloned());
        let skill_level = skill.map(|(_, lv)| lv).unwrap_or(0);

        let mut packets = Vec::new();

        // 尝试攻击怪物
        if state.monsters.contains_key(&target_id) {
            let damage = {
                let mob = state.monsters.get(&target_id).unwrap();
                let target_modifier = state.statuses.get(&target_id).map(|s| s.modifier());
                let target_stats = entity_to_combat_stats(&mob.entity, target_modifier.as_ref());
                compute_damage(
                    &source_stats,
                    &target_stats,
                    skill_info.as_ref(),
                    skill_level,
                )
            };
            Self::apply_status_from_skill(
                &self.game_data,
                &mut state.statuses,
                target_id,
                tick,
                skill_info.as_ref(),
            );
            let mob = state.monsters.get_mut(&target_id).unwrap();
            mob.entity.hp -= damage;
            if mob.entity.hp <= 0 {
                packets.push(Packet::new(
                    HEADER_ZC_NOTIFY_VANISH,
                    ZcNotifyVanish {
                        id: target_id,
                        vanish_type: 1, // 死亡消失
                    }
                    .encode_payload(),
                ));
                state.monsters.remove(&target_id);
            }
            packets.push(Packet::new(
                HEADER_ZC_NOTIFY_ACT,
                ZcNotifyAct {
                    source_id: source,
                    target_id,
                    start_time: tick,
                    attack_mt: 0,
                    attacked_mt: 0,
                    damage,
                    count: 1,
                    action,
                    left_damage: 0,
                }
                .encode_payload(),
            ));
            return Some(packets);
        }

        // 尝试攻击 WoE 攻城器（Emperium）
        if Some(target_id) == state.emperium_id {
            let emperium = state.npcs.get(&target_id)?;
            let target_modifier = state.statuses.get(&target_id).map(|s| s.modifier());
            let target_stats = entity_to_combat_stats(&emperium.entity, target_modifier.as_ref());
            let damage = compute_damage(
                &source_stats,
                &target_stats,
                skill_info.as_ref(),
                skill_level,
            );
            let attacker_guild = state.player_guilds.get(&account_id).copied();
            let (hp, new_owner) =
                state
                    .guild_state
                    .damage_emperium(target_id, damage, attacker_guild);
            if hp <= 0 {
                if let Some(owner) = new_owner {
                    info!("WoE 区域被公会 {} 占领", owner);
                }
                state.npcs.remove(&target_id);
                state.emperium_id = None;
                packets.push(Packet::new(
                    HEADER_ZC_NOTIFY_VANISH,
                    ZcNotifyVanish {
                        id: target_id,
                        vanish_type: 1, // 死亡消失
                    }
                    .encode_payload(),
                ));
            }
            packets.push(Packet::new(
                HEADER_ZC_NOTIFY_ACT,
                ZcNotifyAct {
                    source_id: source,
                    target_id,
                    start_time: tick,
                    attack_mt: 0,
                    attacked_mt: 0,
                    damage,
                    count: 1,
                    action,
                    left_damage: 0,
                }
                .encode_payload(),
            ));
            return Some(packets);
        }

        // 尝试攻击 NPC（仅演示，不掉血）
        if state.npcs.contains_key(&target_id) {
            packets.push(Packet::new(
                HEADER_ZC_NOTIFY_ACT,
                ZcNotifyAct {
                    source_id: source,
                    target_id,
                    start_time: tick,
                    attack_mt: 0,
                    attacked_mt: 0,
                    damage: 0,
                    count: 1,
                    action,
                    left_damage: 0,
                }
                .encode_payload(),
            ));
            return Some(packets);
        }

        None
    }

    /// 将包发送给指定玩家。
    pub fn send_to(&self, account_id: u32, packets: Vec<Packet>) {
        let state = self.inner.lock().unwrap();
        if let Some(player) = state.players.get(&account_id) {
            if player.sender.is_closed() {
                return;
            }
            for packet in packets {
                let _ = player.sender.send(packet);
            }
        }
    }

    /// 广播给所有玩家，可排除指定 account_id。
    pub fn broadcast(&self, packets: Vec<Packet>, except: Option<u32>) {
        let state = self.inner.lock().unwrap();
        for player in state.players.values() {
            if Some(player.account_id) == except || player.sender.is_closed() {
                continue;
            }
            for packet in packets.clone() {
                let _ = player.sender.send(packet);
            }
        }
    }

    /// 游戏循环 tick：更新单位位置、状态效果等。
    pub fn tick(&self) {
        let mut state = self.inner.lock().unwrap();
        let now = system_tick();
        for player in state.players.values_mut() {
            if let Some(move_state) = &player.move_state {
                let elapsed = now.saturating_sub(move_state.start_time);
                if elapsed >= move_state.move_time {
                    player.entity.x = move_state.to_x;
                    player.entity.y = move_state.to_y;
                    player.move_state = None;
                } else {
                    let t = elapsed as f64 / move_state.move_time as f64;
                    player.entity.x = move_state.from_x
                        + ((move_state.to_x - move_state.from_x) as f64 * t) as i16;
                    player.entity.y = move_state.from_y
                        + ((move_state.to_y - move_state.from_y) as f64 * t) as i16;
                }
            }
        }
        // 状态效果 tick：先计算每个实体的 HP 变化，再单独写入实体。
        let player_ids: Vec<u32> = state.players.keys().copied().collect();
        for id in player_ids {
            let hp_change = if let Some(status_state) = state.statuses.get_mut(&id) {
                status_state.tick(now)
            } else {
                0
            };
            if hp_change != 0 {
                if let Some(player) = state.players.get_mut(&id) {
                    player.entity.hp =
                        (player.entity.hp + hp_change).clamp(0, player.entity.max_hp);
                }
            }
        }
        let monster_ids: Vec<u32> = state.monsters.keys().copied().collect();
        for id in monster_ids {
            let hp_change = if let Some(status_state) = state.statuses.get_mut(&id) {
                status_state.tick(now)
            } else {
                0
            };
            if hp_change != 0 {
                if let Some(monster) = state.monsters.get_mut(&id) {
                    monster.entity.hp =
                        (monster.entity.hp + hp_change).clamp(0, monster.entity.max_hp);
                }
            }
        }
    }

    /// 创建公会。
    pub fn create_guild(&self, id: u32, name: impl Into<String>, master_account_id: u32) {
        let mut state = self.inner.lock().unwrap();
        state.guild_state.create_guild(id, name, master_account_id);
    }

    /// 将玩家加入指定公会。
    pub fn set_player_guild(&self, account_id: u32, guild_id: u32) {
        let mut state = self.inner.lock().unwrap();
        state.player_guilds.insert(account_id, guild_id);
    }

    /// 查询 WoE 区域所有者。
    pub fn zone_owner(&self, zone_id: u32) -> Option<u32> {
        let state = self.inner.lock().unwrap();
        state.guild_state.zone_owner(zone_id)
    }

    /// 查询当前 Emperium HP。
    pub fn emperium_hp(&self) -> Option<i32> {
        let state = self.inner.lock().unwrap();
        state
            .emperium_id
            .and_then(|id| state.guild_state.emperium_hp.get(&id).copied())
    }

    /// 返回地图当前状态的快照（供 Agent / UI 查询）。
    pub fn snapshot(&self) -> serde_json::Value {
        let state = self.inner.lock().unwrap();
        let data = self.game_data.read().unwrap();
        fn name_str(bytes: &[u8; 24]) -> String {
            String::from_utf8_lossy(bytes)
                .trim_end_matches('\0')
                .to_string()
        }
        serde_json::json!({
            "name": state.name,
            "player_count": state.players.len(),
            "players": state.players.values().map(|p| name_str(&p.entity.name)).collect::<Vec<_>>(),
            "npc_count": state.npcs.len(),
            "npcs": state.npcs.values().map(|n| name_str(&n.entity.name)).collect::<Vec<_>>(),
            "monster_count": state.monsters.len(),
            "monsters": state.monsters.values().map(|m| name_str(&m.entity.name)).collect::<Vec<_>>(),
            "item_count": data.item_count(),
            "mob_count": data.mob_count(),
            "emperium_id": state.emperium_id,
            "zones": state.guild_state.zones.values().map(|z| serde_json::json!({
                "id": z.id,
                "name": z.name,
                "owner_guild_id": z.owner_guild_id,
            })).collect::<Vec<_>>(),
        })
    }

    /// 在地图中动态生成一个 NPC，返回新实体 ID。
    pub fn spawn_dynamic_npc(&self, name: &str, x: i16, y: i16, _sprite: &str) -> Option<u32> {
        let mut state = self.inner.lock().unwrap();
        let mut id = 10000u32;
        while state.npcs.contains_key(&id) || state.monsters.contains_key(&id) {
            id += 1;
        }
        let entity = Entity {
            id,
            aid: id,
            gid: id,
            object_type: 1, // BL_NPC
            name: name_bytes(name),
            job: 0,
            head: 0,
            speed: 200,
            x,
            y,
            dir: 0,
            hp: 100,
            max_hp: 100,
            level: 1,
            sex: 1,
            body: 0,
            str: 1,
            dex: 1,
            vit: 1,
            int: 1,
            def: 0,
            weapon_atk: 0,
            matk: 0,
        };
        state.npcs.insert(id, Npc { entity });
        Some(id)
    }

    /// 修改已有 NPC。
    pub fn update_npc(&self, id: u32, f: impl FnOnce(&mut Npc)) -> bool {
        let mut state = self.inner.lock().unwrap();
        if let Some(npc) = state.npcs.get_mut(&id) {
            f(npc);
            true
        } else {
            false
        }
    }

    /// 修改已有怪物。
    pub fn update_monster(&self, id: u32, f: impl FnOnce(&mut Monster)) -> bool {
        let mut state = self.inner.lock().unwrap();
        if let Some(mob) = state.monsters.get_mut(&id) {
            f(mob);
            true
        } else {
            false
        }
    }

    /// 设置 NPC 或怪物的坐标。
    pub fn set_entity_position(&self, id: u32, x: i16, y: i16) -> bool {
        let mut state = self.inner.lock().unwrap();
        if let Some(npc) = state.npcs.get_mut(&id) {
            npc.entity.x = x;
            npc.entity.y = y;
            return true;
        }
        if let Some(mob) = state.monsters.get_mut(&id) {
            mob.entity.x = x;
            mob.entity.y = y;
            return true;
        }
        false
    }

    /// 对指定目标施加技能携带的状态效果。
    fn apply_status_from_skill(
        game_data: &Arc<RwLock<GameData>>,
        statuses: &mut HashMap<u32, StatusState>,
        target_id: u32,
        tick: u32,
        skill_info: Option<&crate::combat::SkillInfo>,
    ) {
        let Some(skill) = skill_info else { return };
        let Some(status_id) = skill.status_id else {
            return;
        };
        let Some(status) = game_data.read().unwrap().statuses.get(status_id).cloned() else {
            return;
        };
        statuses.entry(target_id).or_default().apply(status, tick);
    }

    /// 对指定目标施加状态效果。
    pub fn apply_status(&self, target_id: u32, status_id: u16, tick: u32) -> bool {
        let mut state = self.inner.lock().unwrap();
        let Some(status) = self
            .game_data
            .read()
            .unwrap()
            .statuses
            .get(status_id)
            .cloned()
        else {
            return false;
        };
        state
            .statuses
            .entry(target_id)
            .or_default()
            .apply(status, tick);
        true
    }

    /// 移除指定目标身上的状态。
    pub fn remove_status(&self, target_id: u32, status_id: u16) -> bool {
        let mut state = self.inner.lock().unwrap();
        if let Some(status_state) = state.statuses.get_mut(&target_id) {
            status_state.remove(status_id)
        } else {
            false
        }
    }
}

fn entity_to_combat_stats(
    entity: &Entity,
    modifier: Option<&crate::status::StatusModifier>,
) -> CombatStats {
    let m = modifier.copied().unwrap_or_default();
    CombatStats {
        level: entity.level,
        str: entity.str.saturating_add(m.str),
        dex: entity.dex.saturating_add(m.dex),
        vit: entity.vit.saturating_add(m.vit),
        int: entity.int.saturating_add(m.int),
        def: entity.def.saturating_add(m.def),
        weapon_atk: entity.weapon_atk.saturating_add(m.atk),
        matk: entity.matk.saturating_add(m.matk),
    }
}

/// 从 `GameData` 中注入怪物到地图。
///
/// 为避免一次性生成过多实体影响测试与性能，默认最多注入 100 个怪物。
fn spawn_mobs_from_database(state: &mut MapState, game_data: &GameData) {
    const MAX_DB_MOBS: usize = 100;
    for (idx, mob) in game_data.mobs.values().take(MAX_DB_MOBS).enumerate() {
        let id = mob.id as u32;
        if state.npcs.contains_key(&id) || state.monsters.contains_key(&id) {
            continue;
        }
        let x = (150 + (idx % 10) * 2) as i16;
        let y = (180 + (idx / 10) * 2) as i16;
        let entity = Entity {
            id,
            aid: id,
            gid: id,
            object_type: 2, // BL_MOB
            name: name_bytes(&mob.name),
            job: mob.id as i16,
            head: 0,
            speed: mob.move_speed.unwrap_or(300) as i16,
            x,
            y,
            dir: 0,
            hp: mob.hp.unwrap_or(50) as i32,
            max_hp: mob.hp.unwrap_or(50) as i32,
            level: mob.level.unwrap_or(1) as i16,
            sex: 0,
            body: 0,
            str: mob.str.unwrap_or(1) as i16,
            dex: mob.dex.unwrap_or(1) as i16,
            vit: mob.vit.unwrap_or(1) as i16,
            int: mob.int.unwrap_or(1) as i16,
            def: mob.def.unwrap_or(0) as i16,
            weapon_atk: mob.attack_min.unwrap_or(5) as i16,
            matk: mob.int.unwrap_or(1) as i16,
        };
        state.monsters.insert(id, Monster { entity });
    }
}

/// 从 `GameData` 中注入转换后的 NPC 脚本到当前地图。
fn spawn_npcs_from_database(state: &mut MapState, game_data: &GameData, map_name: &str) {
    const NPC_ID_BASE: u32 = 10000;
    for (idx, npc) in game_data
        .npcs
        .iter()
        .filter(|n| n.map == map_name)
        .enumerate()
    {
        let id = NPC_ID_BASE + idx as u32;
        if state.npcs.contains_key(&id) || state.monsters.contains_key(&id) {
            continue;
        }
        let entity = Entity {
            id,
            aid: id,
            gid: id,
            object_type: 1, // BL_NPC
            name: name_bytes(&npc.name),
            job: 0,
            head: 0,
            speed: 200,
            x: npc.x,
            y: npc.y,
            dir: npc.facing,
            hp: 100,
            max_hp: 100,
            level: 1,
            sex: 1,
            body: 0,
            str: 1,
            dex: 1,
            vit: 1,
            int: 1,
            def: 0,
            weapon_atk: 0,
            matk: 0,
        };
        state.npcs.insert(id, Npc { entity });
    }
}

/// 地图服务器。
#[derive(Clone)]
pub struct MapServer {
    pub map: MapInstance,
    pub addr: SocketAddr,
    pub sessions: Arc<dyn SessionStore>,
}

impl MapServer {
    pub fn new(
        addr: SocketAddr,
        sessions: Arc<dyn SessionStore>,
        game_data: GameData,
        spawn_db_mobs: bool,
        spawn_woe: bool,
    ) -> Self {
        Self {
            map: MapInstance::new("prontera", game_data, spawn_db_mobs, spawn_woe),
            addr,
            sessions,
        }
    }

    /// 使用默认空会话创建地图服务器（适合测试）。
    pub fn new_empty_sessions(addr: SocketAddr) -> Self {
        Self::new(
            addr,
            Arc::new(morroc_db::LocalSessionStore::new()),
            GameData::default(),
            false,
            false,
        )
    }

    /// 注册一个已登录会话，供地图服务器验证 CZ_ENTER。
    pub async fn register_session(&self, account_id: u32, auth_code: i32) {
        let _ = self.sessions.insert_session(account_id, auth_code).await;
    }

    /// 启动地图服务器。`ready` 通道可选，用于回传实际绑定的地址。
    pub async fn run(&self, ready: Option<oneshot::Sender<SocketAddr>>) -> anyhow::Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        let bound_addr = listener.local_addr()?;
        if let Some(ready) = ready {
            let _ = ready.send(bound_addr);
        }
        info!("地图服务器监听已启动: {}", bound_addr);

        let map = self.map.clone();
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_millis(TICK_INTERVAL_MS));
            loop {
                ticker.tick().await;
                map.tick();
            }
        });

        let map = self.map.clone();
        let sessions = self.sessions.clone();
        serve_with_listener(listener, move |framed, peer| {
            let map = map.clone();
            let sessions = sessions.clone();
            async move { handle_client(framed, peer, sessions, map).await }
        })
        .await
    }
}

fn system_tick() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u32
}

async fn handle_client(
    mut framed: FramedSession,
    peer: SocketAddr,
    sessions: Arc<dyn SessionStore>,
    map: MapInstance,
) -> anyhow::Result<()> {
    info!("地图客户端连接: {}", peer);

    let (tx, mut rx) = mpsc::unbounded_channel::<Packet>();
    let mut account_id: Option<u32> = None;

    loop {
        tokio::select! {
            result = framed.next() => {
                let Some(packet) = result else { break; };
                let packet = packet?;
                match packet.packet_id {
                    HEADER_CZ_ENTER | HEADER_CZ_ENTER_2019 => {
                        let (req_account_id, req_char_id, auth_code) = if packet.packet_id == HEADER_CZ_ENTER {
                            let req = CzEnter::decode(&packet.payload)?;
                            (req.account_id, req.char_id, req.auth_code)
                        } else {
                            let req = CzEnter2019::decode(&packet.payload)?;
                            (req.account_id, req.char_id, req.auth_code)
                        };
                        info!("地图进入请求: account_id={}, char_id={}", req_account_id, req_char_id);
                        let valid = sessions
                            .get_session(req_account_id)
                            .await
                            .ok()
                            .flatten()
                            .map(|code| code == auth_code)
                            .unwrap_or(false);
                        if !valid {
                            warn!("地图会话验证失败: account_id={}", req_account_id);
                        }


                        let tick = system_tick();
                        let (enter_packet, accept_header) = if packet.packet_id == HEADER_CZ_ENTER {
                            (
                                Packet::new(
                                    HEADER_ZC_ACCEPT_ENTER,
                                    ZcAcceptEnter {
                                        server_tick: tick,
                                        pos_x: DEFAULT_SPAWN_X,
                                        pos_y: DEFAULT_SPAWN_Y,
                                        dir: DEFAULT_DIR,
                                    }
                                    .encode_payload(),
                                ),
                                HEADER_ZC_ACCEPT_ENTER,
                            )
                        } else {
                            (
                                Packet::new(
                                    HEADER_ZC_ACCEPT_ENTER_2019,
                                    ZcAcceptEnter2019 {
                                        server_tick: tick,
                                        pos_x: DEFAULT_SPAWN_X,
                                        pos_y: DEFAULT_SPAWN_Y,
                                        dir: DEFAULT_DIR,
                                        x_size: 0,
                                        y_size: 0,
                                        font: 0,
                                    }
                                    .encode_payload(),
                                ),
                                HEADER_ZC_ACCEPT_ENTER_2019,
                            )
                        };

                        let _ = tx.send(enter_packet);

                        let player = Player {
                            account_id: req_account_id,
                            char_id: req_char_id,
                            entity: Entity {
                                id: req_account_id,
                                aid: req_account_id,
                                gid: req_char_id,
                                object_type: 0, // BL_PC
                                name: name_bytes("Player"),
                                job: 0,
                                head: 1,
                                speed: DEFAULT_SPEED,
                                x: DEFAULT_SPAWN_X,
                                y: DEFAULT_SPAWN_Y,
                                dir: DEFAULT_DIR,
                                hp: 100,
                                max_hp: 100,
                                level: 10,
                                sex: 1,
                                body: 0,
                                str: 10,
                                dex: 10,
                                vit: 10,
                                int: 1,
                                def: 0,
                                weapon_atk: 20,
                                matk: 0,
                            },
                            move_state: None,
                            sender: tx.clone(),
                        };
                        let (to_self, to_others) = map.player_enter(player);
                        map.send_to(req_account_id, to_self);
                        map.broadcast(to_others, Some(req_account_id));
                        account_id = Some(req_account_id);

                        info!("玩家已加入地图: account_id={} (accept=0x{:04x})", req_account_id, accept_header);
                    }
                    HEADER_CZ_REQUEST_MOVE => {
                        let req = CzRequestMove::decode(&packet.payload)?;
                        if let Some(id) = account_id {
                            let tick = system_tick();
                            if let Some((to_self, to_others)) = map.player_move(id, req.x, req.y, req.dir, tick) {
                                map.send_to(id, vec![to_self]);
                                map.broadcast(vec![to_others], Some(id));
                            }
                        }
                    }
                    HEADER_CZ_REQUEST_ACT => {
                        let req = CzRequestAct::decode(&packet.payload)?;
                        if let Some(id) = account_id {
                            let tick = system_tick();
                            if let Some(packets) = map.player_action(id, req.target_id, req.action, tick, None) {
                                map.broadcast(packets, None);
                            }
                        }
                    }
                    HEADER_CZ_USE_SKILL => {
                        let req = CzUseSkill::decode(&packet.payload)?;
                        if let Some(id) = account_id {
                            let tick = system_tick();
                            if let Some(packets) = map.player_action(id, req.target_id, 0, tick, Some((req.skill_id, req.skill_lv as i16))) {
                                map.broadcast(packets, None);
                            }
                        }
                    }
                    _ => warn!("地图服务器收到未处理包 0x{:04x}", packet.packet_id),
                }

                // 发送本次处理入队的所有出站包。
                while let Ok(packet) = rx.try_recv() {
                    framed.send(packet).await?;
                }
            }
            maybe_packet = rx.recv() => {
                let Some(packet) = maybe_packet else { break; };
                framed.send(packet).await?;
            }
        }
    }

    if let Some(id) = account_id {
        if let Some(player) = map.remove_player(id) {
            map.broadcast(
                vec![Packet::new(
                    HEADER_ZC_NOTIFY_VANISH,
                    ZcNotifyVanish {
                        id: player.entity.aid,
                        vanish_type: 2, // 登出消失
                    }
                    .encode_payload(),
                )],
                Some(id),
            );
            // 玩家可能在断开前仍持有出站包，发送消失通知。
            while let Ok(packet) = rx.try_recv() {
                let _ = framed.send(packet).await;
            }
        }
    }

    info!("地图客户端断开: {}", peer);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{SkillDatabase, SkillInfo};
    use crate::status::StatusDatabase;
    use morroc_packets::packet_len;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    async fn start_test_server() -> (SocketAddr, MapServer) {
        let server = MapServer::new_empty_sessions("127.0.0.1:0".parse().unwrap());
        server.register_session(1, 123456).await;
        let (tx, rx) = oneshot::channel();
        tokio::spawn({
            let server = server.clone();
            async move {
                let _ = server.run(Some(tx)).await;
            }
        });
        let addr = rx.await.expect("服务器应发送绑定地址");
        (addr, server)
    }

    fn write_packet(packet_id: u16, payload: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&packet_id.to_le_bytes());
        buf.extend_from_slice(payload);
        buf
    }

    async fn read_response(stream: &mut TcpStream) -> Vec<u8> {
        let mut id_buf = [0u8; 2];
        stream.read_exact(&mut id_buf).await.unwrap();
        let packet_id = u16::from_le_bytes(id_buf);
        let len = packet_len(packet_id).unwrap_or_else(|| panic!("未知包 ID: 0x{:04x}", packet_id));
        let rest = if len == -1 {
            let mut len_buf = [0u8; 2];
            stream.read_exact(&mut len_buf).await.unwrap();
            let total_len = u16::from_le_bytes(len_buf) as usize;
            let mut rest = vec![0u8; total_len - 2];
            rest[..2].copy_from_slice(&len_buf);
            stream.read_exact(&mut rest[2..]).await.unwrap();
            rest
        } else {
            let mut rest = vec![0u8; len as usize - 2];
            stream.read_exact(&mut rest).await.unwrap();
            rest
        };
        let mut full = id_buf.to_vec();
        full.extend_from_slice(&rest);
        full
    }

    #[tokio::test]
    async fn map_2019_enter_only() {
        let (addr, _server) = start_test_server().await;
        let mut stream = TcpStream::connect(addr).await.unwrap();

        let payload = {
            let mut p = Vec::new();
            p.extend_from_slice(&1u32.to_le_bytes()); // account_id
            p.extend_from_slice(&10u32.to_le_bytes()); // char_id
            p.extend_from_slice(&123456i32.to_le_bytes()); // auth_code
            p.extend_from_slice(&0u32.to_le_bytes()); // client_tick
            p.push(1); // sex
            p
        };
        stream
            .write_all(&write_packet(HEADER_CZ_ENTER_2019, &payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();

        // 等待进入确认
        let _accept = read_response(&mut stream).await;

        // 应收到 4 个实体刷新包：2 个 NPC + 2 个怪物
        for _ in 0..4 {
            tokio::time::timeout(Duration::from_secs(2), read_response(&mut stream))
                .await
                .expect("应在2秒内收到刷新包");
        }
    }

    async fn start_test_server_with_game_data(
        game_data: GameData,
        spawn_db_mobs: bool,
        spawn_woe: bool,
    ) -> (SocketAddr, MapServer) {
        let server = MapServer::new(
            "127.0.0.1:0".parse().unwrap(),
            Arc::new(morroc_db::LocalSessionStore::new()),
            game_data,
            spawn_db_mobs,
            spawn_woe,
        );
        server.register_session(1, 123456).await;
        let (tx, rx) = oneshot::channel();
        tokio::spawn({
            let server = server.clone();
            async move {
                let _ = server.run(Some(tx)).await;
            }
        });
        let addr = rx.await.expect("服务器应发送绑定地址");
        (addr, server)
    }

    fn parse_act_damage(response: &[u8]) -> i32 {
        // ZC_NOTIFY_ACT payload: source_id(4), target_id(4), start_time(4), attack_mt(4), attacked_mt(4), damage(4), ...
        // response[0..2] 是包头，damage 起始于 payload offset 20，即 response offset 22。
        i32::from_le_bytes([response[22], response[23], response[24], response[25]])
    }

    #[tokio::test]
    async fn map_2019_attack_uses_damage_formula() {
        let (addr, _server) = start_test_server().await;
        let mut stream = TcpStream::connect(addr).await.unwrap();

        let payload = {
            let mut p = Vec::new();
            p.extend_from_slice(&1u32.to_le_bytes());
            p.extend_from_slice(&10u32.to_le_bytes());
            p.extend_from_slice(&123456i32.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes());
            p.push(1);
            p
        };
        stream
            .write_all(&write_packet(HEADER_CZ_ENTER_2019, &payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let _accept = read_response(&mut stream).await;
        for _ in 0..4 {
            read_response(&mut stream).await;
        }

        let move_payload = morroc_packets::map::encode_pos_dir(160, 190, 0);
        stream
            .write_all(&write_packet(HEADER_CZ_REQUEST_MOVE, &move_payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let _move = read_response(&mut stream).await;

        let mut act_payload = Vec::new();
        act_payload.extend_from_slice(&2000u32.to_le_bytes());
        act_payload.push(0);
        stream
            .write_all(&write_packet(HEADER_CZ_REQUEST_ACT, &act_payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let act_response = read_response(&mut stream).await;
        let act_id = u16::from_le_bytes([act_response[0], act_response[1]]);
        assert_eq!(act_id, HEADER_ZC_NOTIFY_ACT);

        let damage = parse_act_damage(&act_response);
        assert!(damage > 0, "伤害应大于 0");
        assert_ne!(damage, 25, "不应再使用固定 stub 伤害 25");
    }

    #[tokio::test]
    async fn map_2019_skill_cast_deals_damage() {
        let skill_json = r#"[
            {"id": 1, "name": "Bash", "max_level": 10, "element": "Neutral", "attack_type": "Physical", "damage_factor": 1.2}
        ]"#;
        let game_data = GameData {
            items: HashMap::new(),
            mobs: HashMap::new(),
            skills: SkillDatabase::load_from_str(skill_json).unwrap(),
            npcs: Vec::new(),
            statuses: StatusDatabase::default(),
        };
        let (addr, _server) = start_test_server_with_game_data(game_data, false, false).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();

        let payload = {
            let mut p = Vec::new();
            p.extend_from_slice(&1u32.to_le_bytes());
            p.extend_from_slice(&10u32.to_le_bytes());
            p.extend_from_slice(&123456i32.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes());
            p.push(1);
            p
        };
        stream
            .write_all(&write_packet(HEADER_CZ_ENTER_2019, &payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let _accept = read_response(&mut stream).await;
        for _ in 0..4 {
            read_response(&mut stream).await;
        }

        let mut skill_payload = Vec::new();
        skill_payload.extend_from_slice(&1u16.to_le_bytes()); // Bash
        skill_payload.extend_from_slice(&1u16.to_le_bytes()); // level 1
        skill_payload.extend_from_slice(&2000u32.to_le_bytes()); // Poring
        stream
            .write_all(&write_packet(HEADER_CZ_USE_SKILL, &skill_payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let skill_response = read_response(&mut stream).await;
        let skill_id = u16::from_le_bytes([skill_response[0], skill_response[1]]);
        assert_eq!(skill_id, HEADER_ZC_NOTIFY_ACT);
        let skill_damage = parse_act_damage(&skill_response);
        assert!(skill_damage > 0, "技能伤害应大于 0");
    }

    #[tokio::test]
    async fn map_2019_injects_database_mobs() {
        let json = r#"{
            "items": [],
            "mobs": [
                {"id": 1234, "sprite_name": "TEST_MOB", "name": "Test Mob", "hp": 100, "level": 5}
            ],
            "skills": []
        }"#;
        let game_data = GameData::load_from_str(json).unwrap();
        let (addr, _server) = start_test_server_with_game_data(game_data, true, false).await;
        let mut stream = TcpStream::connect(addr).await.unwrap();

        let payload = {
            let mut p = Vec::new();
            p.extend_from_slice(&1u32.to_le_bytes());
            p.extend_from_slice(&10u32.to_le_bytes());
            p.extend_from_slice(&123456i32.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes());
            p.push(1);
            p
        };
        stream
            .write_all(&write_packet(HEADER_CZ_ENTER_2019, &payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let _accept = read_response(&mut stream).await;

        // 默认 2 NPC + 2 怪物，加上从数据库注入的 1 个怪物，共 5 个刷新包
        for _ in 0..5 {
            let packet = read_response(&mut stream).await;
            let id = u16::from_le_bytes([packet[0], packet[1]]);
            assert!(
                id == HEADER_ZC_NOTIFY_STANDENTRY || id == HEADER_ZC_NOTIFY_SPAWNENTRY,
                "应收到实体刷新包"
            );
        }
    }

    #[tokio::test]
    async fn map_2019_enter_and_move_and_attack() {
        let (addr, _server) = start_test_server().await;
        let mut stream = TcpStream::connect(addr).await.unwrap();

        let payload = {
            let mut p = Vec::new();
            p.extend_from_slice(&1u32.to_le_bytes());
            p.extend_from_slice(&10u32.to_le_bytes());
            p.extend_from_slice(&123456i32.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes());
            p.push(1);
            p
        };
        stream
            .write_all(&write_packet(HEADER_CZ_ENTER_2019, &payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let _accept = read_response(&mut stream).await;
        for _ in 0..4 {
            read_response(&mut stream).await;
        }

        // 请求移动
        let move_payload = morroc_packets::map::encode_pos_dir(160, 190, 0);
        stream
            .write_all(&write_packet(HEADER_CZ_REQUEST_MOVE, &move_payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let move_response = read_response(&mut stream).await;
        let move_id = u16::from_le_bytes([move_response[0], move_response[1]]);
        assert!(move_id == HEADER_ZC_NOTIFY_PLAYERMOVE || move_id == HEADER_ZC_NOTIFY_MOVE);

        // 请求攻击怪物 2000
        let mut act_payload = Vec::new();
        act_payload.extend_from_slice(&2000u32.to_le_bytes());
        act_payload.push(0); // action
        stream
            .write_all(&write_packet(HEADER_CZ_REQUEST_ACT, &act_payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();

        let act_response = read_response(&mut stream).await;
        let act_id = u16::from_le_bytes([act_response[0], act_response[1]]);
        assert_eq!(act_id, HEADER_ZC_NOTIFY_ACT);
    }

    #[tokio::test]
    async fn map_1000_bots_concurrent_latency() {
        use futures::future::join_all;
        use std::time::Instant;

        let (addr, server) = start_test_server().await;
        let bot_count = 1000;

        for i in 1..=bot_count {
            server.register_session(i as u32, 123456).await;
        }

        let start = Instant::now();
        let handles: Vec<_> = (1..=bot_count)
            .map(|i| {
                let account_id = i as u32;
                tokio::spawn(async move {
                    let mut stream = TcpStream::connect(addr).await.unwrap();

                    let payload = {
                        let mut p = Vec::new();
                        p.extend_from_slice(&account_id.to_le_bytes());
                        p.extend_from_slice(&10u32.to_le_bytes());
                        p.extend_from_slice(&123456i32.to_le_bytes());
                        p.extend_from_slice(&0u32.to_le_bytes());
                        p.push(1);
                        p
                    };
                    stream
                        .write_all(&write_packet(HEADER_CZ_ENTER_2019, &payload))
                        .await
                        .unwrap();
                    stream.flush().await.unwrap();

                    // 至少完成进入请求和进入确认，确保服务器已处理该会话。
                    let _accept = read_response(&mut stream).await;

                    // 施放技能：Bash Lv.1，目标默认 NPC 1000（确保持续触发广播）
                    let mut skill_payload = Vec::new();
                    skill_payload.extend_from_slice(&1u16.to_le_bytes()); // skill_id
                    skill_payload.extend_from_slice(&1u16.to_le_bytes()); // skill_lv
                    skill_payload.extend_from_slice(&1000u32.to_le_bytes()); // target_id
                    stream
                        .write_all(&write_packet(HEADER_CZ_USE_SKILL, &skill_payload))
                        .await
                        .unwrap();
                    stream.flush().await.unwrap();

                    let _ = stream.shutdown().await;
                    true
                })
            })
            .collect();

        let results: Vec<bool> = join_all(handles)
            .await
            .into_iter()
            .map(|res| res.unwrap())
            .collect();
        let total_time = start.elapsed();

        assert_eq!(results.iter().filter(|&&v| v).count(), bot_count);
        assert!(
            total_time.as_secs() < 10,
            "{} bots 并发进入并施放技能总耗时 {:?}，应小于 10s",
            bot_count,
            total_time
        );
    }

    #[tokio::test]
    async fn map_single_bot_latency_under_5ms() {
        use std::time::Instant;

        let (addr, server) = start_test_server().await;
        server.register_session(1, 123456).await;

        let t0 = Instant::now();
        let mut stream = TcpStream::connect(addr).await.unwrap();

        let payload = {
            let mut p = Vec::new();
            p.extend_from_slice(&1u32.to_le_bytes());
            p.extend_from_slice(&10u32.to_le_bytes());
            p.extend_from_slice(&123456i32.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes());
            p.push(1);
            p
        };
        stream
            .write_all(&write_packet(HEADER_CZ_ENTER_2019, &payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let _accept = read_response(&mut stream).await;

        for _ in 0..4 {
            read_response(&mut stream).await;
        }

        let move_payload = morroc_packets::map::encode_pos_dir(160, 190, 0);
        stream
            .write_all(&write_packet(HEADER_CZ_REQUEST_MOVE, &move_payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let _move = read_response(&mut stream).await;

        let mut act_payload = Vec::new();
        act_payload.extend_from_slice(&2000u32.to_le_bytes());
        act_payload.push(0);
        stream
            .write_all(&write_packet(HEADER_CZ_REQUEST_ACT, &act_payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let _act = read_response(&mut stream).await;

        let elapsed = t0.elapsed();
        assert!(
            elapsed.as_millis() < 5,
            "单 bot 端到端延迟 {:?}，应小于 5ms",
            elapsed
        );
    }

    #[test]
    fn woe_emperium_conquest_transfers_zone_owner() {
        let game_data = GameData {
            items: HashMap::new(),
            mobs: HashMap::new(),
            skills: SkillDatabase::from_skills(vec![SkillInfo {
                id: 999,
                name: "GM_Destroy".to_string(),
                max_level: 1,
                element: "Neutral".to_string(),
                attack_type: "Physical".to_string(),
                damage_factor: 1000.0,
                ..Default::default()
            }]),
            npcs: Vec::new(),
            statuses: StatusDatabase::default(),
        };
        let map = MapInstance::new("prontera", game_data, false, true);

        let (tx, _rx) = mpsc::unbounded_channel();
        let player = Player {
            account_id: 1,
            char_id: 10,
            entity: Entity {
                id: 1,
                aid: 1,
                gid: 10,
                object_type: 0,
                name: name_bytes("Conqueror"),
                job: 0,
                head: 1,
                speed: DEFAULT_SPEED,
                x: DEFAULT_SPAWN_X,
                y: DEFAULT_SPAWN_Y,
                dir: DEFAULT_DIR,
                hp: 100,
                max_hp: 100,
                level: 10,
                sex: 1,
                body: 0,
                str: 10,
                dex: 10,
                vit: 10,
                int: 1,
                def: 0,
                weapon_atk: 20,
                matk: 0,
            },
            move_state: None,
            sender: tx,
        };

        map.player_enter(player);
        map.create_guild(1, "Conquerors", 1);
        map.set_player_guild(1, 1);

        assert_eq!(map.emperium_hp(), Some(1000));

        let packets = map.player_action(1, EMPERIUM_NPC_ID, 0, 0, Some((999, 1)));
        assert!(packets.is_some(), "攻击 Emperium 应返回动作包");

        assert_eq!(map.emperium_hp(), None, "Emperium 应被摧毁");
        assert_eq!(map.zone_owner(1), Some(1), "区域所有者应转为征服者公会");
    }

    #[test]
    fn map_spawns_converted_npcs_for_current_map() {
        let json = r#"{
            "items": [],
            "mobs": [],
            "skills": [],
            "npcs": [
                {"map":"prontera","x":150,"y":180,"facing":0,"kind":{"kind":"Script"},"name":"Script NPC","sprite":"4_M_KAFRA","body":null},
                {"map":"other","x":10,"y":20,"facing":0,"kind":{"kind":"Script"},"name":"Other NPC","sprite":"4_M_KAFRA","body":null}
            ]
        }"#;
        let game_data = GameData::load_from_str(json).unwrap();
        let map = MapInstance::new("prontera", game_data, false, false);

        let (tx, _rx) = mpsc::unbounded_channel();
        let player = Player {
            account_id: 1,
            char_id: 10,
            entity: Entity {
                id: 1,
                aid: 1,
                gid: 10,
                object_type: 0,
                name: name_bytes("Player"),
                job: 0,
                head: 1,
                speed: DEFAULT_SPEED,
                x: DEFAULT_SPAWN_X,
                y: DEFAULT_SPAWN_Y,
                dir: DEFAULT_DIR,
                hp: 100,
                max_hp: 100,
                level: 1,
                sex: 1,
                body: 0,
                str: 1,
                dex: 1,
                vit: 1,
                int: 1,
                def: 0,
                weapon_atk: 1,
                matk: 0,
            },
            move_state: None,
            sender: tx,
        };

        let (to_self, _) = map.player_enter(player);
        let npc_packets = to_self
            .iter()
            .filter(|p| p.packet_id == HEADER_ZC_NOTIFY_STANDENTRY)
            .count();
        // 默认 2 个 NPC + 1 个 prontera 转换 NPC。
        assert_eq!(npc_packets, 3, "应包含默认 NPC 与当前地图的转换 NPC");
    }

    #[tokio::test]
    async fn map_stun_blocks_player_movement() {
        let statuses_json = r#"[
            {"id": 2, "name": "Stun", "kind": "debuff", "duration_ms": 1000, "modifier": {"blocks_movement": true}}
        ]"#;
        let game_data = GameData {
            items: HashMap::new(),
            mobs: HashMap::new(),
            skills: SkillDatabase::default(),
            npcs: Vec::new(),
            statuses: StatusDatabase::load_from_str(statuses_json).unwrap(),
        };
        let server = MapServer::new(
            "127.0.0.1:0".parse().unwrap(),
            Arc::new(morroc_db::LocalSessionStore::new()),
            game_data,
            false,
            false,
        );
        server.register_session(1, 123456).await;
        let (tx, rx) = oneshot::channel();
        tokio::spawn({
            let server = server.clone();
            async move {
                let _ = server.run(Some(tx)).await;
            }
        });
        let addr = rx.await.expect("服务器应发送绑定地址");

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let payload = {
            let mut p = Vec::new();
            p.extend_from_slice(&1u32.to_le_bytes());
            p.extend_from_slice(&10u32.to_le_bytes());
            p.extend_from_slice(&123456i32.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes());
            p.push(1);
            p
        };
        stream
            .write_all(&write_packet(HEADER_CZ_ENTER_2019, &payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let _accept = read_response(&mut stream).await;
        for _ in 0..4 {
            let _ = read_response(&mut stream).await;
        }

        let tick = system_tick();
        assert!(server.map.apply_status(1, 2, tick), "应能施加 Stun 状态");
        assert!(
            server.map.player_move(1, 160, 190, 0, tick).is_none(),
            "眩晕时应禁止移动"
        );
        assert!(server.map.remove_status(1, 2));
        assert!(
            server.map.player_move(1, 160, 190, 0, tick).is_some(),
            "移除眩晕后应允许移动"
        );
    }

    #[tokio::test]
    async fn map_status_buff_increases_combat_damage() {
        let statuses_json = r#"[
            {"id": 3, "name": "PowerUp", "kind": "buff", "duration_ms": 1000, "modifier": {"str": 100, "atk": 50}}
        ]"#;
        let game_data = GameData {
            items: HashMap::new(),
            mobs: HashMap::new(),
            skills: SkillDatabase::from_skills(vec![SkillInfo {
                id: 1,
                name: "Bash".to_string(),
                max_level: 1,
                element: "Neutral".to_string(),
                attack_type: "Weapon".to_string(),
                damage_factor: 1.0,
                status_id: Some(3),
                status_duration_ms: 1000,
                status_chance: 1.0,
            }]),
            npcs: Vec::new(),
            statuses: StatusDatabase::load_from_str(statuses_json).unwrap(),
        };
        let server = MapServer::new(
            "127.0.0.1:0".parse().unwrap(),
            Arc::new(morroc_db::LocalSessionStore::new()),
            game_data,
            false,
            false,
        );
        server.register_session(1, 123456).await;
        let (tx, rx) = oneshot::channel();
        tokio::spawn({
            let server = server.clone();
            async move {
                let _ = server.run(Some(tx)).await;
            }
        });
        let addr = rx.await.expect("服务器应发送绑定地址");

        let mut stream = TcpStream::connect(addr).await.unwrap();
        let payload = {
            let mut p = Vec::new();
            p.extend_from_slice(&1u32.to_le_bytes());
            p.extend_from_slice(&10u32.to_le_bytes());
            p.extend_from_slice(&123456i32.to_le_bytes());
            p.extend_from_slice(&0u32.to_le_bytes());
            p.push(1);
            p
        };
        stream
            .write_all(&write_packet(HEADER_CZ_ENTER_2019, &payload))
            .await
            .unwrap();
        stream.flush().await.unwrap();
        let _accept = read_response(&mut stream).await;
        for _ in 0..4 {
            let _ = read_response(&mut stream).await;
        }

        // 确保测试怪物能承受两次攻击，避免第一次死亡导致后续读包错位。
        server.map.update_monster(2000, |m| {
            m.entity.hp = 1_000_000;
            m.entity.max_hp = 1_000_000;
        });

        fn extract_damage(packets: &[Packet]) -> i32 {
            for p in packets {
                if p.packet_id == HEADER_ZC_NOTIFY_ACT {
                    let mut full = Vec::new();
                    full.extend_from_slice(&p.packet_id.to_le_bytes());
                    full.extend_from_slice(&p.payload);
                    return parse_damage_from_act_packet(&full);
                }
            }
            panic!("攻击结果中应包含 ZcNotifyAct");
        }

        let tick = system_tick();
        let before = server
            .map
            .player_action(1, 2000, 0, tick, None)
            .expect("无 Buff 攻击应命中");
        let damage_before = extract_damage(&before);

        server.map.apply_status(1, 3, tick);

        let after = server
            .map
            .player_action(1, 2000, 0, tick, None)
            .expect("有 Buff 攻击应命中");
        let damage_after = extract_damage(&after);

        assert!(
            damage_after > damage_before,
            "PowerUp 应提升伤害: before={}, after={}",
            damage_before,
            damage_after
        );
    }

    fn parse_damage_from_act_packet(packet: &[u8]) -> i32 {
        // ZcNotifyAct 编码后结构：source_id(4), target_id(4), start_time(4), attack_mt(4), attacked_mt(4), damage(4), is_sp_damaged(1), count(2), action(1), left_damage(4)
        // 包体从第 2 字节开始（跳过 2 字节包头）。
        let offset = 2 + 4 + 4 + 4 + 4 + 4;
        i32::from_le_bytes([
            packet[offset],
            packet[offset + 1],
            packet[offset + 2],
            packet[offset + 3],
        ])
    }
}
