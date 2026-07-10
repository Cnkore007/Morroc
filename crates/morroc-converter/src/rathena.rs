//! rAthena YAML 数据库解析器。
//!
//! 支持 rAthena 的 `item_db.yml`、`mob_db.yml`、`skill_db.yml` 格式：
//! 顶层为 `Header` / `Body` 结构，其中 `Body` 是记录列表。
//! 字段命名与 Hercules libconfig 略有不同，这里做兼容映射并保留未识别字段到 `extra`。

use crate::schema::{GameDatabase, Item, Mob, Skill};
use serde_yaml::Value;
use std::collections::HashMap;

/// 解析 rAthena YAML 数据库目录，默认读取 `item_db.yml`、`mob_db.yml`、`skill_db.yml`。
pub fn convert_database_dir(db_dir: &std::path::Path) -> anyhow::Result<GameDatabase> {
    let mut db = GameDatabase::default();

    let item_path = db_dir.join("item_db.yml");
    if item_path.exists() {
        let source = std::fs::read_to_string(&item_path)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", item_path.display(), e))?;
        db.items = parse_items(&source)?;
    }

    let mob_path = db_dir.join("mob_db.yml");
    if mob_path.exists() {
        let source = std::fs::read_to_string(&mob_path)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", mob_path.display(), e))?;
        db.mobs = parse_mobs(&source)?;
    }

    let skill_path = db_dir.join("skill_db.yml");
    if skill_path.exists() {
        let source = std::fs::read_to_string(&skill_path)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", skill_path.display(), e))?;
        db.skills = parse_skills(&source)?;
    }

    Ok(db)
}

/// 解析道具 YAML。
pub fn parse_items(source: &str) -> anyhow::Result<Vec<Item>> {
    let root: Value = serde_yaml::from_str(source)?;
    let body = body_array(&root)?;
    Ok(body.iter().filter_map(item_from_value).collect())
}

/// 解析怪物 YAML。
pub fn parse_mobs(source: &str) -> anyhow::Result<Vec<Mob>> {
    let root: Value = serde_yaml::from_str(source)?;
    let body = body_array(&root)?;
    Ok(body.iter().filter_map(mob_from_value).collect())
}

/// 解析技能 YAML。
pub fn parse_skills(source: &str) -> anyhow::Result<Vec<Skill>> {
    let root: Value = serde_yaml::from_str(source)?;
    let body = body_array(&root)?;
    Ok(body.iter().filter_map(skill_from_value).collect())
}

fn body_array(root: &Value) -> anyhow::Result<&Vec<Value>> {
    root.get("Body")
        .and_then(Value::as_sequence)
        .or_else(|| root.as_sequence())
        .ok_or_else(|| anyhow::anyhow!("YAML 数据库缺少 Body 数组"))
}

fn item_from_value(v: &Value) -> Option<Item> {
    let m = v.as_mapping()?;
    Some(Item {
        id: get_i64(m, "Id")?,
        aegis_name: get_string(m, "AegisName")?,
        name: get_string(m, "Name")?,
        item_type: get_string_opt(m, "Type"),
        buy: get_i64_opt(m, "Buy"),
        sell: get_i64_opt(m, "Sell"),
        weight: get_i64_opt(m, "Weight"),
        atk: get_i64_opt(m, "Atk"),
        matk: get_i64_opt(m, "Matk"),
        def: get_i64_opt(m, "Def"),
        range: get_i64_opt(m, "Range"),
        slots: get_i64_opt(m, "Slots"),
        weapon_level: get_i64_opt(m, "WeaponLv").or_else(|| get_i64_opt(m, "WeaponLevel")),
        equip_level: get_i64_opt(m, "EquipLv").or_else(|| get_i64_opt(m, "EquipLevel")),
        refine: get_bool_opt(m, "Refine"),
        script: get_string_opt(m, "Script"),
        on_equip_script: get_string_opt(m, "OnEquipScript"),
        on_unequip_script: get_string_opt(m, "OnUnequipScript"),
        extra: convert_extra(m, item_known_keys()),
    })
}

fn mob_from_value(v: &Value) -> Option<Mob> {
    let m = v.as_mapping()?;
    let stats = m.get("Stats").and_then(Value::as_mapping);
    Some(Mob {
        id: get_i64(m, "Id")?,
        sprite_name: get_string(m, "Sprite")
            .or_else(|| get_string(m, "SpriteName"))?,
        name: get_string(m, "Name")?,
        level: get_i64_opt(m, "Lv").or_else(|| get_i64_opt(m, "Level")),
        hp: get_i64_opt(m, "Hp"),
        sp: get_i64_opt(m, "Sp"),
        exp: get_i64_opt(m, "Exp"),
        job_exp: get_i64_opt(m, "JExp"),
        attack_range: get_i64_opt(m, "AttackRange"),
        attack_min: get_attack_min(m),
        attack_max: get_attack_max(m),
        def: get_i64_opt(m, "Def"),
        mdef: get_i64_opt(m, "Mdef"),
        str: get_stat(m, stats, "Str"),
        agi: get_stat(m, stats, "Agi"),
        vit: get_stat(m, stats, "Vit"),
        int: get_stat(m, stats, "Int"),
        dex: get_stat(m, stats, "Dex"),
        luk: get_stat(m, stats, "Luk"),
        view_range: get_i64_opt(m, "ViewRange"),
        chase_range: get_i64_opt(m, "ChaseRange"),
        size: get_string_opt(m, "Size"),
        race: get_string_opt(m, "Race"),
        element: get_element(m),
        move_speed: get_i64_opt(m, "MoveSpeed"),
        attack_delay: get_i64_opt(m, "AttackDelay"),
        attack_motion: get_i64_opt(m, "AttackMotion"),
        damage_motion: get_i64_opt(m, "DamageMotion"),
        drops: parse_drops(m),
        extra: convert_extra(m, mob_known_keys()),
    })
}

