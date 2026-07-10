//! Legacy NPC 脚本解析器。
//!
//! NPC 脚本格式：每行 tab 分隔为 4 个字段：
//! `<map>,<x>,<y>,<facing>\t<type>\t<name>\t<sprite>[,{<script body>}]`
//!
//! 支持的类型：warp, script, shop, trader, function

use std::collections::HashMap;

use crate::libconfig::ParseError;

use serde::{Deserialize, Serialize};

/// NPC 定义。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Npc {
    pub map: String,
    pub x: i16,
    pub y: i16,
    pub facing: u8,
    pub kind: NpcKind,
    pub name: String,
    pub sprite: String,
    pub body: Option<String>,
}

/// NPC 类型。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum NpcKind {
    /// warp
    Warp {
        x_size: i16,
        y_size: i16,
        to_map: String,
        to_x: i16,
        to_y: i16,
    },
    /// script
    Script,
    /// shop / trader
    Shop,
    /// function
    Function,
    /// unknown
    Unknown { value: String },
}

impl NpcKind {
    fn from_str(s: &str) -> Self {
        match s {
            "warp" => NpcKind::Warp {
                x_size: 0,
                y_size: 0,
                to_map: String::new(),
                to_x: 0,
                to_y: 0,
            },
            "script" => NpcKind::Script,
            "shop" => NpcKind::Shop,
            "trader" => NpcKind::Shop,
            "function" => NpcKind::Function,
            other => NpcKind::Unknown {
                value: other.to_string(),
            },
        }
    }
}

/// NPC 脚本文件解析结果。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NpcFile {
    pub npcs: Vec<Npc>,
    pub functions: HashMap<String, String>,
}

/// 解析单个 NPC 脚本文件内容。
pub fn parse(source: &str) -> Result<NpcFile, ParseError> {
    let mut file = NpcFile::default();
    let mut function_name: Option<String> = None;
    let mut function_body: Vec<String> = Vec::new();
    let mut brace_depth = 0;

    for (line_no, raw) in source.lines().enumerate() {
        let line = raw.trim_end();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }

        // 如果在 function 块内，收集直到块结束
        if function_name.is_some() {
            function_body.push(line.to_string());
            brace_depth += line.chars().filter(|c| *c == '{').count() as i32;
            brace_depth -= line.chars().filter(|c| *c == '}').count() as i32;
            if brace_depth == 0 {
                let name = function_name.take().unwrap();
                let body = function_body.join("\n");
                file.functions.insert(name, body);
                function_body.clear();
            }
            continue;
        }

        // function 声明：function\tname\t{...}
        // 也支持 map,x,y,facing\tfunction\tname\tbody 格式
        if let Some(remain) = line.strip_prefix("function\t") {
            let mut parts = remain.splitn(2, '\t');
            let name = parts.next().map(|s| s.trim()).unwrap_or("").to_string();
            let body = parts.next().unwrap_or("").to_string();
            brace_depth = body.chars().filter(|c| *c == '{').count() as i32
                - body.chars().filter(|c| *c == '}').count() as i32;
            if brace_depth == 0 {
                file.functions.insert(name, body);
            } else {
                function_name = Some(name);
                function_body.push(body);
            }
            continue;
        }

        // 支持 map,x,y,facing\tfunction\tname\tbody 格式
        let parts_for_function: Vec<&str> = line.split('\t').collect();
        if parts_for_function.len() >= 3 && parts_for_function[1].trim() == "function" {
            let name = parts_for_function[2].trim().to_string();
            let body_start = parts_for_function[3..].join("\t");
            brace_depth = body_start.chars().filter(|c| *c == '{').count() as i32
                - body_start.chars().filter(|c| *c == '}').count() as i32;
            if brace_depth == 0 {
                file.functions.insert(name, body_start);
            } else {
                function_name = Some(name);
                function_body.push(body_start);
            }
            continue;
        }

        // 普通 NPC 行
        match parse_line(line) {
            Ok(npc) => file.npcs.push(npc),
            Err(e) => {
                return Err(ParseError::Other(format!(
                    "第 {} 行解析失败: {} (原始: {})",
                    line_no + 1,
                    e,
                    line
                )))
            }
        }
    }

    Ok(file)
}

