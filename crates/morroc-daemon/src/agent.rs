//! Morroc 内置 LLM Agent 的工具执行器。
//!
//! 把上层 `morroc_agent` 定义的工具调用转换成对数据库、脚本、配置等的实际操作。

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use morroc_agent::{ontology::server_ontology, Agent, LlmClient, LlmConfig, Tool, ToolExecutor};
use morroc_dsl::Value;
use morroc_map::data::Item;
use serde_json::Map as JsonMap;
use serde_json::Value as JsonValue;
use tracing::{error, info, warn};

use crate::dsl::ScriptRuntime;

/// Agent 运行上下文。
pub struct AgentContext {
    db: morroc_db::Database,
    scripts: Arc<Mutex<ScriptRuntime>>,
    scripts_dir: PathBuf,
    listen_addrs: Vec<String>,
    sessions: Arc<dyn morroc_db::SessionStore>,
    map_server: morroc_map::MapServer,
}

impl AgentContext {
    pub fn new(
        db: morroc_db::Database,
        scripts: Arc<Mutex<ScriptRuntime>>,
        scripts_dir: impl AsRef<Path>,
        listen_addrs: Vec<String>,
        sessions: Arc<dyn morroc_db::SessionStore>,
        map_server: morroc_map::MapServer,
    ) -> Self {
        Self {
            db,
            scripts,
            scripts_dir: scripts_dir.as_ref().to_path_buf(),
            listen_addrs,
            sessions,
            map_server,
        }
    }

    /// 构建 Agent 并启动 HTTP 服务。
    pub async fn serve(self, addr: std::net::SocketAddr) -> anyhow::Result<()> {
        let tools = default_tools();
        let executor: Arc<dyn ToolExecutor> = Arc::new(self);
        let agent = Arc::new(Agent::new(
            tools,
            executor,
            LlmClient::new(LlmConfig::from_env()),
        ));
        agent.run_http(addr).await
    }
}

impl ToolExecutor for AgentContext {
    fn execute(
        &self,
        name: &str,
        args: &JsonValue,
    ) -> Pin<Box<dyn Future<Output = anyhow::Result<JsonValue>> + Send + '_>> {
        let name = name.to_string();
        let args = args.clone();
        let db = self.db.clone();
        let scripts = Arc::clone(&self.scripts);
        let scripts_dir = self.scripts_dir.clone();
        let listen_addrs = self.listen_addrs.clone();
        let sessions = Arc::clone(&self.sessions);
        let map_server = self.map_server.clone();