fn skill_from_value(v: &Value) -> Option<Skill> {
    let m = v.as_mapping()?;
    Some(Skill {
        id: get_i64(m, "Id")?,
        name: get_string(m, "Name")?,
        description: get_string_opt(m, "Description"),
        max_level: get_i64_opt(m, "MaxLevel"),
        range: get_i64_opt(m, "Range"),
        hit: get_string_opt(m, "Hit"),
        attack_type: get_string_opt(m, "AttackType"),
        element: get_string_opt(m, "Element"),
        extra: convert_extra(m, skill_known_keys()),
    })
}

fn get_string(m: &serde_yaml::Mapping, key: &str) -> Option<String> {
    m.get(Value::String(key.to_string()))
        .and_then(Value::as_str)
        .map(|s| s.to_string())
}

fn get_string_opt(m: &serde_yaml::Mapping, key: &str) -> Option<String> {
    get_string(m, key)
}

fn get_i64(m: &serde_yaml::Mapping, key: &str) -> Option<i64> {
    m.get(Value::String(key.to_string()))
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
}

fn get_i64_opt(m: &serde_yaml::Mapping, key: &str) -> Option<i64> {
    get_i64(m, key)
}

fn get_bool_opt(m: &serde_yaml::Mapping, key: &str) -> Option<bool> {
    m.get(Value::String(key.to_string()))
        .and_then(Value::as_bool)
}

fn get_stat(
    top: &serde_yaml::Mapping,
    stats: Option<&serde_yaml::Mapping>,
    key: &str,
) -> Option<i64> {
    get_i64(top, key).or_else(|| stats.and_then(|s| get_i64(s, key)))
}

fn get_attack_min(m: &serde_yaml::Mapping) -> Option<i64> {
    match m.get(Value::String("Attack".to_string())) {
        Some(Value::Sequence(seq)) => seq.first().and_then(Value::as_i64),
        Some(v) => v.as_i64(),
        _ => None,
    }
}

fn get_attack_max(m: &serde_yaml::Mapping) -> Option<i64> {
    match m.get(Value::String("Attack".to_string())) {
        Some(Value::Sequence(seq)) => seq.get(1).and_then(Value::as_i64),
        Some(v) => v.as_i64(),
        _ => None,
    }
}

fn get_element(m: &serde_yaml::Mapping) -> Option<String> {
    match m.get(Value::String("Element".to_string())) {
        Some(Value::Sequence(seq)) if !seq.is_empty() => {
            seq.first().and_then(Value::as_str).map(|s| s.to_string())
        }
        Some(v) => v.as_str().map(|s| s.to_string()),
        _ => None,
    }
}

fn parse_drops(m: &serde_yaml::Mapping) -> HashMap<String, i64> {
    let mut drops = HashMap::new();
    let Some(value) = m.get(Value::String("Drops".to_string())) else {
        return drops;
    };

    match value {
        Value::Sequence(seq) => {
            for entry in seq {
                if let Some(entry_map) = entry.as_mapping() {
                    let item = get_string(entry_map, "Item");
                    let rate = get_i64(entry_map, "Rate");
                    if let (Some(item), Some(rate)) = (item, rate) {
                        drops.insert(item, rate);
                    }
                }
            }
        }
        Value::Mapping(map) => {
            for (k, v) in map {
                let key = k.as_str().unwrap_or("").to_string();
                if let Some(rate) = v.as_i64() {
                    drops.insert(key, rate);
                }
            }
        }
        _ => {}
    }
    drops
}

fn convert_extra(
    m: &serde_yaml::Mapping,
    known: &[&str],
) -> HashMap<String, serde_json::Value> {
    m.iter()
        .filter(|(k, _)| {
            k.as_str()
                .map(|key| !known.contains(&key))
                .unwrap_or(false)
        })
        .filter_map(|(k, v)| {
            let key = k.as_str()?.to_string();
            yaml_to_json(v).map(|jv| (key, jv))
        })
        .collect()
}

