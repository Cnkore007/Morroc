//! Morroc Converter
//!
//! 将 Hercules/rAthena 的 `.conf` 数据库和 NPC 脚本转换为 Rust 友好的 JSON/TOML/AST。

pub mod libconfig;
pub mod npc;
pub mod rathena;
pub mod schema;

use anyhow::Context;
use schema::{GameDatabase, Item, Mob, Skill};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::warn;

use crate::libconfig::{Document, Value};
use crate::npc::NpcFile;

pub use libconfig::ParseError;

/// 解析 Hercules 数据库目录，转换 item/mob/skill 数据库。
///
/// 默认读取 `db/re/` 下的 `item_db.conf`、`mob_db.conf`、`skill_db.conf`。
/// 如果找不到 `db/re`，会回退到 `db/pre-re/` 或指定的目录。
pub fn convert_database_dir(db_dir: &Path) -> anyhow::Result<GameDatabase> {
    let mut db = GameDatabase::default();

    let item_path = db_dir.join("item_db.conf");
    if item_path.exists() {
        let source = fs::read_to_string(&item_path)
            .with_context(|| format!("读取 {} 失败", item_path.display()))?;
        let doc = libconfig::parse(&source)?;
        db.items = convert_items(&doc);
    }

    let mob_path = db_dir.join("mob_db.conf");
    if mob_path.exists() {
        let source = fs::read_to_string(&mob_path)
            .with_context(|| format!("读取 {} 失败", mob_path.display()))?;
        let doc = libconfig::parse(&source)?;
        db.mobs = convert_mobs(&doc);
    }

    let skill_path = db_dir.join("skill_db.conf");
    if skill_path.exists() {
        let source = fs::read_to_string(&skill_path)
            .with_context(|| format!("读取 {} 失败", skill_path.display()))?;
        let doc = libconfig::parse(&source)?;
        db.skills = convert_skills(&doc);
    }

    Ok(db)
}

/// 从 Hercules 仓库根目录自动定位数据库，并扫描 NPC 脚本目录。
pub fn convert_hercules(hercules_dir: &Path) -> anyhow::Result<GameDatabase> {
    let re = hercules_dir.join("db/re");
    let pre = hercules_dir.join("db/pre-re");
    let db_dir = if re.exists() { re } else { pre };
    let mut db = convert_database_dir(&db_dir)?;

    let npc_dir = hercules_dir.join("npc");
    if npc_dir.exists() {
        db.npcs = convert_npc_dir(&npc_dir)?;
    }
    Ok(db)
}

/// 解析单个 NPC 脚本文件。
pub fn convert_npc_file(path: &Path) -> anyhow::Result<NpcFile> {
    let source =
        fs::read_to_string(path).with_context(|| format!("读取 {} 失败", path.display()))?;
    Ok(npc::parse(&source)?)
}

/// 递归扫描 NPC 脚本目录，解析所有 `.txt` 文件并收集 NPC 定义。
pub fn convert_npc_dir(npc_dir: &Path) -> anyhow::Result<Vec<crate::npc::Npc>> {
    let mut npcs = Vec::new();
    for entry in walkdir::WalkDir::new(npc_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("txt"))
    {
        let path = entry.path();
        match convert_npc_file(path) {
            Ok(file) => npcs.extend(file.npcs),
            Err(e) => warn!("跳过无法解析的 NPC 脚本 {}: {}", path.display(), e),
        }
    }
    Ok(npcs)
}

fn convert_items(doc: &Document) -> Vec<Item> {
    let mut items = Vec::new();
    if let Some(Value::Tuple(entries)) = doc.get("item_db") {
        for entry in entries {
            if let Some(item) = Item::from_libconfig(entry) {
                items.push(item);
            }
        }
    }
    items
}

fn convert_mobs(doc: &Document) -> Vec<Mob> {
    let mut mobs = Vec::new();
    if let Some(Value::Tuple(entries)) = doc.get("mob_db") {
        for entry in entries {
            if let Some(mob) = Mob::from_libconfig(entry) {
                mobs.push(mob);
            }
        }
    }
    mobs
}

fn convert_skills(doc: &Document) -> Vec<Skill> {
    let mut skills = Vec::new();
    if let Some(Value::Tuple(entries)) = doc.get("skill_db") {
        for entry in entries {
            if let Some(skill) = Skill::from_libconfig(entry) {
                skills.push(skill);
            }
        }
    }
    skills
}

/// 将数据库写入 JSON 文件。
pub fn write_database_json(db: &GameDatabase, path: &Path) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(db).context("序列化数据库为 JSON 失败")?;
    fs::write(path, json).with_context(|| format!("写入 {} 失败", path.display()))?;
    Ok(())
}

/// 将数据库写入 TOML 文件。
pub fn write_database_toml(db: &GameDatabase, path: &Path) -> anyhow::Result<()> {
    let toml = toml::to_string_pretty(db).context("序列化数据库为 TOML 失败")?;
    fs::write(path, toml).with_context(|| format!("写入 {} 失败", path.display()))?;
    Ok(())
}