        Box::pin(async move {
            match name.as_str() {
                "server_status" => {
                    let accounts = db.account_count().await.unwrap_or(-1);
                    let sessions_count = sessions.session_count().await;
                    Ok(serde_json::json!({
                        "status": "running",
                        "accounts": accounts,
                        "sessions": sessions_count,
                        "listen_addresses": listen_addrs,
                    }))
                }
                "list_accounts" => {
                    let list = db.list_accounts().await.unwrap_or_default();
                    Ok(serde_json::json!({ "accounts": list }))
                }
                "create_account" => {
                    let userid = args["userid"].as_str().unwrap_or("");
                    let password = args["password"].as_str().unwrap_or("");
                    let sex = args["sex"].as_str().unwrap_or("M");
                    if userid.is_empty() || password.is_empty() {
                        return Err(anyhow::anyhow!("userid 和 password 不能为空"));
                    }
                    match db.create_account(userid, password, sex).await {
                        Ok(id) => Ok(serde_json::json!({ "account_id": id })),
                        Err(e) => Err(anyhow::anyhow!("创建账户失败: {}", e)),
                    }
                }
                "create_script" => {
                    let script_name = args["name"].as_str().unwrap_or("");
                    let content = args["content"].as_str().unwrap_or("");
                    if script_name.is_empty() || content.is_empty() {
                        return Err(anyhow::anyhow!("name 和 content 不能为空"));
                    }
                    let path = scripts_dir.join(format!("{}.ro", script_name));
                    match std::fs::write(&path, content) {
                        Ok(()) => {
                            let result = match scripts.lock().unwrap().reload(&scripts_dir) {
                                Ok(()) => {
                                    info!("脚本 {} 已写入并热重载", path.display());
                                    serde_json::json!({ "path": path.to_string_lossy(), "reloaded": true })
                                }
                                Err(e) => {
                                    error!("脚本 {} 写入成功但热重载失败: {}", path.display(), e);
                                    serde_json::json!({ "path": path.to_string_lossy(), "reloaded": false, "error": e.to_string() })
                                }
                            };
                            Ok(result)
                        }
                        Err(e) => Err(anyhow::anyhow!("写入脚本失败: {}", e)),
                    }
                }
                "run_dsl_function" => {
                    let func_name = args["name"].as_str().unwrap_or("");
                    let dsl_args = args["args"].as_array().cloned().unwrap_or_default();
                    let values: Vec<Value> = dsl_args
                        .iter()
                        .map(|v| match v {
                            JsonValue::Number(n) => Value::Int(n.as_i64().unwrap_or(0)),
                            JsonValue::String(s) => Value::String(s.clone()),
                            JsonValue::Bool(b) => Value::Bool(*b),
                            _ => Value::Nil,
                        })
                        .collect();
                    match scripts.lock().unwrap().call(func_name, &values) {
                        Ok(result) => Ok(dsl_value_to_json(&result)),
                        Err(e) => Err(anyhow::anyhow!("DSL 调用失败: {}", e)),
                    }
                }
                "query_map_state" => {
                    let snapshot = map_server.map.snapshot();
                    Ok(snapshot)
                }
                "spawn_dynamic_npc" => {
                    let name = args["name"].as_str().unwrap_or("");
                    let x = args["x"].as_i64().unwrap_or(0) as i16;
                    let y = args["y"].as_i64().unwrap_or(0) as i16;
                    let sprite = args["sprite"].as_str().unwrap_or("");
                    if name.is_empty() {
                        return Err(anyhow::anyhow!("name 不能为空"));
                    }
                    match map_server.map.spawn_dynamic_npc(name, x, y, sprite) {
                        Some(id) => Ok(serde_json::json!({ "npc_id": id, "name": name, "x": x, "y": y })),
                        None => Err(anyhow::anyhow!("生成 NPC 失败")),
                    }
                }
                "describe_ontology" => {
                    let ontology = server_ontology();
                    Ok(serde_json::to_value(ontology)?)
                }
                "create_item" => {
                    let ontology = server_ontology();
                    if let Err(errors) = ontology.validate("Item", &args) {
                        return Err(anyhow::anyhow!("本体校验失败: {}", errors.join(", ")));
                    }
                    let item: Item = serde_json::from_value(args.clone())
                        .map_err(|e| anyhow::anyhow!("解析道具失败: {}", e))?;
                    let mut data = map_server.map.game_data_mut();
                    data.insert_item(item.clone());
                    Ok(serde_json::json!({
                        "id": item.id,
                        "aegis_name": item.aegis_name,
                        "name": item.name,
                        "item_count": data.item_count(),
                    }))
                }
                "create_equipment" => {
                    let ontology = server_ontology();
                    if let Err(errors) = ontology.validate("Equipment", &args) {
                        return Err(anyhow::anyhow!("本体校验失败: {}", errors.join(", ")));
                    }
                    let mut item: Item = serde_json::from_value(args.clone())
                        .map_err(|e| anyhow::anyhow!("解析装备失败: {}", e))?;
                    if item.item_type.is_none() {
                        item.item_type = Some("IT_WEAPON".to_string());
                    }
                    let mut data = map_server.map.game_data_mut();
                    data.insert_item(item.clone());
                    Ok(serde_json::json!({
                        "id": item.id,
                        "aegis_name": item.aegis_name,
                        "name": item.name,
                        "item_type": item.item_type,
                        "item_count": data.item_count(),
                    }))
                }
                "mutate_entity" => {
                    let class = args["class"].as_str().unwrap_or("");
                    if class.is_empty() {
                        return Err(anyhow::anyhow!("class 不能为空"));
                    }
                    let properties = args["properties"].as_object().cloned().unwrap_or_default();
                    if properties.is_empty() {
                        return Err(anyhow::anyhow!("properties 不能为空"));
                    }
                    match class {
                        "Item" => mutate_item(&map_server.map, &args["id"], &properties),
                        "Npc" => mutate_npc(&map_server.map, &args["id"], &properties),
                        "Monster" => mutate_monster(&map_server.map, &args["id"], &properties),
                        "Skill" => mutate_skill(&map_server.map, &args["id"], &properties),
                        _ => Err(anyhow::anyhow!("class {} 暂不支持运行时变更", class)),
                    }
                }
                _ => {
                    warn!("Agent 请求了未知工具: {}", name);
                    Err(anyhow::anyhow!("未知工具: {}", name))
                }
            }
        })
    }
}