fn yaml_to_json(v: &Value) -> Option<serde_json::Value> {
    match v {
        Value::Null => Some(serde_json::Value::Null),
        Value::Bool(b) => Some(serde_json::Value::Bool(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(serde_json::Value::from(i))
            } else if let Some(f) = n.as_f64() {
                Some(serde_json::Value::from(f))
            } else {
                None
            }
        }
        Value::String(s) => Some(serde_json::Value::String(s.clone())),
        Value::Sequence(seq) => {
            let arr: Vec<serde_json::Value> = seq.iter().filter_map(yaml_to_json).collect();
            Some(serde_json::Value::Array(arr))
        }
        Value::Mapping(map) => {
            let obj: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .filter_map(|(k, v)| {
                    let key = k.as_str()?.to_string();
                    yaml_to_json(v).map(|jv| (key, jv))
                })
                .collect();
            Some(serde_json::Value::Object(obj))
        }
        Value::Tagged(tagged) => yaml_to_json(&tagged.value),
    }
}

fn item_known_keys() -> &'static [&'static str] {
    &[
        "Id", "AegisName", "Name", "Type", "Buy", "Sell", "Weight", "Atk", "Matk", "Def",
        "Range", "Slots", "WeaponLv", "WeaponLevel", "EquipLv", "EquipLevel", "Refine", "Script",
        "OnEquipScript", "OnUnequipScript",
    ]
}

fn mob_known_keys() -> &'static [&'static str] {
    &[
        "Id", "Sprite", "SpriteName", "Name", "Lv", "Level", "Hp", "Sp", "Exp", "JExp",
        "AttackRange", "Attack", "Def", "Mdef", "Str", "Agi", "Vit", "Int", "Dex", "Luk",
        "Stats", "ViewRange", "ChaseRange", "Size", "Race", "Element", "MoveSpeed",
        "AttackDelay", "AttackMotion", "DamageMotion", "Drops",
    ]
}

fn skill_known_keys() -> &'static [&'static str] {
    &[
        "Id", "Name", "Description", "MaxLevel", "Range", "Hit", "AttackType", "Element",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rathena_item_db() {
        let source = r#"
Header:
  Version: 1
Body:
  - Id: 501
    AegisName: Red_Potion
    Name: Red Potion
    Type: Healing
    Buy: 50
    Weight: 70
    Script: |
      itemheal rand(45,65),0;
  - Id: 506
    AegisName: Green_Potion
    Name: Green Potion
    Type: Healing
    Buy: 40
    Weight: 70
"#;
        let items = parse_items(source).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, 501);
        assert_eq!(items[0].aegis_name, "Red_Potion");
        assert_eq!(
            items[0].script.as_ref().unwrap().trim(),
            "itemheal rand(45,65),0;"
        );
    }

    #[test]
    fn parse_rathena_mob_db() {
        let source = r#"
Body:
  - Id: 1001
    Sprite: SCORPION
    Name: Scorpion
    Level: 16
    Hp: 153
    Sp: 1
    Exp: 108
    JExp: 81
    AttackRange: 1
    Attack: [33, 7]
    Def: 16
    Mdef: 5
    Str: 12
    Agi: 15
    Vit: 10
    Int: 5
    Dex: 19
    Luk: 5
    Size: Small
    Race: Insect
    Element: Fire
    Drops:
      - Item: Red_Potion
        Rate: 70
"#;
        let mobs = parse_mobs(source).unwrap();
        assert_eq!(mobs.len(), 1);
        let mob = &mobs[0];
        assert_eq!(mob.id, 1001);
        assert_eq!(mob.name, "Scorpion");
        assert_eq!(mob.attack_min, Some(33));
        assert_eq!(mob.attack_max, Some(7));
        assert_eq!(mob.element.as_deref(), Some("Fire"));
        assert_eq!(mob.drops.get("Red_Potion"), Some(&70));
    }

    #[test]
    fn parse_rathena_skill_db() {
        let source = r#"
Body:
  - Id: 5
    Name: SM_BASH
    Description: Bash
    MaxLevel: 10
    Range: -1
    Hit: BDT_SKILL
    AttackType: Weapon
    Element: Neutral
"#;
        let skills = parse_skills(source).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "SM_BASH");
    }

    #[test]
    fn parse_rathena_database_dir() {
        let dir = tempfile::tempdir().unwrap();
        let item = dir.path().join("item_db.yml");
        std::fs::write(
            &item,
            r#"
Body:
  - Id: 502
    AegisName: Orange_Potion
    Name: Orange Potion
    Type: Healing
"#,
        )
        .unwrap();
        let mob = dir.path().join("mob_db.yml");
        std::fs::write(
            &mob,
            r#"
Body:
  - Id: 1002
    Sprite: PORING
    Name: Poring
"#,
        )
        .unwrap();
        let skill = dir.path().join("skill_db.yml");
        std::fs::write(
            &skill,
            r#"
Body:
  - Id: 6
    Name: SM_MAGNUM
    Description: Magnum Break
"#,
        )
        .unwrap();

        let db = convert_database_dir(dir.path()).unwrap();
        assert_eq!(db.items.len(), 1);
        assert_eq!(db.items[0].id, 502);
        assert_eq!(db.mobs.len(), 1);
        assert_eq!(db.mobs[0].id, 1002);
        assert_eq!(db.skills.len(), 1);
        assert_eq!(db.skills[0].id, 6);
    }
}
