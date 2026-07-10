//! 状态效果（Status Effect）系统。
//!
//! 提供 BUFF/DEBUFF 的定义、加载、持续计时、属性修正与战斗限制。
//! 当前支持：属性修正、周期性 HP 变化、禁止移动/攻击。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::warn;

/// 状态类型。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    /// 增益。
    Buff,
    /// 减益。
    Debuff,
    /// 其他。
    #[default]
    Neutral,
}

/// 状态对战斗属性的修正。
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct StatusModifier {
    #[serde(default)]
    pub str: i16,
    #[serde(default)]
    pub agi: i16,
    #[serde(default)]
    pub vit: i16,
    #[serde(default)]
    pub int: i16,
    #[serde(default)]
    pub dex: i16,
    #[serde(default)]
    pub luk: i16,
    #[serde(default)]
    pub atk: i16,
    #[serde(default)]
    pub def: i16,
    #[serde(default)]
    pub matk: i16,
    #[serde(default)]
    pub speed: i16,
    /// 每次 tick 的 HP 变化（正为恢复，负为伤害）。
    #[serde(default)]
    pub hp_per_tick: i32,
    /// tick 间隔（毫秒）。
    #[serde(default = "default_tick_interval")]
    pub tick_interval_ms: u32,
    /// 是否禁止移动。
    #[serde(default)]
    pub blocks_movement: bool,
    /// 是否禁止攻击。
    #[serde(default)]
    pub blocks_attack: bool,
}

fn default_tick_interval() -> u32 {
    1000
}

impl StatusModifier {
    /// 合并两个修正（字段相加）。
    pub fn merge(self, other: &StatusModifier) -> StatusModifier {
        StatusModifier {
            str: self.str.saturating_add(other.str),
            agi: self.agi.saturating_add(other.agi),
            vit: self.vit.saturating_add(other.vit),
            int: self.int.saturating_add(other.int),
            dex: self.dex.saturating_add(other.dex),
            luk: self.luk.saturating_add(other.luk),
            atk: self.atk.saturating_add(other.atk),
            def: self.def.saturating_add(other.def),
            matk: self.matk.saturating_add(other.matk),
            speed: self.speed.saturating_add(other.speed),
            hp_per_tick: self.hp_per_tick.saturating_add(other.hp_per_tick),
            tick_interval_ms: self.tick_interval_ms.max(other.tick_interval_ms),
            blocks_movement: self.blocks_movement || other.blocks_movement,
            blocks_attack: self.blocks_attack || other.blocks_attack,
        }
    }
}

/// 状态效果定义。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct StatusEffect {
    pub id: u16,
    pub name: String,
    #[serde(default)]
    pub kind: StatusKind,
    /// 持续时间（毫秒）。
    pub duration_ms: u32,
    #[serde(default)]
    pub modifier: StatusModifier,
}

/// 一个正在生效的状态实例。
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveStatus {
    pub effect: StatusEffect,
    pub expires_at: u32,
    pub next_tick: u32,
}

/// 实体上的状态集合。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusState {
    active: HashMap<u16, ActiveStatus>,
}

impl StatusState {
    /// 创建空状态集合。
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加/刷新一个状态。
    pub fn apply(&mut self, effect: StatusEffect, now: u32) {
        let expires = now.saturating_add(effect.duration_ms);
        self.active.insert(
            effect.id,
            ActiveStatus {
                effect,
                expires_at: expires,
                next_tick: now,
            },
        );
    }

    /// 移除指定状态。
    pub fn remove(&mut self, id: u16) -> bool {
        self.active.remove(&id).is_some()
    }

    /// 推进时间并返回本 tick 的 HP 变化总量。
    pub fn tick(&mut self, now: u32) -> i32 {
        let mut total_hp_change = 0i32;
        let mut expired = Vec::new();
        for (id, active) in &mut self.active {
            if now >= active.expires_at {
                expired.push(*id);
                continue;
            }
            if active.effect.modifier.hp_per_tick != 0 && now >= active.next_tick {
                total_hp_change =
                    total_hp_change.saturating_add(active.effect.modifier.hp_per_tick);
                active.next_tick = active
                    .next_tick
                    .saturating_add(active.effect.modifier.tick_interval_ms.max(1));
            }
        }
        for id in expired {
            self.active.remove(&id);
        }
        total_hp_change
    }

    /// 当前所有活跃状态的累计修正。
    pub fn modifier(&self) -> StatusModifier {
        self.active
            .values()
            .map(|a| a.effect.modifier)
            .fold(StatusModifier::default(), |acc, m| acc.merge(&m))
    }

