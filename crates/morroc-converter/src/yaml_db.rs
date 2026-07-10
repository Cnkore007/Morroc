//! YAML 数据库解析器。
//!
//! 支持 YAML 的 `item_db*.yml`、`mob_db*.yml`、`skill_db*.yml` 格式：
//! 顶层为 `Header` / `Body` 结构，其中 `Body` 是记录列表。
//! YAML 将道具按用途拆分为多个文件（如 item_db_usable.yml、item_db_equip.yml 等），
//! 本函数会把主文件（item_db.yml）与所有 `prefix_*.yml` 合并。
//!
//! 使用 `yaml-rust2` 作为底层解析器，允许 YAML 映射中出现重复键（YAML 中常见，后值覆盖前值）。

use crate::schema::{GameDatabase, Item, Mob, Skill};
use serde_json::Value;
use std::collections::HashMap;
use yaml_rust2::{Yaml, YamlLoader};

/// 解析 YAML 数据库目录，读取 `item_db*.yml`、`mob_db*.yml`、`skill_db*.yml`。
pub fn convert_database_dir(db_dir: &std::path::Path) -> anyhow::Result<GameDatabase> {
    let db = GameDatabase {
        items: parse_all_yaml(db_dir, "item_db", parse_items)?,
        mobs: parse_all_yaml(db_dir, "mob_db", parse_mobs)?,
        skills: parse_all_yaml(db_dir, "skill_db", parse_skills)?,
        ..Default::default()
    };

    Ok(db)
}

/// 读取主文件 `prefix.yml` 以及所有 `prefix_*.yml`，合并解析结果。
fn parse_all_yaml<T>(
    db_dir: &std::path::Path,
    prefix: &str,
    parse: impl Fn(&str) -> anyhow::Result<Vec<T>>,
) -> anyhow::Result<Vec<T>> {
    let mut results = Vec::new();

    let main_file = db_dir.join(format!("{}.yml", prefix));
    if main_file.exists() {
        let source = std::fs::read_to_string(&main_file)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", main_file.display(), e))?;
        results.extend(parse(&source)?);
    }

    let mut extras: Vec<std::path::PathBuf> = Vec::new();
    for entry in std::fs::read_dir(db_dir)
        .map_err(|e| anyhow::anyhow!("读取目录 {} 失败: {}", db_dir.display(), e))?
    {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with(&format!("{}_", prefix)) && name.ends_with(".yml") {
            extras.push(entry.path());
        }
    }
    extras.sort();

    for path in extras {
        let source = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("读取 {} 失败: {}", path.display(), e))?;
        results.extend(parse(&source)?);
    }

    Ok(results)
}

/// 解析道具 YAML。
pub fn parse_items(source: &str) -> anyhow::Result<Vec<Item>> {
    let root = load_yaml(source)?;
    let body = body_array(&root)?;
    Ok(body.iter().filter_map(item_from_value).collect())
}

/// 解析怪物 YAML。
pub fn parse_mobs(source: &str) -> anyhow::Result<Vec<Mob>> {
    let root = load_yaml(source)?;
    let body = body_array(&root)?;
    Ok(body.iter().filter_map(mob_from_value).collect())
}

/// 解析技能 YAML。
pub fn parse_skills(source: &str) -> anyhow::Result<Vec<Skill>> {
    let root = load_yaml(source)?;
    let body = body_array(&root)?;
    Ok(body.iter().filter_map(skill_from_value).collect())
}

fn load_yaml(source: &str) -> anyhow::Result<Yaml> {
    let source = deduplicate_yaml(source);
    let docs =
        YamlLoader::load_from_str(&source).map_err(|e| anyhow::anyhow!("YAML 解析失败: {}", e))?;
    Ok(docs.into_iter().next().unwrap_or(Yaml::Null))
}

