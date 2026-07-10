//! 最小 WoE / 公会系统。
//!
//! 提供公会、WoE 区域和攻城器（Emperium）状态管理。玩家攻击 Emperium
//! 并将其 HP 归零后，对应 WoE 区域的所有权会转移到该玩家所属公会。

use std::collections::HashMap;

/// 公会。
#[derive(Debug, Clone)]
pub struct Guild {
    pub id: u32,
    pub name: String,
    pub master_account_id: u32,
    pub member_account_ids: Vec<u32>,
}

impl Guild {
    /// 判断某 account_id 是否为本公会成员。
    pub fn is_member(&self, account_id: u32) -> bool {
        self.master_account_id == account_id || self.member_account_ids.contains(&account_id)
    }
}

/// WoE 攻城区域。
#[derive(Debug, Clone)]
pub struct WoEZone {
    pub id: u32,
    pub name: String,
    pub min_x: i16,
    pub min_y: i16,
    pub max_x: i16,
    pub max_y: i16,
    pub owner_guild_id: Option<u32>,
    pub emperium_entity_id: u32,
}

impl WoEZone {
    /// 判断坐标是否在区域内。
    pub fn contains(&self, x: i16, y: i16) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}

/// 公会 / WoE 运行时状态。
#[derive(Debug, Clone, Default)]
pub struct GuildState {
    pub guilds: HashMap<u32, Guild>,
    pub zones: HashMap<u32, WoEZone>,
    pub emperium_hp: HashMap<u32, i32>, // entity_id -> 当前 HP
}

impl GuildState {
    /// 创建公会。
    pub fn create_guild(
        &mut self,
        id: u32,
        name: impl Into<String>,
        master_account_id: u32,
    ) {
        self.guilds.insert(
            id,
            Guild {
                id,
                name: name.into(),
                master_account_id,
                member_account_ids: vec![master_account_id],
            },
        );
    }

    /// 注册 WoE 区域。
    pub fn register_zone(
        &mut self,
        id: u32,
        name: impl Into<String>,
        min_x: i16,
        min_y: i16,
        max_x: i16,
        max_y: i16,
        emperium_entity_id: u32,
        emperium_max_hp: i32,
    ) {
        self.zones.insert(
            id,
            WoEZone {
                id,
                name: name.into(),
                min_x,
                min_y,
                max_x,
                max_y,
                owner_guild_id: None,
                emperium_entity_id,
            },
        );
        self.emperium_hp.insert(emperium_entity_id, emperium_max_hp);
    }

    /// 设置区域所有者。
    pub fn set_zone_owner(&mut self, zone_id: u32, guild_id: Option<u32>) -> Option<()> {
        self.zones.get_mut(&zone_id).map(|zone| {
            zone.owner_guild_id = guild_id;
        })
    }

    /// 查询区域所有者。
    pub fn zone_owner(&self, zone_id: u32) -> Option<u32> {
        self.zones.get(&zone_id).and_then(|z| z.owner_guild_id)
    }

    /// 根据攻城器实体 ID 查找所属区域。
    pub fn find_zone_by_emperium(&self, emperium_id: u32,
    ) -> Option<&WoEZone> {
        self.zones
            .values()
            .find(|z| z.emperium_entity_id == emperium_id)
    }

    /// 对攻城器造成伤害。
    ///
    /// 返回 `(hp_after, zone_owner_changed_to)`。
    pub fn damage_emperium(
        &mut self,
        emperium_id: u32,
        damage: i32,
        attacker_guild_id: Option<u32>,
    ) -> (i32, Option<u32>) {
        let hp = self
            .emperium_hp
            .get_mut(&emperium_id)
            .map(|hp| {
                *hp -= damage;
                *hp
            })
            .unwrap_or(0);

        if hp <= 0 {
            if let Some(zone_id) = self
                .find_zone_by_emperium(emperium_id)
                .map(|z| z.id)
            {
                if let Some(guild_id) = attacker_guild_id {
                    self.set_zone_owner(zone_id, Some(guild_id));
                    return (0, Some(guild_id));
                }
            }
        }
        (hp.max(0), None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emperium_damage_changes_zone_owner() {
        let mut state = GuildState::default();
        state.create_guild(1, "TestGuild", 100);
        state.register_zone(10, "prtg_cas01", 140, 170, 170, 190, 5000, 100);

        let (hp, owner) = state.damage_emperium(5000, 30, Some(1));
        assert_eq!(hp, 70);
        assert!(owner.is_none());

        let (hp, owner) = state.damage_emperium(5000, 80, Some(1));
        assert_eq!(hp, 0);
        assert_eq!(owner, Some(1));
        assert_eq!(state.zone_owner(10), Some(1));
    }

    #[test]
    fn zone_contains_position() {
        let mut state = GuildState::default();
        state.register_zone(10, "prtg_cas01", 140, 170, 160, 180, 5000, 100);
        let zone = state.zones.get(&10).unwrap();
        assert!(zone.contains(150, 175));
        assert!(!zone.contains(10, 10));
    }
}