/// 将转换结果表示为 `HashMap` 形式，便于嵌入其他配置。
pub fn database_to_map(
    db: &GameDatabase,
) -> HashMap<String, Vec<HashMap<String, serde_json::Value>>> {
    let mut map = HashMap::new();
    map.insert(
        "items".to_string(),
        db.items
            .iter()
            .filter_map(|item| serde_json::to_value(item).ok())
            .filter_map(|v| v.as_object().cloned().map(|o| o.into_iter().collect()))
            .collect(),
    );
    map.insert(
        "mobs".to_string(),
        db.mobs
            .iter()
            .filter_map(|mob| serde_json::to_value(mob).ok())
            .filter_map(|v| v.as_object().cloned().map(|o| o.into_iter().collect()))
            .collect(),
    );
    map.insert(
        "skills".to_string(),
        db.skills
            .iter()
            .filter_map(|skill| serde_json::to_value(skill).ok())
            .filter_map(|v| v.as_object().cloned().map(|o| o.into_iter().collect()))
            .collect(),
    );
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_sample_item_db() {
        let source = r#"item_db: (
        {
            Id: 501
            AegisName: "Red_Potion"
            Name: "Red Potion"
            Type: "IT_HEALING"
            Buy: 50
            Weight: 70
            Script: <" itemheal rand(45,65),0; ">
        },
        {
            Id: 506
            AegisName: "Green_Potion"
            Name: "Green Potion"
            Type: "IT_HEALING"
            Buy: 40
            Weight: 70
        }
        )"#;
        let doc = libconfig::parse(source).unwrap();
        let items = convert_items(&doc);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, 501);
        assert_eq!(items[0].aegis_name, "Red_Potion");
        assert_eq!(
            items[0].script.as_ref().unwrap().trim(),
            "itemheal rand(45,65),0;"
        );
    }

    #[test]
    fn convert_sample_mob_db() {
        let source = r#"mob_db: (
        {
            Id: 1001
            SpriteName: "SCORPION"
            Name: "Scorpion"
            Lv: 16
            Hp: 153
            Sp: 1
            Exp: 108
            JExp: 81
            AttackRange: 1
            Attack: [33, 7]
            Def: 16
            Mdef: 5
            Stats: {
                Str: 12
                Agi: 15
                Vit: 10
                Int: 5
                Dex: 19
                Luk: 5
            }
            Size: "Size_Small"
            Race: "RC_Insect"
            Element: ("Ele_Fire", 1)
            Drops: {
                Red_Potion: 70
            }
        }
        )"#;
        let doc = libconfig::parse(source).unwrap();
        let mobs = convert_mobs(&doc);
        assert_eq!(mobs.len(), 1);
        let mob = &mobs[0];
        assert_eq!(mob.id, 1001);
        assert_eq!(mob.name, "Scorpion");
        assert_eq!(mob.attack_min, Some(33));
        assert_eq!(mob.attack_max, Some(7));
        assert_eq!(mob.element.as_deref(), Some("Ele_Fire"));
        assert_eq!(mob.drops.get("Red_Potion"), Some(&70));
    }

    #[test]
    fn convert_sample_skill_db() {
        let source = r#"skill_db: (
        {
            Id: 5
            Name: "SM_BASH"
            Description: "Bash"
            MaxLevel: 10
            Range: -1
            Hit: "BDT_SKILL"
            AttackType: "Weapon"
            Element: "Ele_Weapon"
        }
        )"#;
        let doc = libconfig::parse(source).unwrap();
        let skills = convert_skills(&doc);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "SM_BASH");
    }

    #[test]
    fn parse_hercules_item_db() {
        let source = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../vendor/hercules/db/re/item_db.conf"),
        )
        .unwrap();
        let doc = libconfig::parse(&source).unwrap();
        let items = convert_items(&doc);
        assert!(!items.is_empty(), "应至少解析一个道具");
        assert_eq!(items[0].id, 501);
    }

    #[test]
    fn parse_hercules_mob_db() {
        let source = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../vendor/hercules/db/re/mob_db.conf"),
        )
        .unwrap();
        let doc = libconfig::parse(&source).unwrap();
        let mobs = convert_mobs(&doc);
        assert!(!mobs.is_empty(), "应至少解析一个怪物");
        assert_eq!(mobs[0].id, 1001);
    }

    #[test]
    fn parse_hercules_skill_db() {
        let source = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../vendor/hercules/db/re/skill_db.conf"),
        )
        .unwrap();
        let doc = libconfig::parse(&source).unwrap();
        let skills = convert_skills(&doc);
        assert!(!skills.is_empty(), "应至少解析一个技能");
        assert_eq!(skills[0].id, 1);
    }

    #[test]
    fn convert_hercules_database_dir() {
        let db = convert_database_dir(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../vendor/hercules/db/re")
                .as_path(),
        )
        .unwrap();
        assert!(!db.items.is_empty(), "应至少解析一个道具");
        assert!(!db.mobs.is_empty(), "应至少解析一个怪物");
        assert!(!db.skills.is_empty(), "应至少解析一个技能");
    }

    #[test]
    fn convert_hercules_full() {
        let db = convert_hercules(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../vendor/hercules")
                .as_path(),
        )
        .unwrap();
        assert!(!db.items.is_empty(), "应至少解析一个道具");
        assert!(!db.mobs.is_empty(), "应至少解析一个怪物");
        assert!(!db.skills.is_empty(), "应至少解析一个技能");
    }
}