pub fn default_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "server_status",
            "返回 Morroc 服务端的运行状态，包括账户数量、在线会话数和监听地址",
            Tool::empty_params(),
        ),
        Tool::new(
            "list_accounts",
            "列出所有已注册的账户用户名",
            Tool::empty_params(),
        ),
        Tool::new(
            "create_account",
            "创建新游戏账户",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "userid": { "type": "string", "description": "用户名" },
                    "password": { "type": "string", "description": "密码" },
                    "sex": { "type": "string", "description": "性别 M/F" }
                },
                "required": ["userid", "password"]
            }),
        ),
        Tool::new(
            "create_script",
            "创建或覆盖一个 .ro 脚本文件，并触发热重载",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "脚本文件名（不含 .ro）" },
                    "content": { "type": "string", "description": "DSL 脚本内容" }
                },
                "required": ["name", "content"]
            }),
        ),
        Tool::new(
            "run_dsl_function",
            "调用已加载的 DSL 函数或事件",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "函数或事件名" },
                    "args": { "type": "array", "description": "参数列表，支持整数、字符串、布尔" }
                },
                "required": ["name"]
            }),
        ),
        Tool::new(
            "query_map_state",
            "查询当前地图状态，包括玩家、NPC、怪物数量与 WoE 区域归属",
            Tool::empty_params(),
        ),
        Tool::new(
            "spawn_dynamic_npc",
            "在当前地图动态生成一个 NPC",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "NPC 名称" },
                    "x": { "type": "integer", "description": "X 坐标" },
                    "y": { "type": "integer", "description": "Y 坐标" },
                    "sprite": { "type": "string", "description": "NPC 精灵名称" }
                },
                "required": ["name", "x", "y"]
            }),
        ),
        Tool::new(
            "describe_ontology",
            "返回 Morroc 服务器本体论 schema，包括可操作的实体类、属性与关系",
            Tool::empty_params(),
        ),
        Tool::new(
            "create_item",
            "在运行时数据库中创建或覆盖一个通用道具",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "道具 ID" },
                    "aegis_name": { "type": "string", "description": "Aegis 名称" },
                    "name": { "type": "string", "description": "显示名称" },
                    "item_type": { "type": "string", "description": "道具类型" },
                    "buy": { "type": "integer", "description": "购买价格" },
                    "sell": { "type": "integer", "description": "出售价格" },
                    "weight": { "type": "integer", "description": "重量" },
                    "atk": { "type": "integer", "description": "物理攻击力" },
                    "matk": { "type": "integer", "description": "魔法攻击力" },
                    "def": { "type": "integer", "description": "防御力" },
                    "range": { "type": "integer", "description": "攻击范围" },
                    "slots": { "type": "integer", "description": "插槽数" },
                    "weapon_level": { "type": "integer", "description": "武器等级" },
                    "equip_level": { "type": "integer", "description": "装备等级" },
                    "refine": { "type": "boolean", "description": "是否可精炼" },
                    "script": { "type": "string", "description": "使用脚本" },
                    "on_equip_script": { "type": "string", "description": "装备脚本" },
                    "on_unequip_script": { "type": "string", "description": "卸下脚本" }
                },
                "required": ["id", "aegis_name", "name"]
            }),
        ),
        Tool::new(
            "create_equipment",
            "在运行时数据库中创建或覆盖一件装备（会自动设置默认 item_type 为 IT_WEAPON）",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "装备 ID" },
                    "aegis_name": { "type": "string", "description": "Aegis 名称" },
                    "name": { "type": "string", "description": "显示名称" },
                    "equip_level": { "type": "integer", "description": "装备等级" },
                    "atk": { "type": "integer", "description": "物理攻击力" },
                    "matk": { "type": "integer", "description": "魔法攻击力" },
                    "def": { "type": "integer", "description": "防御力" },
                    "slots": { "type": "integer", "description": "插槽数" },
                    "refine": { "type": "boolean", "description": "是否可精炼" },
                    "on_equip_script": { "type": "string", "description": "装备脚本" },
                    "on_unequip_script": { "type": "string", "description": "卸下脚本" }
                },
                "required": ["id", "aegis_name", "name"]
            }),
        ),
        Tool::new(
            "mutate_entity",
            "对运行时实体做增量修改（当前支持 Item、Npc、Monster、Skill）",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "class": { "type": "string", "description": "实体类名（Item/Npc/Monster/Skill）" },
                    "id": { "type": "integer", "description": "实体 ID" },
                    "properties": { "type": "object", "description": "要修改的属性键值对" }
                },
                "required": ["class", "id", "properties"]
            }),
        ),
    ]
}