fn parse_line(line: &str) -> Result<Npc, String> {
    // 分块：w1\tw2\tw3\tw4
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 4 {
        return Err(format!("需要 4 个 tab 分隔字段，实际 {}", parts.len()));
    }

    let w1 = parts[0].trim();
    let w2 = parts[1].trim();
    let w3 = parts[2].trim();
    let w4 = parts[3].trim();

    // 解析 w1: map,x,y,facing
    let loc_parts: Vec<&str> = w1.splitn(4, ',').collect();
    if loc_parts.len() < 4 {
        return Err(format!("坐标格式错误: {}", w1));
    }
    let map = loc_parts[0].to_string();
    let x = loc_parts[1]
        .parse::<i16>()
        .map_err(|e| format!("x 解析失败: {}", e))?;
    let y = loc_parts[2]
        .parse::<i16>()
        .map_err(|e| format!("y 解析失败: {}", e))?;
    let facing = loc_parts[3]
        .parse::<u8>()
        .map_err(|e| format!("facing 解析失败: {}", e))?;

    let kind_str = w2;
    let mut name = w3.to_string();
    // name::duplicate 拆分
    if let Some(idx) = name.find("::") {
        name = name[..idx].to_string();
    }

    // 解析 w4：sprite 以及可选的脚本体
    let mut body = None;
    let mut kind = NpcKind::from_str(kind_str);

    let sprite = if let Some(idx) = w4.find(",{") {
        // 有脚本体：sprite,{...}
        body = Some(w4[idx + 1..].to_string());
        w4[..idx].to_string()
    } else if kind_str == "warp" {
        // warp: xsize,ysize,to_map,to_x,to_y (无 sprite)
        let warp_parts: Vec<&str> = w4.split(',').collect();
        if warp_parts.len() >= 5 {
            let x_size = warp_parts[0]
                .parse::<i16>()
                .map_err(|e| format!("warp x_size 解析失败: {}", e))?;
            let y_size = warp_parts[1]
                .parse::<i16>()
                .map_err(|e| format!("warp y_size 解析失败: {}", e))?;
            let to_map = warp_parts[2].to_string();
            let to_x = warp_parts[3]
                .parse::<i16>()
                .map_err(|e| format!("warp to_x 解析失败: {}", e))?;
            let to_y = warp_parts[4]
                .parse::<i16>()
                .map_err(|e| format!("warp to_y 解析失败: {}", e))?;
            kind = NpcKind::Warp {
                x_size,
                y_size,
                to_map,
                to_x,
                to_y,
            };
            "warp".to_string()
        } else {
            return Err(format!("warp 字段不足: {}", w4));
        }
    } else if kind_str == "shop" || kind_str == "trader" {
        // shop/trader：sprite 后可能有 { sellitem ... }
        w4.to_string()
    } else {
        // script 无脚本体：仅 sprite
        w4.to_string()
    };

    Ok(Npc {
        map,
        x,
        y,
        facing,
        kind,
        name,
        sprite,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_warp() {
        let line = "aldebaran,118,63,0\twarp\tald01\t1,1,aldeba_in,211,117";
        let npc = parse_line(line).unwrap();
        assert_eq!(npc.map, "aldebaran");
        assert_eq!(npc.x, 118);
        assert_eq!(npc.y, 63);
        assert_eq!(npc.name, "ald01");
        if let NpcKind::Warp {
            x_size,
            y_size,
            to_map,
            to_x,
            to_y,
        } = npc.kind
        {
            assert_eq!(x_size, 1);
            assert_eq!(y_size, 1);
            assert_eq!(to_map, "aldeba_in");
            assert_eq!(to_x, 211);
            assert_eq!(to_y, 117);
        } else {
            panic!("expected warp");
        }
    }

    #[test]
    fn parse_script() {
        let line =
            "alberta_in,165,96,0\tscript\tItem Collector#alb\t1_F_MERCHANT_02,{ mes \"Hi\"; }";
        let npc = parse_line(line).unwrap();
        assert_eq!(npc.kind, NpcKind::Script);
        assert_eq!(npc.name, "Item Collector#alb");
        assert_eq!(npc.sprite, "1_F_MERCHANT_02");
        assert!(npc.body.as_ref().unwrap().contains("mes"));
    }

    #[test]
    fn parse_file_with_function() {
        let source = r#"// sample
prontera,150,180,4	script	Guide	4_M_04,{ mes "Hello"; }
prontera,152,182,0	function	F_Help	{ mes "Help"; }
"#;
        let file = parse(source).unwrap();
        assert_eq!(file.npcs.len(), 1);
        assert_eq!(file.functions.len(), 1);
        assert!(file.functions.contains_key("F_Help"));
    }
}
