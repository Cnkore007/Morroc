//! 转换后的数据结构定义。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::libconfig::Value;
use crate::npc::Npc;

/// 道具。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Item {
    pub id: i64,
    pub aegis_name: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buy: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sell: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atk: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matk: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub def: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slots: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weapon_level: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equip_level: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refine: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_equip_script: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_unequip_script: Option<String>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Item {
    pub fn from_libconfig(value: &Value) -> Option<Self> {
        let group = match value {
            Value::Group(g) => g,
            _ => return None,
        };
        Some(Item {
            id: get_i64(group, "Id")?,
            aegis_name: get_string(group, "AegisName")?,
            name: get_string(group, "Name")?,
            item_type: get_string_opt(group, "Type"),
            buy: get_i64_opt(group, "Buy"),
            sell: get_i64_opt(group, "Sell"),
            weight: get_i64_opt(group, "Weight"),
            atk: get_i64_opt(group, "Atk"),
            matk: get_i64_opt(group, "Matk"),
            def: get_i64_opt(group, "Def"),
            range: get_i64_opt(group, "Range"),
            slots: get_i64_opt(group, "Slots"),
            weapon_level: get_i64_opt(group, "WeaponLv"),
            equip_level: get_i64_opt(group, "EquipLv"),
            refine: get_bool_opt(group, "Refine"),
            script: get_string_opt(group, "Script"),
            on_equip_script: get_string_opt(group, "OnEquipScript"),
            on_unequip_script: get_string_opt(group, "OnUnequipScript"),
            extra: convert_extra(group),
        })
    }
}

/// 怪物。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Mob {
    pub id: i64,
    pub sprite_name: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_exp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_range: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_min: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_max: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub def: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mdef: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub str: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agi: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub int: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dex: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub luk: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_range: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chase_range: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub race: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub move_speed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_delay: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_motion: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damage_motion: Option<i64>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub drops: HashMap<String, i64>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Mob {
    pub fn from_libconfig(value: &Value) -> Option<Self> {
        let group = match value {
            Value::Group(g) => g,
            _ => return None,
        };
        let stats = match group.get("Stats") {
            Some(Value::Group(g)) => g,
            _ => &HashMap::new(),
        };
        let drops = match group.get("Drops") {
            Some(Value::Group(g)) => g,
            _ => &HashMap::new(),
        };
        Some(Mob {
            id: get_i64(group, "Id")?,
            sprite_name: get_string(group, "SpriteName")?,
            name: get_string(group, "Name")?,
            level: get_i64_opt(group, "Lv"),
            hp: get_i64_opt(group, "Hp"),
            sp: get_i64_opt(group, "Sp"),
            exp: get_i64_opt(group, "Exp"),
            job_exp: get_i64_opt(group, "JExp"),
            attack_range: get_i64_opt(group, "AttackRange"),
            attack_min: get_attack_min(group),
            attack_max: get_attack_max(group),
            def: get_i64_opt(group, "Def"),
            mdef: get_i64_opt(group, "Mdef"),
            str: get_i64_opt_map(stats, "Str"),
            agi: get_i64_opt_map(stats, "Agi"),
            vit: get_i64_opt_map(stats, "Vit"),
            int: get_i64_opt_map(stats, "Int"),
            dex: get_i64_opt_map(stats, "Dex"),
            luk: get_i64_opt_map(stats, "Luk"),
            view_range: get_i64_opt(group, "ViewRange"),
            chase_range: get_i64_opt(group, "ChaseRange"),
            size: get_string_opt(group, "Size"),
            race: get_string_opt(group, "Race"),
            element: get_element(group),
            move_speed: get_i64_opt(group, "MoveSpeed"),
            attack_delay: get_i64_opt(group, "AttackDelay"),
            attack_motion: get_i64_opt(group, "AttackMotion"),
            damage_motion: get_i64_opt(group, "DamageMotion"),
            drops: drops
                .iter()
                .filter_map(|(k, v)| match v {
                    Value::Int(n) => Some((k.clone(), *n)),
                    _ => None,
                })
                .collect(),
            extra: convert_extra(group),
        })
    }
}

/// 技能。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Skill {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_level: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attack_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Skill {
    pub fn from_libconfig(value: &Value) -> Option<Self> {
        let group = match value {
            Value::Group(g) => g,
            _ => return None,
        };
        Some(Skill {
            id: get_i64(group, "Id")?,
            name: get_string(group, "Name")?,
            description: get_string_opt(group, "Description"),
            max_level: get_i64_opt(group, "MaxLevel"),
            range: get_i64_opt(group, "Range"),
            hit: get_string_opt(group, "Hit"),
            attack_type: get_string_opt(group, "AttackType"),
            element: get_string_opt(group, "Element"),
            extra: convert_extra(group),
        })
    }
}

