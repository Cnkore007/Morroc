//! Morroc Agent 本体论（Ontology）模块。
//!
//! 以声明式 schema 描述服务器中的实体类别、属性、关系与约束，
//! 供 GmAgent 在动态生成/修改玩家、地图、NPC、道具、装备等对象时做校验与提示。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 属性类型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PropertyType {
    /// 字符串。
    #[default]
    String,
    /// 整数。
    Integer,
    /// 浮点数。
    Number,
    /// 布尔值。
    Boolean,
    /// 枚举，给出可选值列表。
    Enum(Vec<String>),
    /// 对另一实体类的引用，参数为类名。
    Entity(String),
    /// 数组，元素类型为内层属性类型。
    Array(Box<PropertyType>),
}

/// 属性基数。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    /// 恰好一个。
    #[default]
    One,
    /// 零个或多个。
    Many,
    /// 零或一个。
    Optional,
}

/// 实体属性定义。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Property {
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub property_type: PropertyType,
    #[serde(default)]
    pub cardinality: Cardinality,
    #[serde(default)]
    pub required: bool,
}

impl Property {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        property_type: PropertyType,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            property_type,
            cardinality: Cardinality::One,
            required: false,
        }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn many(mut self) -> Self {
        self.cardinality = Cardinality::Many;
        self
    }

    pub fn optional(mut self) -> Self {
        self.cardinality = Cardinality::Optional;
        self
    }
}

/// 实体类之间的关系。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Relationship {
    pub name: String,
    pub description: String,
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub cardinality: Cardinality,
}

impl Relationship {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        source: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            source: source.into(),
            target: target.into(),
            cardinality: Cardinality::One,
        }
    }

    pub fn many(mut self) -> Self {
        self.cardinality = Cardinality::Many;
        self
    }
}

/// 实体类定义。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct EntityClass {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub properties: Vec<Property>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

impl EntityClass {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            properties: Vec::new(),
            relationships: Vec::new(),
        }
    }

    pub fn with_property(mut self, p: Property) -> Self {
        self.properties.push(p);
        self
    }

    pub fn with_relationship(mut self, r: Relationship) -> Self {
        self.relationships.push(r);
        self
    }
}

/// 本体定义。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Ontology {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub classes: Vec<EntityClass>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}