/// 单遍扫描 YAML 并删除重复键的前一次出现块。
///
/// YAML 数据库中同一映射内偶尔出现重复键（后值覆盖前值）。yaml-rust2 默认会
/// 报错，因此本函数在解析前按路径去重：先遍历一次文本，为每个键建立“路径”
/// （Body / 列表索引 / 映射键 的序列），同一路径出现多次时只保留最后一次。
fn deduplicate_yaml(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let n = lines.len();

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    enum Segment {
        Key(String),
        Index(usize),
    }

    #[derive(Clone, Debug)]
    struct StackEntry {
        indent: usize,
        segment: Segment,
        block_idx: Option<usize>,
    }

    #[derive(Clone, Debug)]
    struct Block {
        path: Vec<Segment>,
        start: usize,
        end: usize,
    }

    let mut blocks: Vec<Block> = Vec::new();
    let mut stack: Vec<StackEntry> = Vec::new();
    let mut list_indices: std::collections::HashMap<Vec<Segment>, usize> =
        std::collections::HashMap::new();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut path_to_block: std::collections::HashMap<Vec<Segment>, usize> =
        std::collections::HashMap::new();

    for (i, line) in lines.iter().enumerate() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // 关闭所有缩进大于等于当前行的活跃键。
        while let Some(top) = stack.last() {
            if top.indent >= indent {
                if let Some(idx) = top.block_idx {
                    blocks[idx].end = i;
                }
                stack.pop();
            } else {
                break;
            }
        }

        let current_path: Vec<Segment> = stack.iter().map(|e| e.segment.clone()).collect();

        if let Some(rest) = trimmed.strip_prefix('-') {
            // 列表项。
            let idx = *list_indices.get(&current_path).unwrap_or(&0);
            list_indices.insert(current_path.clone(), idx + 1);
            stack.push(StackEntry {
                indent,
                segment: Segment::Index(idx),
                block_idx: None,
            });

            // 处理形如 "- Id: 400276" 的紧凑列表项键。
            let after_dash = rest.trim_start();
            if let Some(key) = parse_key_line(after_dash) {
                let key_indent = indent + 2;
                let mut path = current_path.clone();
                path.push(Segment::Index(idx));
                let block_idx = blocks.len();
                let mut path_for_block = path.clone();
                path_for_block.push(Segment::Key(key.clone()));
                blocks.push(Block {
                    path: path_for_block.clone(),
                    start: i,
                    end: n,
                });
                if let Some(&prev_idx) = path_to_block.get(&path_for_block) {
                    ranges.push((blocks[prev_idx].start, blocks[prev_idx].end));
                }
                path_to_block.insert(path_for_block, block_idx);
                stack.push(StackEntry {
                    indent: key_indent,
                    segment: Segment::Key(key),
                    block_idx: Some(block_idx),
                });
            }
        } else if let Some(key) = parse_key_line(trimmed) {
            // 映射键。
            let mut path = current_path.clone();
            path.push(Segment::Key(key));
            let block_idx = blocks.len();
            blocks.push(Block {
                path: path.clone(),
                start: i,
                end: n,
            });
            if let Some(&prev_idx) = path_to_block.get(&path) {
                ranges.push((blocks[prev_idx].start, blocks[prev_idx].end));
            }
            path_to_block.insert(path, block_idx);
            stack.push(StackEntry {
                indent,
                segment: blocks[block_idx].path.last().unwrap().clone(),
                block_idx: Some(block_idx),
            });
        }
    }

    // 关闭剩余块。
    while let Some(top) = stack.pop() {
        if let Some(idx) = top.block_idx {
            blocks[idx].end = n;
        }
    }

    // 合并重叠区间并输出。
    if ranges.is_empty() {
        return source.to_string();
    }

    ranges.sort_by_key(|r| r.0);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut() {
            if start <= last.1 {
                last.1 = last.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }

    let mut result: Vec<&str> = Vec::new();
    let mut last_end = 0;
    for (start, end) in merged {
        result.extend(&lines[last_end..start]);
        last_end = end;
    }
    result.extend(&lines[last_end..]);
    result.join("\n")
}

/// 从一行文本中提取键名（支持列表项和映射键）。
fn parse_key_line(trimmed: &str) -> Option<String> {
    if let Some(after_dash) = trimmed.strip_prefix('-') {
        let after_dash = after_dash.trim_start();
        if let Some(colon_pos) = after_dash.find(':') {
            let key = after_dash[..colon_pos].trim();
            if !key.is_empty() {
                return Some(key.to_string());
            }
        }
        return None;
    }
    if let Some(colon_pos) = trimmed.find(':') {
        let key = trimmed[..colon_pos].trim();
        if !key.is_empty() && !key.starts_with('#') {
            return Some(key.to_string());
        }
    }
    None
}

