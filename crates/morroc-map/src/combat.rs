//! 最小战斗/技能系统。
//!
//! 提供基于属性/等级的伤害公式和技能数据库加载。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::warn;

/// 技能定义。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillInfo {
    pub id: u16,
    pub name: String,
    #[serde(default)]
    pub max_level: i16,
    #[serde(default)]
    pub element: String,
    #[serde(default)]
    pub attack_type: String,
    #[serde(default = "default_damage_factor")]
    pub damage_factor: f64,
    /// 命中目标时附加的状态 ID。
    #[serde(default)]
    pub status_id: Option<u16>,
    /// 状态持续时间（毫秒）。
    #[serde(default)]
    pub status_duration_ms: u32,
    /// 状态触发概率（0.0~1.0）。未实现概率时为 1.0。
    #[serde(default = "default_status_chance")]
    pub status_chance: f64,
}

fn default_status_chance() -> f64 {
    1.0
}

fn default_damage_factor() -> f64 {
    1.0
}

/// 技能数据库。
#[derive(Debug, Clone, Default)]
pub struct SkillDatabase {
    skills: HashMap<u16, SkillInfo>,
}

impl SkillDatabase {
    /// 从 JSON 文件加载技能数据库。
    ///
    /// JSON 格式为技能对象的数组，例如：
    /// ```json
    /// [
    ///   { "id": 1, "name": "Bash", "max_level": 10, "element": "Neutral", "attack_type": "Physical", "damage_factor": 1.2 }
    /// ]
    /// ```
    pub fn load_from_json(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        if !path.exists() {
            warn!("技能数据库 {} 不存在，使用空技能数据库", path.display());
            return Ok(Self::default());
        }
        let source = std::fs::read_to_string(path)?;
        Self::load_from_str(&source)
    }

    /// 从 JSON 字符串加载。
    pub fn load_from_str(source: &str) -> anyhow::Result<Self> {
        let skills: Vec<SkillInfo> = serde_json::from_str(source)?;
        let mut map = HashMap::new();
        for skill in skills {
            map.insert(skill.id, skill);
        }
        Ok(Self { skills: map })
    }

    /// 从技能列表构造。
    pub fn from_skills(skills: Vec<SkillInfo>) -> Self {
        let mut map = HashMap::new();
        for skill in skills {
            map.insert(skill.id, skill);
        }
        Self { skills: map }
    }

    /// 查询技能。
    pub fn get(&self, id: u16) -> Option<&SkillInfo> {
        self.skills.get(&id)
    }

    /// 插入/覆盖技能，返回旧值。
    pub fn insert(&mut self, skill: SkillInfo) -> Option<SkillInfo> {
        self.skills.insert(skill.id, skill)
    }

    /// 移除技能。
    pub fn remove(&mut self, id: u16) -> Option<SkillInfo> {
        self.skills.remove(&id)
    }
}

/// 战斗属性集合。
#[derive(Debug, Clone, Default)]
pub struct CombatStats {
    pub level: i16,
    pub str: i16,
    pub dex: i16,
    pub vit: i16,
    pub int: i16,
    pub def: i16,
    pub weapon_atk: i16,
    pub matk: i16,
}