/// 转换后的数据库集合。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GameDatabase {
    pub items: Vec<Item>,
    pub mobs: Vec<Mob>,
    pub skills: Vec<Skill>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub npcs: Vec<Npc>,
    /// 状态效果定义（可选，运行时状态数据库）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statuses: Vec<serde_json::Value>,
}

fn get_i64(group: &HashMap<String, Value>, key: &str) -> Option<i64> {
    match group.get(key) {
        Some(Value::Int(n)) => Some(*n),
        _ => None,
    }
}

fn get_i64_opt(group: &HashMap<String, Value>, key: &str) -> Option<i64> {
    get_i64(group, key)
}

fn get_i64_opt_map(group: &HashMap<String, Value>, key: &str) -> Option<i64> {
    get_i64(group, key)
}

fn get_string(group: &HashMap<String, Value>, key: &str) -> Option<String> {
    match group.get(key) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn get_string_opt(group: &HashMap<String, Value>, key: &str) -> Option<String> {
    get_string(group, key)
}

fn get_bool_opt(group: &HashMap<String, Value>, key: &str) -> Option<bool> {
    match group.get(key) {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn get_attack_min(group: &HashMap<String, Value>) -> Option<i64> {
    match group.get("Attack") {
        Some(Value::Tuple(v) | Value::Array(v)) if !v.is_empty() => match &v[0] {
            Value::Int(n) => Some(*n),
            _ => None,
        },
        Some(Value::Int(n)) => Some(*n),
        _ => None,
    }
}

fn get_attack_max(group: &HashMap<String, Value>) -> Option<i64> {
    match group.get("Attack") {
        Some(Value::Tuple(v) | Value::Array(v)) if v.len() > 1 => match &v[1] {
            Value::Int(n) => Some(*n),
            _ => None,
        },
        Some(Value::Int(n)) => Some(*n),
        _ => None,
    }
}

fn get_element(group: &HashMap<String, Value>) -> Option<String> {
    match group.get("Element") {
        Some(Value::Tuple(v)) if !v.is_empty() => match &v[0] {
            Value::String(s) => Some(s.clone()),
            _ => None,
        },
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// 将剩余未显式映射的字段转为 JSON Value。
fn convert_extra(group: &HashMap<String, Value>) -> HashMap<String, serde_json::Value> {
    let known: Vec<String> = vec![
        "Id".to_string(),
        "AegisName".to_string(),
        "Name".to_string(),
        "Type".to_string(),
        "Buy".to_string(),
        "Sell".to_string(),
        "Weight".to_string(),
        "Atk".to_string(),
        "Matk".to_string(),
        "Def".to_string(),
        "Range".to_string(),
        "Slots".to_string(),
        "WeaponLv".to_string(),
        "EquipLv".to_string(),
        "Refine".to_string(),
        "Script".to_string(),
        "OnEquipScript".to_string(),
        "OnUnequipScript".to_string(),
        "SpriteName".to_string(),
        "Lv".to_string(),
        "Hp".to_string(),
        "Sp".to_string(),
        "Exp".to_string(),
        "JExp".to_string(),
        "AttackRange".to_string(),
        "Attack".to_string(),
        "Mdef".to_string(),
        "Stats".to_string(),
        "ViewRange".to_string(),
        "ChaseRange".to_string(),
        "Size".to_string(),
        "Race".to_string(),
        "Element".to_string(),
        "MoveSpeed".to_string(),
        "AttackDelay".to_string(),
        "AttackMotion".to_string(),
        "DamageMotion".to_string(),
        "Drops".to_string(),
        "MaxLevel".to_string(),
        "Hit".to_string(),
        "Description".to_string(),
    ];

    group
        .iter()
        .filter(|(k, _)| !known.contains(*k))
        .filter_map(|(k, v)| libconfig_to_json(v).map(|jv| (k.clone(), jv)))
        .collect()
}

fn libconfig_to_json(value: &Value) -> Option<serde_json::Value> {
    match value {
        Value::Null => Some(serde_json::Value::Null),
        Value::Int(n) => Some(serde_json::Value::from(*n)),
        Value::Float(n) => Some(serde_json::Value::from(*n)),
        Value::String(s) => Some(serde_json::Value::String(s.clone())),
        Value::Bool(b) => Some(serde_json::Value::Bool(*b)),
        Value::Array(arr) => {
            let values: Vec<serde_json::Value> = arr.iter().filter_map(libconfig_to_json).collect();
            Some(serde_json::Value::Array(values))
        }
        Value::Tuple(arr) => {
            let values: Vec<serde_json::Value> = arr.iter().filter_map(libconfig_to_json).collect();
            Some(serde_json::Value::Array(values))
        }
        Value::Group(g) => {
            let map: serde_json::Map<String, serde_json::Value> = g
                .iter()
                .filter_map(|(k, v)| libconfig_to_json(v).map(|jv| (k.clone(), jv)))
                .collect();
            Some(serde_json::Value::Object(map))
        }
        Value::Include(_) => None,
    }
}