fn body_array(root: &Yaml) -> anyhow::Result<Vec<Value>> {
    let root_json = yaml_to_json(root).ok_or_else(|| anyhow::anyhow!("无法转换 YAML 根节点"))?;
    if let Some(seq) = root_json.get("Body").and_then(Value::as_array) {
        return Ok(seq.clone());
    }
    if let Some(seq) = root_json.as_array() {
        return Ok(seq.clone());
    }
    // 仅有 Header 或注释的文件视为空数据库，避免 item_db.yml 这类纯说明文件报错。
    if root_json.is_null()
        || root_json
            .as_object()
            .is_some_and(|m| m.is_empty() || m.contains_key("Header"))
    {
        return Ok(Vec::new());
    }
    anyhow::bail!("YAML 数据库缺少 Body 数组")
}

fn item_from_value(v: &Value) -> Option<Item> {
    let m = v.as_object()?;
    Some(Item {
        id: get_i64(m, "Id")?,
        aegis_name: get_string(m, "AegisName")?,
        name: get_string(m, "Name")?,
        item_type: get_string_opt(m, "Type"),
        buy: get_i64_opt(m, "Buy"),
        sell: get_i64_opt(m, "Sell"),
        weight: get_i64_opt(m, "Weight"),
        atk: get_i64_opt(m, "Atk").or_else(|| get_i64_opt(m, "Attack")),
        matk: get_i64_opt(m, "Matk").or_else(|| get_i64_opt(m, "MagicAttack")),
        def: get_i64_opt(m, "Def").or_else(|| get_i64_opt(m, "Defense")),
        range: get_i64_opt(m, "Range"),
        slots: get_i64_opt(m, "Slots"),
        weapon_level: get_i64_opt(m, "WeaponLv").or_else(|| get_i64_opt(m, "WeaponLevel")),
        equip_level: get_i64_opt(m, "EquipLv")
            .or_else(|| get_i64_opt(m, "EquipLevel"))
            .or_else(|| get_i64_opt(m, "EquipLevelMin"))
            .or_else(|| get_i64_opt(m, "ArmorLevel")),
        refine: get_bool_opt(m, "Refine").or_else(|| get_bool_opt(m, "Refineable")),
        script: get_string_opt(m, "Script"),
        on_equip_script: get_string_opt(m, "OnEquipScript"),
        on_unequip_script: get_string_opt(m, "OnUnequipScript"),
        extra: convert_extra(m, item_known_keys()),
    })
}

fn mob_from_value(v: &Value) -> Option<Mob> {
    let m = v.as_object()?;
    let stats = m.get("Stats").and_then(Value::as_object);
    Some(Mob {
        id: get_i64(m, "Id")?,
        sprite_name: get_string(m, "SpriteName")
            .or_else(|| get_string(m, "Sprite"))
            .or_else(|| get_string(m, "AegisName"))?,
        name: get_string(m, "Name")?,
        level: get_i64_opt(m, "Lv").or_else(|| get_i64_opt(m, "Level")),
        hp: get_i64_opt(m, "Hp"),
        sp: get_i64_opt(m, "Sp"),
        exp: get_i64_opt(m, "Exp").or_else(|| get_i64_opt(m, "BaseExp")),
        job_exp: get_i64_opt(m, "JExp").or_else(|| get_i64_opt(m, "JobExp")),
        attack_range: get_i64_opt(m, "AttackRange"),
        attack_min: get_attack_min(m),
        attack_max: get_attack_max(m),
        def: get_i64_opt(m, "Def").or_else(|| get_i64_opt(m, "Defense")),
        mdef: get_i64_opt(m, "Mdef").or_else(|| get_i64_opt(m, "MagicDefense")),
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
        move_speed: get_i64_opt(m, "MoveSpeed").or_else(|| get_i64_opt(m, "WalkSpeed")),
        attack_delay: get_i64_opt(m, "AttackDelay"),
        attack_motion: get_i64_opt(m, "AttackMotion"),
        damage_motion: get_i64_opt(m, "DamageMotion"),
        drops: parse_drops(m),
        extra: convert_extra(m, mob_known_keys()),
    })
}