fn dsl_value_to_json(value: &Value) -> JsonValue {
    match value {
        Value::Int(n) => JsonValue::Number((*n).into()),
        Value::Bool(b) => JsonValue::Bool(*b),
        Value::String(s) => JsonValue::String(s.clone()),
        Value::Nil => JsonValue::Null,
    }
}

fn mutate_item(
    map: &morroc_map::MapInstance,
    id_value: &JsonValue,
    properties: &JsonMap<String, JsonValue>,
) -> anyhow::Result<JsonValue> {
    let id = id_value.as_i64().ok_or_else(|| anyhow::anyhow!("id 应为整数"))?;
    let mut data = map.game_data_mut();
    let updated = data.update_item(id, |item| {
        for (k, v) in properties {
            match k.as_str() {
                "aegis_name" => {
                    if let Some(s) = v.as_str() {
                        item.aegis_name = s.to_string();
                    }
                }
                "name" => {
                    if let Some(s) = v.as_str() {
                        item.name = s.to_string();
                    }
                }
                "item_type" => item.item_type = v.as_str().map(|s| s.to_string()),
                "buy" => item.buy = v.as_i64(),
                "sell" => item.sell = v.as_i64(),
                "weight" => item.weight = v.as_i64(),
                "atk" => item.atk = v.as_i64(),
                "matk" => item.matk = v.as_i64(),
                "def" => item.def = v.as_i64(),
                "range" => item.range = v.as_i64(),
                "slots" => item.slots = v.as_i64(),
                "weapon_level" => item.weapon_level = v.as_i64(),
                "equip_level" => item.equip_level = v.as_i64(),
                "refine" => item.refine = v.as_bool(),
                "script" => item.script = v.as_str().map(|s| s.to_string()),
                "on_equip_script" => item.on_equip_script = v.as_str().map(|s| s.to_string()),
                "on_unequip_script" => item.on_unequip_script = v.as_str().map(|s| s.to_string()),
                _ => {
                    item.extra.insert(k.clone(), v.clone());
                }
            }
        }
    });
    if !updated {
        return Err(anyhow::anyhow!("道具 {} 不存在", id));
    }
    Ok(serde_json::json!({
        "class": "Item",
        "id": id,
        "updated": true,
        "item_count": data.item_count(),
    }))
}

fn mutate_npc(
    map: &morroc_map::MapInstance,
    id_value: &JsonValue,
    properties: &JsonMap<String, JsonValue>,
) -> anyhow::Result<JsonValue> {
    let id = id_value
        .as_i64()
        .ok_or_else(|| anyhow::anyhow!("id 应为整数"))? as u32;
    let x = properties
        .get("x")
        .and_then(|v| v.as_i64())
        .map(|n| n as i16);
    let y = properties
        .get("y")
        .and_then(|v| v.as_i64())
        .map(|n| n as i16);
    let updated = map.update_npc(id, |npc| {
        if let Some(name) = properties.get("name").and_then(|v| v.as_str()) {
            npc.entity.name = morroc_packets::map::name_bytes(name);
        }
        if let Some(x) = x {
            npc.entity.x = x;
        }
        if let Some(y) = y {
            npc.entity.y = y;
        }
    });
    if !updated {
        return Err(anyhow::anyhow!("NPC {} 不存在", id));
    }
    Ok(serde_json::json!({
        "class": "Npc",
        "id": id,
        "updated": true,
        "x": x,
        "y": y,
    }))
}

