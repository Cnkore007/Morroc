//! 转换后的游戏数据库加载。
//!
//! 从 `morroc-converter` 生成的 `database.json` 中加载 item/mob/skill 数据，
//! 注入到地图服务器中。

use crate::combat::{SkillDatabase, SkillInfo};
use crate::status::{StatusDatabase, StatusEffect};
use morroc_converter::npc::Npc as ConvertedNpc;
use morroc_converter::schema::GameDatabase;
use std::collections::HashMap;
use std::path::Path;
use tracing::warn;

pub use morroc_converter::schema::{Item, Mob, Skill};

/// 地图服务器使用的运行时游戏数据库。
#[derive(Debug, Clone, Default)]
pub struct GameData {
    pub items: HashMap<i64, Item>,
    pub mobs: HashMap<i64, Mob>,
    pub skills: SkillDatabase,
    pub npcs: Vec<ConvertedNpc>,
    pub statuses: StatusDatabase,
}

impl GameData {
    /// 从 JSON 文件加载。
    pub fn load_from_json(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            warn!("游戏数据库 {} 不存在，使用空数据库", path.display());
            return Ok(Self::default());
        }
        let source = std::fs::read_to_string(path)?;
        Self::load_from_str(&source)
    }

    /// 从 JSON 字符串加载。
    pub fn load_from_str(source: &str) -> anyhow::Result<Self> {
        let db: GameDatabase = serde_json::from_str(source)?;
        let items = db.items.into_iter().map(|item| (item.id, item)).collect();
        let mobs = db.mobs.into_iter().map(|mob| (mob.id, mob)).collect();
        let skills = SkillDatabase::from_skills(
            db.skills
                .into_iter()
                .map(|s| SkillInfo {
                    id: s.id as u16,
                    name: s.name,
                    max_level: s.max_level.unwrap_or(1) as i16,
                    element: s.element.unwrap_or_default(),
                    attack_type: s.attack_type.unwrap_or_default(),
                    damage_factor: 1.0,
                    status_id: None,
                    status_duration_ms: 0,
                    status_chance: 1.0,
                })
                .collect(),
        );
        let statuses: Vec<StatusEffect> = db
            .statuses
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();
        Ok(Self {
            items,
            mobs,
            skills,
            npcs: db.npcs,
            statuses: StatusDatabase::from_statuses(statuses),
        })
    }

    /// NPC 数量。
    pub fn npc_count(&self) -> usize {
        self.npcs.len()
    }

    /// 道具数量。
    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// 怪物数量。
    pub fn mob_count(&self) -> usize {
        self.mobs.len()
    }

    /// 查询道具。
    pub fn get_item(&self, id: i64) -> Option<&Item> {
        self.items.get(&id)
    }

    /// 插入/覆盖道具，返回旧值。
    pub fn insert_item(&mut self, item: Item) -> Option<Item> {
        self.items.insert(item.id, item)
    }

    /// 移除道具。
    pub fn remove_item(&mut self, id: i64) -> Option<Item> {
        self.items.remove(&id)
    }

    /// 修改道具字段。
    pub fn update_item<F>(&mut self, id: i64, f: F) -> bool
    where
        F: FnOnce(&mut Item),
    {
        if let Some(item) = self.items.get_mut(&id) {
            f(item);
            true
        } else {
            false
        }
    }

    /// 查询怪物。
    pub fn get_mob(&self, id: i64) -> Option<&Mob> {
        self.mobs.get(&id)
    }

    /// 插入/覆盖怪物，返回旧值。
    pub fn insert_mob(&mut self, mob: Mob) -> Option<Mob> {
        self.mobs.insert(mob.id, mob)
    }

    /// 移除怪物。
    pub fn remove_mob(&mut self, id: i64) -> Option<Mob> {
        self.mobs.remove(&id)
    }

    /// 查询技能。
    pub fn get_skill(&self, id: u16) -> Option<&SkillInfo> {
        self.skills.get(id)
    }

    /// 插入/覆盖技能。
    pub fn insert_skill(&mut self, skill: SkillInfo) -> Option<SkillInfo> {
        self.skills.insert(skill)
    }

    /// 移除技能。
    pub fn remove_skill(&mut self, id: u16) -> Option<SkillInfo> {
        self.skills.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_data_loads_from_json() {
        let json = r#"{
            "items": [
                {"id": 501, "aegis_name": "Red_Potion", "name": "Red Potion"}
            ],
            "mobs": [
                {"id": 1002, "sprite_name": "PORING", "name": "Poring", "hp": 50, "level": 1}
            ],
            "skills": [
                {"id": 5, "name": "SM_BASH", "max_level": 10, "attack_type": "Weapon"}
            ]
        }"#;
        let data = GameData::load_from_str(json).unwrap();
        assert_eq!(data.item_count(), 1);
        assert_eq!(data.mob_count(), 1);
        assert!(data.skills.get(5).is_some());
    }

    #[test]
    fn game_data_loads_npcs_from_json() {
        let json = r#"{
            "items": [],
            "mobs": [],
            "skills": [],
            "npcs": [
                {"map":"prontera","x":150,"y":180,"facing":0,"kind":{"kind":"Script"},"name":"Test NPC","sprite":"4_M_KAFRA","body":null}
            ]
        }"#;
        let data = GameData::load_from_str(json).unwrap();
        assert_eq!(data.npc_count(), 1);
        let npc = &data.npcs[0];
        assert_eq!(npc.name, "Test NPC");
        assert_eq!(npc.x, 150);
        assert_eq!(npc.y, 180);
    }

    #[test]
    fn game_data_loads_from_converted_file() {
        let manifest = std::env!("CARGO_MANIFEST_DIR");
        let path = Path::new(manifest).join("../../data/database.json");
        let data = GameData::load_from_json(path).unwrap();
        assert!(data.item_count() > 0, "应加载转换后的道具数据");
        assert!(data.mob_count() > 0, "应加载转换后的怪物数据");
        assert!(data.skills.get(5).is_some(), "应加载转换后的技能数据");
    }

    #[test]
    fn game_data_loads_from_rathena_yaml() {
        use morroc_converter::rathena;
        let dir = tempfile::tempdir().unwrap();

        std::fs::write(
            dir.path().join("item_db.yml"),
            r#"
Body:
  - Id: 502
    AegisName: Orange_Potion
    Name: Orange Potion
    Type: Healing
    Buy: 100
    Weight: 70
"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join("mob_db.yml"),
            r#"
Body:
  - Id: 1002
    Sprite: PORING
    Name: Poring
    Level: 1
    Hp: 50
    Sp: 1
    Attack: [5, 2]
    Size: Medium
    Race: Plant
    Element: Water
    Drops:
      - Item: Red_Potion
        Rate: 100
"#,
        )
        .unwrap();

        std::fs::write(
            dir.path().join("skill_db.yml"),
            r#"
Body:
  - Id: 5
    Name: SM_BASH
    Description: Bash
    MaxLevel: 10
    Range: -1
    Hit: BDT_SKILL
    AttackType: Weapon
    Element: Neutral
"#,
        )
        .unwrap();

        let db = rathena::convert_database_dir(dir.path()).unwrap();
        let json = serde_json::to_string(&db).unwrap();
        let data = GameData::load_from_str(&json).unwrap();

        assert_eq!(data.item_count(), 1, "应从 rAthena YAML 加载 1 个道具");
        assert_eq!(data.mob_count(), 1, "应从 rAthena YAML 加载 1 个怪物");
        assert!(data.skills.get(5).is_some(), "应从 rAthena YAML 加载 Bash 技能");
        assert_eq!(data.get_item(502).unwrap().name, "Orange Potion");
    }
}