impl Ontology {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            classes: Vec::new(),
            relationships: Vec::new(),
        }
    }

    pub fn with_class(mut self, c: EntityClass) -> Self {
        self.classes.push(c);
        self
    }

    pub fn with_relationship(mut self, r: Relationship) -> Self {
        self.relationships.push(r);
        self
    }

    /// 查找指定类。
    pub fn class(&self, name: &str) -> Option<&EntityClass> {
        self.classes.iter().find(|c| c.name == name)
    }

    /// 校验一个 JSON 实例是否符合某个类的 schema。
    ///
    /// 仅检查 required 属性是否存在且类型匹配；不检查未知属性。
    pub fn validate(&self, class_name: &str, instance: &Value) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();
        let Some(class) = self.class(class_name) else {
            return Err(vec![format!("未知类: {}", class_name)]);
        };
        let obj = match instance.as_object() {
            Some(o) => o,
            None => {
                return Err(vec![format!("{} 实例必须是 JSON Object", class_name)]);
            }
        };

        for prop in &class.properties {
            let Some(value) = obj.get(&prop.name) else {
                if prop.required {
                    errors.push(format!("缺少必填属性: {}", prop.name));
                }
                continue;
            };
            if let Err(e) = Self::check_type(&prop.property_type, value) {
                errors.push(format!("属性 {} 类型错误: {}", prop.name, e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    fn check_type(ty: &PropertyType, value: &Value) -> Result<(), String> {
        match ty {
            PropertyType::String => {
                if !value.is_string() {
                    return Err("应为字符串".to_string());
                }
            }
            PropertyType::Integer => {
                if !value.is_i64() {
                    return Err("应为整数".to_string());
                }
            }
            PropertyType::Number => {
                if !value.is_number() {
                    return Err("应为数字".to_string());
                }
            }
            PropertyType::Boolean => {
                if !value.is_boolean() {
                    return Err("应为布尔值".to_string());
                }
            }
            PropertyType::Enum(options) => {
                let s = value
                    .as_str()
                    .ok_or_else(|| "枚举值应为字符串".to_string())?;
                if !options.iter().any(|o| o == s) {
                    return Err(format!("可选值: {:?}", options));
                }
            }
            PropertyType::Entity(_) => {
                if !value.is_number() && !value.is_string() {
                    return Err("应为实体标识符（整数或字符串）".to_string());
                }
            }
            PropertyType::Array(inner) => {
                let arr = value.as_array().ok_or_else(|| "应为数组".to_string())?;
                for (i, item) in arr.iter().enumerate() {
                    if let Err(e) = Self::check_type(inner, item) {
                        return Err(format!("[{}] {}", i, e));
                    }
                }
            }
        }
        Ok(())
    }
}

/// 返回 Morroc 服务器默认本体论。
///
/// 包含 Account、Player、Map、Npc、Monster、Item、Equipment、Skill、Script、Guild、Session、Zone 等类。
pub fn server_ontology() -> Ontology {
    Ontology::new("morroc_server", "Morroc 游戏服务端运行时本体论")
        .with_class(
            EntityClass::new("Account", "游戏账户")
                .with_property(Property::new("id", "账户 ID", PropertyType::Integer).required())
                .with_property(
                    Property::new("userid", "登录用户名", PropertyType::String).required(),
                )
                .with_property(Property::new(
                    "sex",
                    "性别",
                    PropertyType::Enum(vec!["M".to_string(), "F".to_string()]),
                ))
                .with_relationship(Relationship::new(
                    "has_player",
                    "账户拥有玩家角色",
                    "Account",
                    "Player",
                )),
        )
        .with_class(
            EntityClass::new("Player", "在线玩家角色")
                .with_property(
                    Property::new("account_id", "账户 ID", PropertyType::Integer).required(),
                )
                .with_property(
                    Property::new("char_id", "角色 ID", PropertyType::Integer).required(),
                )
                .with_property(Property::new("name", "角色名", PropertyType::String).required())
                .with_property(Property::new("job", "职业 ID", PropertyType::Integer))
                .with_property(Property::new("level", "等级", PropertyType::Integer))
                .with_property(Property::new("x", "X 坐标", PropertyType::Integer))
                .with_property(Property::new("y", "Y 坐标", PropertyType::Integer))
                .with_property(Property::new("hp", "当前 HP", PropertyType::Integer))
                .with_property(Property::new("max_hp", "最大 HP", PropertyType::Integer))
                .with_property(
                    Property::new("guild_id", "所属公会 ID", PropertyType::Integer).optional(),
                )
                .with_relationship(Relationship::new("on_map", "玩家位于地图", "Player", "Map"))
                .with_relationship(
                    Relationship::new("owns_item", "玩家拥有道具", "Player", "Item").many(),
                ),
        )
        .with_class(
            EntityClass::new("Map", "地图实例")
                .with_property(Property::new("name", "地图名称", PropertyType::String).required())
                .with_property(Property::new("width", "宽度", PropertyType::Integer))
                .with_property(Property::new("height", "高度", PropertyType::Integer))
                .with_relationship(
                    Relationship::new("has_player", "地图上存在玩家", "Map", "Player").many(),
                )
                .with_relationship(
                    Relationship::new("has_npc", "地图上存在 NPC", "Map", "Npc").many(),
                )
                .with_relationship(
                    Relationship::new("has_monster", "地图上存在怪物", "Map", "Monster").many(),
                )
                .with_relationship(
                    Relationship::new("has_zone", "地图包含 WoE 区域", "Map", "Zone").many(),
                ),
        )
        .with_class(
            EntityClass::new("Npc", "非玩家角色")
                .with_property(Property::new("id", "实体 ID", PropertyType::Integer).required())
                .with_property(Property::new("name", "NPC 名称", PropertyType::String).required())
                .with_property(Property::new("x", "X 坐标", PropertyType::Integer).required())
                .with_property(Property::new("y", "Y 坐标", PropertyType::Integer).required())
                .with_property(Property::new("sprite", "精灵名称", PropertyType::String))
                .with_property(Property::new("script", "脚本体", PropertyType::String).optional()),
        )
        .with_class(
            EntityClass::new("Monster", "怪物")
                .with_property(Property::new("id", "怪物 ID", PropertyType::Integer).required())
                .with_property(Property::new("name", "怪物名称", PropertyType::String).required())
                .with_property(Property::new("x", "X 坐标", PropertyType::Integer))
                .with_property(Property::new("y", "Y 坐标", PropertyType::Integer))
                .with_property(Property::new("level", "等级", PropertyType::Integer))
                .with_property(Property::new("hp", "HP", PropertyType::Integer))
                .with_property(Property::new(
                    "mob_id",
                    "数据库怪物 ID",
                    PropertyType::Integer,
                )),
        )
        .with_class(
            EntityClass::new("Item", "通用道具")
                .with_property(Property::new("id", "道具 ID", PropertyType::Integer).required())
                .with_property(
                    Property::new("aegis_name", "Aegis 名称", PropertyType::String).required(),
                )
                .with_property(Property::new("name", "显示名称", PropertyType::String).required())
                .with_property(Property::new("item_type", "道具类型", PropertyType::String))
                .with_property(Property::new("buy", "购买价格", PropertyType::Integer))
                .with_property(Property::new("sell", "出售价格", PropertyType::Integer))
                .with_property(Property::new("weight", "重量", PropertyType::Integer))
                .with_property(Property::new("atk", "物理攻击力", PropertyType::Integer))
                .with_property(Property::new("matk", "魔法攻击力", PropertyType::Integer))
                .with_property(Property::new("def", "防御力", PropertyType::Integer))
                .with_property(Property::new("range", "攻击范围", PropertyType::Integer))
                .with_property(Property::new("slots", "插槽数", PropertyType::Integer))
                .with_property(Property::new(
                    "weapon_level",
                    "武器等级",
                    PropertyType::Integer,
                ))
                .with_property(Property::new(
                    "equip_level",
                    "装备等级",
                    PropertyType::Integer,
                ))
                .with_property(Property::new("refine", "是否可精炼", PropertyType::Boolean))
                .with_property(Property::new("script", "使用脚本", PropertyType::String))
                .with_property(Property::new(
                    "on_equip_script",
                    "装备脚本",
                    PropertyType::String,
                ))
                .with_property(Property::new(
                    "on_unequip_script",
                    "卸下脚本",
                    PropertyType::String,
                )),
        )
        .with_class(
            EntityClass::new("Equipment", "可穿戴装备")
                .with_property(Property::new("id", "道具 ID", PropertyType::Integer).required())
                .with_property(
                    Property::new("aegis_name", "Aegis 名称", PropertyType::String).required(),
                )
                .with_property(Property::new("name", "显示名称", PropertyType::String).required())
                .with_property(Property::new(
                    "equip_level",
                    "装备等级",
                    PropertyType::Integer,
                ))
                .with_property(Property::new("atk", "物理攻击力", PropertyType::Integer))
                .with_property(Property::new("matk", "魔法攻击力", PropertyType::Integer))
                .with_property(Property::new("def", "防御力", PropertyType::Integer))
                .with_property(Property::new("slots", "插槽数", PropertyType::Integer))
                .with_property(Property::new("refine", "是否可精炼", PropertyType::Boolean))
                .with_property(Property::new(
                    "on_equip_script",
                    "装备脚本",
                    PropertyType::String,
                ))
                .with_property(Property::new(
                    "on_unequip_script",
                    "卸下脚本",
                    PropertyType::String,
                ))
                .with_relationship(
                    Relationship::new("equipped_by", "被玩家装备", "Equipment", "Player").many(),
                ),
        )
        .with_class(
            EntityClass::new("Skill", "技能")
                .with_property(Property::new("id", "技能 ID", PropertyType::Integer).required())
                .with_property(Property::new("name", "技能名", PropertyType::String).required())
                .with_property(Property::new(
                    "max_level",
                    "最大等级",
                    PropertyType::Integer,
                ))
                .with_property(Property::new("element", "元素", PropertyType::String))
                .with_property(Property::new(
                    "attack_type",
                    "攻击类型",
                    PropertyType::String,
                ))
                .with_property(Property::new(
                    "damage_factor",
                    "伤害系数",
                    PropertyType::Number,
                )),
        )
        .with_class(
            EntityClass::new("Script", "DSL 脚本")
                .with_property(Property::new("name", "脚本名", PropertyType::String).required())
                .with_property(
                    Property::new("content", "脚本内容", PropertyType::String).required(),
                )
                .with_property(Property::new("path", "文件路径", PropertyType::String)),
        )
        .with_class(
            EntityClass::new("Guild", "公会")
                .with_property(Property::new("id", "公会 ID", PropertyType::Integer).required())
                .with_property(Property::new("name", "公会名", PropertyType::String).required())
                .with_property(Property::new(
                    "master_account_id",
                    "会长账户 ID",
                    PropertyType::Integer,
                ))
                .with_relationship(
                    Relationship::new("owns_zone", "占领 WoE 区域", "Guild", "Zone").many(),
                ),
        )
        .with_class(
            EntityClass::new("Session", "登录会话")
                .with_property(
                    Property::new("account_id", "账户 ID", PropertyType::Integer).required(),
                )
                .with_property(
                    Property::new("auth_code", "认证码", PropertyType::Integer).required(),
                )
                .with_relationship(Relationship::new(
                    "belongs_to",
                    "会话属于账户",
                    "Session",
                    "Account",
                )),
        )
        .with_class(
            EntityClass::new("Zone", "WoE 区域")
                .with_property(Property::new("id", "区域 ID", PropertyType::Integer).required())
                .with_property(Property::new("name", "区域名", PropertyType::String).required())
                .with_property(Property::new("map", "所在地图", PropertyType::String))
                .with_property(Property::new("x1", "左上角 X", PropertyType::Integer))
                .with_property(Property::new("y1", "左上角 Y", PropertyType::Integer))
                .with_property(Property::new("x2", "右下角 X", PropertyType::Integer))
                .with_property(Property::new("y2", "右下角 Y", PropertyType::Integer))
                .with_property(
                    Property::new("owner_guild_id", "当前占领公会 ID", PropertyType::Integer)
                        .optional(),
                ),
        )
        .with_relationship(
            Relationship::new("player_owns_item", "玩家拥有道具", "Player", "Item").many(),
        )
        .with_relationship(
            Relationship::new("map_contains_npc", "地图包含 NPC", "Map", "Npc").many(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ontology_has_core_classes() {
        let onto = server_ontology();
        let names: Vec<_> = onto.classes.iter().map(|c| c.name.as_str()).collect();
        for expected in [
            "Account",
            "Player",
            "Map",
            "Npc",
            "Monster",
            "Item",
            "Equipment",
            "Skill",
            "Script",
            "Guild",
            "Session",
            "Zone",
        ] {
            assert!(names.contains(&expected), "缺少类 {}", expected);
        }
    }

    #[test]
    fn validate_item_requires_id_name() {
        let onto = server_ontology();
        let item = serde_json::json!({
            "id": 9999,
            "aegis_name": "GM_Dagger",
            "name": "GM Dagger"
        });
        assert!(onto.validate("Item", &item).is_ok());

        let bad = serde_json::json!({
            "id": 9999,
            "aegis_name": "GM_Dagger"
        });
        let err = onto.validate("Item", &bad).unwrap_err();
        assert!(err.iter().any(|e| e.contains("name")), "应提示缺少 name");
    }

    #[test]
    fn validate_npc_coordinates() {
        let onto = server_ontology();
        let npc = serde_json::json!({
            "id": 100,
            "name": "Event NPC",
            "x": 150,
            "y": 180
        });
        assert!(onto.validate("Npc", &npc).is_ok());
    }

    #[test]
    fn validate_enum_sex() {
        let onto = server_ontology();
        let account = serde_json::json!({
            "id": 1,
            "userid": "admin",
            "sex": "X"
        });
        let err = onto.validate("Account", &account).unwrap_err();
        assert!(err.iter().any(|e| e.contains("可选值")), "应提示枚举错误");
    }
}