fn mutate_monster(
    map: &morroc_map::MapInstance,
    id_value: &JsonValue,
    properties: &JsonMap<String, JsonValue>,
) -> anyhow::Result<JsonValue> {
    let id = id_value
        .as_i64()
        .ok_or_else(|| anyhow::anyhow!("id 应为整数"))? as u32;
    let x = properties
        .get("x")
        .and_then(|v| v.as_i64())
        .map(|n| n as i16);
    let y = properties
        .get("y")
        .and_then(|v| v.as_i64())
        .map(|n| n as i16);
    let updated = map.update_monster(id, |mob| {
        if let Some(x) = x {
            mob.entity.x = x;
        }
        if let Some(y) = y {
            mob.entity.y = y;
        }
        if let Some(hp) = properties.get("hp").and_then(|v| v.as_i64()) {
            mob.entity.hp = hp as i32;
        }
    });
    if !updated {
        return Err(anyhow::anyhow!("怪物 {} 不存在", id));
    }
    Ok(serde_json::json!({
        "class": "Monster",
        "id": id,
        "updated": true,
        "x": x,
        "y": y,
    }))
}

fn mutate_skill(
    map: &morroc_map::MapInstance,
    id_value: &JsonValue,
    properties: &JsonMap<String, JsonValue>,
) -> anyhow::Result<JsonValue> {
    let id = id_value
        .as_i64()
        .ok_or_else(|| anyhow::anyhow!("id 应为整数"))? as u16;
    let name = properties
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return Err(anyhow::anyhow!("技能 name 不能为空"));
    }
    let skill = morroc_map::combat::SkillInfo {
        id,
        name,
        max_level: properties
            .get("max_level")
            .and_then(|v| v.as_i64())
            .map(|n| n as i16)
            .unwrap_or(1),
        element: properties
            .get("element")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        attack_type: properties
            .get("attack_type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        damage_factor: properties
            .get("damage_factor")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0),
        ..Default::default()
    };
    let mut data = map.game_data_mut();
    data.insert_skill(skill);
    Ok(serde_json::json!({
        "class": "Skill",
        "id": id,
        "updated": true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tool_server_status() {
        let db = morroc_db::Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        let scripts = Arc::new(Mutex::new(
            ScriptRuntime::load(Path::new("scripts")).await.unwrap(),
        ));
        let ctx = AgentContext::new(
            db,
            scripts,
            "scripts",
            vec!["127.0.0.1:6900".to_string()],
            Arc::new(morroc_db::LocalSessionStore::new()),
            morroc_map::MapServer::new_empty_sessions("127.0.0.1:0".parse().unwrap()),
        );
        let result = ctx
            .execute("server_status", &JsonValue::Null)
            .await
            .unwrap();
        assert_eq!(result["accounts"], 1);
        assert_eq!(result["status"], "running");
    }

    #[tokio::test]
    async fn tool_create_account_changes_db_state() {
        let db = morroc_db::Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        assert_eq!(db.account_count().await.unwrap(), 1);

        let scripts = Arc::new(Mutex::new(
            ScriptRuntime::load(Path::new("scripts")).await.unwrap(),
        ));
        let ctx = AgentContext::new(
            db.clone(),
            scripts,
            "scripts",
            vec![],
            Arc::new(morroc_db::LocalSessionStore::new()),
            morroc_map::MapServer::new_empty_sessions("127.0.0.1:0".parse().unwrap()),
        );

        let args = serde_json::json!({
            "userid": "gm_test_user",
            "password": "secret",
            "sex": "M"
        });
        let result = ctx.execute("create_account", &args).await.unwrap();
        assert!(result["account_id"].as_i64().is_some());

        assert_eq!(db.account_count().await.unwrap(), 2);
        let accounts = db.list_accounts().await.unwrap();
        assert!(accounts.contains(&"gm_test_user".to_string()));
    }

    #[tokio::test]
    async fn tool_create_script_changes_runtime_state() {
        let dir = tempfile::tempdir().unwrap();
        let db = morroc_db::Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        let scripts = Arc::new(Mutex::new(ScriptRuntime::load(dir.path()).await.unwrap()));
        let ctx = AgentContext::new(
            db,
            scripts,
            dir.path(),
            vec![],
            Arc::new(morroc_db::LocalSessionStore::new()),
            morroc_map::MapServer::new_empty_sessions("127.0.0.1:0".parse().unwrap()),
        );

        let args = serde_json::json!({
            "name": "gm_test_script",
            "content": "fn gm_test_answer() { return 42; }"
        });
        let result = ctx.execute("create_script", &args).await.unwrap();
        assert_eq!(result["reloaded"], true);

        let run_args = serde_json::json!({
            "name": "gm_test_answer",
            "args": []
        });
        let run_result = ctx.execute("run_dsl_function", &run_args).await.unwrap();
        assert_eq!(run_result, 42);
    }

    #[tokio::test]
    async fn tool_query_map_state_returns_snapshot() {
        let db = morroc_db::Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        let scripts = Arc::new(Mutex::new(
            ScriptRuntime::load(Path::new("scripts")).await.unwrap(),
        ));
        let map_server = morroc_map::MapServer::new_empty_sessions("127.0.0.1:0".parse().unwrap());
        let ctx = AgentContext::new(
            db,
            scripts,
            "scripts",
            vec![],
            Arc::new(morroc_db::LocalSessionStore::new()),
            map_server,
        );

        let result = ctx.execute("query_map_state", &JsonValue::Null).await.unwrap();
        assert_eq!(result["name"], "prontera");
        assert!(result["npc_count"].as_u64().unwrap() >= 2);
    }

    #[tokio::test]
    async fn tool_spawn_dynamic_npc_changes_map_state() {
        let db = morroc_db::Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        let scripts = Arc::new(Mutex::new(
            ScriptRuntime::load(Path::new("scripts")).await.unwrap(),
        ));
        let map_server = morroc_map::MapServer::new_empty_sessions("127.0.0.1:0".parse().unwrap());
        let ctx = AgentContext::new(
            db,
            scripts,
            "scripts",
            vec![],
            Arc::new(morroc_db::LocalSessionStore::new()),
            map_server.clone(),
        );

        let before = ctx.execute("query_map_state", &JsonValue::Null).await.unwrap();
        let before_npcs = before["npc_count"].as_u64().unwrap();

        let args = serde_json::json!({
            "name": "GM_NPC",
            "x": 150,
            "y": 180,
            "sprite": "4_M_KAFRA"
        });
        let result = ctx.execute("spawn_dynamic_npc", &args).await.unwrap();
        assert!(result["npc_id"].as_u64().is_some());

        let after = ctx.execute("query_map_state", &JsonValue::Null).await.unwrap();
        assert_eq!(after["npc_count"].as_u64().unwrap(), before_npcs + 1);
        assert!(after["npcs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n == "GM_NPC"));
    }

    #[tokio::test]
    async fn tool_describe_ontology_returns_schema() {
        let db = morroc_db::Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        let scripts = Arc::new(Mutex::new(
            ScriptRuntime::load(Path::new("scripts")).await.unwrap(),
        ));
        let ctx = AgentContext::new(
            db,
            scripts,
            "scripts",
            vec![],
            Arc::new(morroc_db::LocalSessionStore::new()),
            morroc_map::MapServer::new_empty_sessions("127.0.0.1:0".parse().unwrap()),
        );
        let result = ctx.execute("describe_ontology", &JsonValue::Null).await.unwrap();
        let classes = result["classes"].as_array().unwrap();
        assert!(classes.iter().any(|c| c["name"] == "Item"));
        assert!(classes.iter().any(|c| c["name"] == "Npc"));
        assert!(classes.iter().any(|c| c["name"] == "Player"));
    }

    #[tokio::test]
    async fn tool_create_item_changes_game_data() {
        let db = morroc_db::Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        let scripts = Arc::new(Mutex::new(
            ScriptRuntime::load(Path::new("scripts")).await.unwrap(),
        ));
        let map_server = morroc_map::MapServer::new_empty_sessions("127.0.0.1:0".parse().unwrap());
        let ctx = AgentContext::new(
            db,
            scripts,
            "scripts",
            vec![],
            Arc::new(morroc_db::LocalSessionStore::new()),
            map_server.clone(),
        );

        let before = ctx.execute("query_map_state", &JsonValue::Null).await.unwrap();
        assert_eq!(before["item_count"].as_u64().unwrap(), 0);

        let args = serde_json::json!({
            "id": 99001,
            "aegis_name": "GM_Dagger",
            "name": "GM Dagger",
            "item_type": "IT_WEAPON",
            "atk": 100,
            "refine": true
        });
        let result = ctx.execute("create_item", &args).await.unwrap();
        assert_eq!(result["id"], 99001);
        assert_eq!(result["item_count"], 1);

        let after = ctx.execute("query_map_state", &JsonValue::Null).await.unwrap();
        assert_eq!(after["item_count"].as_u64().unwrap(), 1);

        let data = map_server.map.game_data();
        let item = data.get_item(99001).unwrap();
        assert_eq!(item.aegis_name, "GM_Dagger");
        assert_eq!(item.atk, Some(100));
    }

    #[tokio::test]
    async fn tool_create_equipment_changes_game_data() {
        let db = morroc_db::Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        let scripts = Arc::new(Mutex::new(
            ScriptRuntime::load(Path::new("scripts")).await.unwrap(),
        ));
        let map_server = morroc_map::MapServer::new_empty_sessions("127.0.0.1:0".parse().unwrap());
        let ctx = AgentContext::new(
            db,
            scripts,
            "scripts",
            vec![],
            Arc::new(morroc_db::LocalSessionStore::new()),
            map_server.clone(),
        );

        let args = serde_json::json!({
            "id": 99002,
            "aegis_name": "GM_Armor",
            "name": "GM Armor",
            "def": 50
        });
        let result = ctx.execute("create_equipment", &args).await.unwrap();
        assert_eq!(result["item_type"], "IT_WEAPON");
        assert_eq!(result["item_count"], 1);

        let data = map_server.map.game_data();
        let item = data.get_item(99002).unwrap();
        assert_eq!(item.def, Some(50));
    }

    #[tokio::test]
    async fn tool_mutate_entity_changes_item_and_npc() {
        let db = morroc_db::Database::connect(":memory:").await.unwrap();
        db.migrate().await.unwrap();
        let scripts = Arc::new(Mutex::new(
            ScriptRuntime::load(Path::new("scripts")).await.unwrap(),
        ));
        let map_server = morroc_map::MapServer::new_empty_sessions("127.0.0.1:0".parse().unwrap());
        let ctx = AgentContext::new(
            db,
            scripts,
            "scripts",
            vec![],
            Arc::new(morroc_db::LocalSessionStore::new()),
            map_server.clone(),
        );

        ctx.execute(
            "create_item",
            &serde_json::json!({
                "id": 99003,
                "aegis_name": "GM_Potion",
                "name": "GM Potion"
            }),
        )
        .await
        .unwrap();

        let item_args = serde_json::json!({
            "class": "Item",
            "id": 99003,
            "properties": {
                "atk": 5,
                "buy": 1000
            }
        });
        ctx.execute("mutate_entity", &item_args).await.unwrap();
        let data = map_server.map.game_data();
        let item = data.get_item(99003).unwrap();
        assert_eq!(item.atk, Some(5));
        assert_eq!(item.buy, Some(1000));

        let npc_args = serde_json::json!({
            "class": "Npc",
            "id": 1000,
            "properties": {
                "x": 160,
                "y": 190
            }
        });
        ctx.execute("mutate_entity", &npc_args).await.unwrap();
    }
}