    /// 是否禁止移动。
    pub fn blocks_movement(&self) -> bool {
        self.modifier().blocks_movement
    }

    /// 是否禁止攻击。
    pub fn blocks_attack(&self) -> bool {
        self.modifier().blocks_attack
    }

    /// 当前活跃状态数量。
    pub fn len(&self) -> usize {
        self.active.len()
    }

    /// 是否没有任何状态。
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
}

/// 状态效果数据库。
#[derive(Debug, Clone, Default)]
pub struct StatusDatabase {
    statuses: HashMap<u16, StatusEffect>,
}

impl StatusDatabase {
    /// 从 JSON 文件加载。
    pub fn load_from_json(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            warn!("状态数据库 {} 不存在，使用空状态数据库", path.display());
            return Ok(Self::default());
        }
        let source = std::fs::read_to_string(path)?;
        Self::load_from_str(&source)
    }

    /// 从 JSON 字符串加载。
    pub fn load_from_str(source: &str) -> anyhow::Result<Self> {
        let statuses: Vec<StatusEffect> = serde_json::from_str(source)?;
        let mut map = HashMap::new();
        for status in statuses {
            map.insert(status.id, status);
        }
        Ok(Self { statuses: map })
    }

    /// 从状态列表构造。
    pub fn from_statuses(statuses: Vec<StatusEffect>) -> Self {
        let mut map = HashMap::new();
        for status in statuses {
            map.insert(status.id, status);
        }
        Self { statuses: map }
    }

    /// 查询状态定义。
    pub fn get(&self, id: u16) -> Option<&StatusEffect> {
        self.statuses.get(&id)
    }

    /// 插入/覆盖状态定义。
    pub fn insert(&mut self, status: StatusEffect) -> Option<StatusEffect> {
        self.statuses.insert(status.id, status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_database_loads_from_json() {
        let json = r#"[
            {"id": 1, "name": "Poison", "kind": "debuff", "duration_ms": 5000, "modifier": {"hp_per_tick": -10, "tick_interval_ms": 1000}},
            {"id": 2, "name": "Blessing", "kind": "buff", "duration_ms": 30000, "modifier": {"str": 5, "dex": 5}}
        ]"#;
        let db = StatusDatabase::load_from_str(json).unwrap();
        assert_eq!(db.get(1).unwrap().name, "Poison");
        assert_eq!(db.get(2).unwrap().modifier.str, 5);
    }

    #[test]
    fn poison_deals_periodic_damage() {
        let effect = StatusEffect {
            id: 1,
            name: "Poison".to_string(),
            kind: StatusKind::Debuff,
            duration_ms: 5000,
            modifier: StatusModifier {
                hp_per_tick: -10,
                tick_interval_ms: 1000,
                ..Default::default()
            },
        };
        let mut state = StatusState::new();
        state.apply(effect, 0);
        assert_eq!(state.tick(0), -10);
        assert_eq!(state.tick(500), 0); // 未到间隔
        assert_eq!(state.tick(1000), -10);
        assert_eq!(state.tick(5000), 0); // 已过期，tick 返回 0
        assert!(state.is_empty());
    }

    #[test]
    fn modifiers_stack() {
        let mut state = StatusState::new();
        state.apply(
            StatusEffect {
                id: 1,
                name: "Buff1".to_string(),
                kind: StatusKind::Buff,
                duration_ms: 1000,
                modifier: StatusModifier {
                    str: 5,
                    atk: 10,
                    ..Default::default()
                },
            },
            0,
        );
        state.apply(
            StatusEffect {
                id: 2,
                name: "Buff2".to_string(),
                kind: StatusKind::Buff,
                duration_ms: 1000,
                modifier: StatusModifier {
                    str: 3,
                    def: 5,
                    ..Default::default()
                },
            },
            0,
        );
        let m = state.modifier();
        assert_eq!(m.str, 8);
        assert_eq!(m.atk, 10);
        assert_eq!(m.def, 5);
    }

    #[test]
    fn movement_and_attack_block() {
        let mut state = StatusState::new();
        state.apply(
            StatusEffect {
                id: 1,
                name: "Stun".to_string(),
                kind: StatusKind::Debuff,
                duration_ms: 1000,
                modifier: StatusModifier {
                    blocks_movement: true,
                    blocks_attack: true,
                    ..Default::default()
                },
            },
            0,
        );
        assert!(state.blocks_movement());
        assert!(state.blocks_attack());
    }
}