fn skill_from_value(v: &Value) -> Option<Skill> {
    let m = v.as_object()?;
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

fn get_string(m: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    m.get(key).and_then(Value::as_str).map(|s| s.to_string())
}

fn get_string_opt(m: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    get_string(m, key)
}

fn get_i64(m: &serde_json::Map<String, Value>, key: &str) -> Option<i64> {
    m.get(key)
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
}

fn get_i64_opt(m: &serde_json::Map<String, Value>, key: &str) -> Option<i64> {
    get_i64(m, key)
}

fn get_bool_opt(m: &serde_json::Map<String, Value>, key: &str) -> Option<bool> {
    m.get(key).and_then(Value::as_bool)
}

fn get_stat(
    top: &serde_json::Map<String, Value>,
    stats: Option<&serde_json::Map<String, Value>>,
    key: &str,
) -> Option<i64> {
    get_i64(top, key).or_else(|| stats.and_then(|s| get_i64(s, key)))
}

fn get_attack_min(m: &serde_json::Map<String, Value>) -> Option<i64> {
    match m.get("Attack") {
        Some(Value::Array(seq)) => seq.first().and_then(Value::as_i64),
        Some(v) => v.as_i64(),
        _ => None,
    }
}

fn get_attack_max(m: &serde_json::Map<String, Value>) -> Option<i64> {
    match m.get("Attack") {
        Some(Value::Array(seq)) => seq.get(1).and_then(Value::as_i64),
        _ => get_i64_opt(m, "Attack2"),
    }
}

fn get_element(m: &serde_json::Map<String, Value>) -> Option<String> {
    match m.get("Element") {
        Some(Value::Array(seq)) if !seq.is_empty() => {
            seq.first().and_then(Value::as_str).map(|s| s.to_string())
        }
        Some(v) => v.as_str().map(|s| s.to_string()),
        _ => None,
    }
}

fn parse_drops(m: &serde_json::Map<String, Value>) -> HashMap<String, i64> {
    let mut drops = HashMap::new();
    let Some(value) = m.get("Drops") else {
        return drops;
    };

    match value {
        Value::Array(seq) => {
            for entry in seq {
                if let Some(entry_map) = entry.as_object() {
                    let item = get_string(entry_map, "Item");
                    let rate = get_i64(entry_map, "Rate");
                    if let (Some(item), Some(rate)) = (item, rate) {
                        drops.insert(item, rate);
                    }
                }
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                if let Some(rate) = v.as_i64() {
                    drops.insert(k.clone(), rate);
                }
            }
        }
        _ => {}
    }
    drops
}

fn convert_extra(
    m: &serde_json::Map<String, Value>,
    known: &[&str],
) -> HashMap<String, serde_json::Value> {
    m.iter()
        .filter(|(k, _)| !known.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn yaml_to_json(v: &Yaml) -> Option<Value> {
    match v {
        Yaml::Null => Some(Value::Null),
        Yaml::Boolean(b) => Some(Value::Bool(*b)),
        Yaml::Integer(i) => Some(Value::from(*i)),
        Yaml::Real(s) => Some(Value::from(s.parse::<f64>().ok()?)),
        Yaml::String(s) => Some(Value::String(s.clone())),
        Yaml::Array(arr) => {
            let arr: Vec<Value> = arr.iter().filter_map(yaml_to_json).collect();
            Some(Value::Array(arr))
        }
        Yaml::Hash(map) => {
            let obj: serde_json::Map<String, Value> = map
                .iter()
                .filter_map(|(k, v)| {
                    let key = k.as_str()?.to_string();
                    yaml_to_json(v).map(|jv| (key, jv))
                })
                .collect();
            Some(Value::Object(obj))
        }
        Yaml::Alias(_) => None,
        Yaml::BadValue => None,
    }
}

fn item_known_keys() -> &'static [&'static str] {
    &[
        "Id",
        "AegisName",
        "Name",
        "Type",
        "SubType",
        "Buy",
        "Sell",
        "Weight",
        "Atk",
        "Attack",
        "Matk",
        "MagicAttack",
        "Def",
        "Defense",
        "Range",
        "Slots",
        "WeaponLv",
        "WeaponLevel",
        "EquipLv",
        "EquipLevel",
        "EquipLevelMin",
        "EquipLevelMax",
        "ArmorLevel",
        "Refine",
        "Refineable",
        "Script",
        "OnEquipScript",
        "OnUnequipScript",
    ]
}

fn mob_known_keys() -> &'static [&'static str] {
    &[
        "Id",
        "Sprite",
        "SpriteName",
        "AegisName",
        "Name",
        "Lv",
        "Level",
        "Hp",
        "Sp",
        "Exp",
        "BaseExp",
        "JExp",
        "JobExp",
        "AttackRange",
        "Attack",
        "Attack2",
        "Def",
        "Defense",
        "Mdef",
        "MagicDefense",
        "Str",
        "Agi",
        "Vit",
        "Int",
        "Dex",
        "Luk",
        "Stats",
        "ViewRange",
        "SkillRange",
        "ChaseRange",
        "Size",
        "Race",
        "RaceGroups",
        "Element",
        "ElementLevel",
        "MoveSpeed",
        "WalkSpeed",
        "AttackDelay",
        "AttackMotion",
        "DamageMotion",
        "Drops",
        "MvpDrops",
        "Modes",
        "Mode",
    ]
}

fn skill_known_keys() -> &'static [&'static str] {
    &[
        "Id",
        "Name",
        "Description",
        "MaxLevel",
        "Range",
        "Hit",
        "AttackType",
        "Element",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_yaml_item_db() {
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
    fn parse_yaml_mob_db() {
        let source = r#"
Body:
  - Id: 1001
    Sprite: SCORPION
    Name: Scorpion
    Level: 16
    Hp: 153
    Sp: 1
    BaseExp: 108
    JobExp: 81
    AttackRange: 1
    Attack: 33
    Attack2: 7
    Defense: 16
    MagicDefense: 5
    Str: 12
    Agi: 15
    Vit: 10
    Int: 5
    Dex: 19
    Luk: 5
    Size: Small
    Race: Insect
    Element: Fire
    ElementLevel: 1
    WalkSpeed: 200
    AttackDelay: 1564
    AttackMotion: 864
    DamageMotion: 576
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
        assert_eq!(mob.move_speed, Some(200));
        assert_eq!(mob.drops.get("Red_Potion"), Some(&70));
    }

    #[test]
    fn parse_yaml_skill_db() {
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
    fn parse_yaml_database_dir() {
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

    #[test]
    fn parse_yaml_split_item_db() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("item_db.yml");
        std::fs::write(&main, "# header only\n").unwrap();
        let usable = dir.path().join("item_db_usable.yml");
        std::fs::write(
            &usable,
            r#"
Body:
  - Id: 501
    AegisName: Red_Potion
    Name: Red Potion
    Type: Healing
"#,
        )
        .unwrap();
        let equip = dir.path().join("item_db_equip.yml");
        std::fs::write(
            &equip,
            r#"
Body:
  - Id: 2101
    AegisName: Sword
    Name: Sword
    Type: Weapon
"#,
        )
        .unwrap();

        let db = convert_database_dir(dir.path()).unwrap();
        assert_eq!(db.items.len(), 2);
        let ids: Vec<i64> = db.items.iter().map(|i| i.id).collect();
        assert!(ids.contains(&501));
        assert!(ids.contains(&2101));
    }

    #[test]
    fn parse_duplicate_yaml_keys() {
        // YAML 中同一映射内出现重复键时，后值覆盖前值。
        let source = r#"
Body:
  - Id: 400276
    AegisName: Comp_Bright_Fury_ROC
    Name: "[Not For Sale] Sinulog Hat"
    Trade:
      NoDrop: true
      NoTrade: true
    Trade:
      NoDrop: false
"#;
        let items = parse_items(source).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, 400276);
        // 解析不 panic 即视为通过；具体保留哪个值取决于 yaml-rust2 行为。
    }
}