/// 基于属性/等级计算伤害。
///
/// 普通攻击 / 物理技能：伤害 = (weapon_atk + str/5 + dex/10) * (1 + 0.05*level)
/// 魔法技能：伤害 = (matk + int/5 + dex/10) * (1 + 0.05*level)
/// 技能攻击在上述基础上乘以技能系数 damage_factor * (1 + 0.1*skill_level)
/// 目标防御/体质按减伤公式削减。
pub fn compute_damage(
    source: &CombatStats,
    target: &CombatStats,
    skill: Option<&SkillInfo>,
    skill_level: i16,
) -> i32 {
    let is_magical = skill
        .map(|s| s.attack_type.eq_ignore_ascii_case("Magical"))
        .unwrap_or(false);

    let base = if is_magical {
        (source.matk as f64 + source.int as f64 / 5.0 + source.dex as f64 / 10.0)
            * (1.0 + 0.05 * source.level as f64)
    } else {
        (source.weapon_atk as f64 + source.str as f64 / 5.0 + source.dex as f64 / 10.0)
            * (1.0 + 0.05 * source.level as f64)
    };

    let skill_multiplier = if let Some(skill) = skill {
        let effective_level = skill_level.clamp(1, skill.max_level) as f64;
        skill.damage_factor * (1.0 + 0.1 * effective_level)
    } else {
        1.0
    };

    let raw = base * skill_multiplier;

    let def_reduction = (target.def as f64 / (target.def as f64 + 100.0)).min(0.99);
    let vit_reduction = (target.vit as f64 / (target.vit as f64 + 100.0)).min(0.99);

    let damage = raw * (1.0 - def_reduction) * (1.0 - vit_reduction);
    damage.max(1.0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_database_loads_from_json() {
        let json = r#"[
            {"id": 1, "name": "Bash", "max_level": 10, "element": "Neutral", "attack_type": "Physical", "damage_factor": 1.2},
            {"id": 5, "name": "Magnum Break", "max_level": 10, "element": "Fire", "attack_type": "Magical", "damage_factor": 1.5}
        ]"#;
        let db = SkillDatabase::load_from_str(json).unwrap();
        assert_eq!(db.get(1).unwrap().name, "Bash");
        assert_eq!(db.get(5).unwrap().element, "Fire");
        assert!(db.get(999).is_none());
    }

    #[test]
    fn skill_database_loads_from_json_file() {
        let manifest = std::env!("CARGO_MANIFEST_DIR");
        let path = std::path::Path::new(manifest).join("../../data/skill_db.json");
        let db = SkillDatabase::load_from_json(path).unwrap();
        assert!(db.get(5).is_some(), "应包含 Bash 技能");
        assert_eq!(db.get(5).unwrap().name, "SM_BASH");
    }

    #[test]
    fn damage_formula_changes_with_stats() {
        let weak = CombatStats {
            level: 1,
            str: 1,
            dex: 1,
            weapon_atk: 5,
            ..Default::default()
        };
        let strong = CombatStats {
            level: 10,
            str: 20,
            dex: 10,
            weapon_atk: 30,
            ..Default::default()
        };
        let target = CombatStats {
            vit: 5,
            def: 5,
            ..Default::default()
        };

        let weak_dmg = compute_damage(&weak, &target, None, 0);
        let strong_dmg = compute_damage(&strong, &target, None, 0);
        assert!(weak_dmg > 0);
        assert!(strong_dmg > weak_dmg, "高属性应造成更高伤害");
    }

    #[test]
    fn skill_multiplier_increases_damage() {
        let source = CombatStats {
            level: 10,
            str: 20,
            dex: 10,
            weapon_atk: 30,
            ..Default::default()
        };
        let target = CombatStats {
            vit: 5,
            def: 5,
            ..Default::default()
        };
        let skill = SkillInfo {
            id: 1,
            name: "Bash".to_string(),
            max_level: 10,
            damage_factor: 1.2,
            ..Default::default()
        };

        let normal = compute_damage(&source, &target, None, 0);
        let skill_dmg = compute_damage(&source, &target, Some(&skill), 10);
        assert!(skill_dmg > normal, "技能伤害应高于普通攻击");
    }

    #[test]
    fn defense_and_vit_reduce_damage() {
        let source = CombatStats {
            level: 10,
            str: 20,
            dex: 10,
            weapon_atk: 30,
            ..Default::default()
        };
        let soft = CombatStats::default();
        let hard = CombatStats {
            vit: 50,
            def: 50,
            ..Default::default()
        };

        let soft_dmg = compute_damage(&source, &soft, None, 0);
        let hard_dmg = compute_damage(&source, &hard, None, 0);
        assert!(hard_dmg < soft_dmg, "高防御/体质应减少伤害");
    }

    #[test]
    fn magical_skill_uses_matk_and_int() {
        let physical_source = CombatStats {
            level: 10,
            str: 20,
            dex: 10,
            int: 5,
            weapon_atk: 30,
            matk: 5,
            ..Default::default()
        };
        let magical_source = CombatStats {
            level: 10,
            str: 5,
            dex: 10,
            int: 50,
            weapon_atk: 5,
            matk: 80,
            ..Default::default()
        };
        let target = CombatStats {
            vit: 5,
            def: 5,
            ..Default::default()
        };

        let magical_skill = SkillInfo {
            id: 5,
            name: "Fire Bolt".to_string(),
            max_level: 10,
            element: "Fire".to_string(),
            attack_type: "Magical".to_string(),
            damage_factor: 1.0,
            ..Default::default()
        };

        let physical_normal = compute_damage(&physical_source, &target, None, 0);
        let physical_skill = compute_damage(&physical_source, &target, Some(&magical_skill), 10);
        let magical_skill_dmg = compute_damage(&magical_source, &target, Some(&magical_skill), 10);

        assert!(
            magical_skill_dmg > physical_skill,
            "高 matk/int 的法师使用魔法技能应造成更高伤害"
        );
        assert!(
            physical_normal > physical_skill,
            "物理角色在低 matk 时不应靠魔法技能打出比普攻更高的伤害"
        );
    }
}
