//! Morroc 运行时配置。

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 运行模式：standalone / distributed / headless。
    pub mode: RunMode,
    /// 数据库文件路径。
    pub database_path: String,
    /// 是否启用 GUI。
    pub gui: bool,
    /// 登录服务监听地址。
    pub login: ServerEndpoint,
    /// 角色服务监听地址。
    pub char: ServerEndpoint,
    /// 地图服务监听地址。
    pub map: ServerEndpoint,
    /// Agent 服务监听地址。
    pub agent: ServerEndpoint,
    /// 分布式消息总线监听地址。
    pub broker: ServerEndpoint,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    #[default]
    Standalone,
    Distributed,
    Headless,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerEndpoint {
    /// 监听地址，例如 `127.0.0.1:6900`。
    pub listen: String,
    /// 对外暴露的 IP，用于返回给客户端（小端序整数）。
    pub ip: String,
    /// 对外暴露的端口。
    pub port: u16,
}

impl Default for ServerEndpoint {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:0".to_string(),
            ip: "127.0.0.1".to_string(),
            port: 0,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: RunMode::Standalone,
            database_path: "data/morroc.db".to_string(),
            gui: true,
            login: ServerEndpoint {
                listen: "127.0.0.1:6900".to_string(),
                ip: "127.0.0.1".to_string(),
                port: 6900,
            },
            char: ServerEndpoint {
                listen: "127.0.0.1:6121".to_string(),
                ip: "127.0.0.1".to_string(),
                port: 6121,
            },
            map: ServerEndpoint {
                listen: "127.0.0.1:5121".to_string(),
                ip: "127.0.0.1".to_string(),
                port: 5121,
            },
            agent: ServerEndpoint {
                listen: "127.0.0.1:3000".to_string(),
                ip: "127.0.0.1".to_string(),
                port: 3000,
            },
            broker: ServerEndpoint {
                listen: "127.0.0.1:5999".to_string(),
                ip: "127.0.0.1".to_string(),
                port: 5999,
            },
        }
    }
}

impl Config {
    /// 从 TOML 字符串加载配置。
    pub fn from_toml(s: &str) -> Result<Self> {
        toml::from_str(s).map_err(|e| crate::Error::Config(e.to_string()))
    }

    /// 加载 `config/morroc.toml`，缺失的字段使用默认值填充。
    pub fn load() -> Result<Self> {
        let path = Path::new("config/morroc.toml");
        if !path.exists() {
            return Ok(Self::default());
        }
        let source = std::fs::read_to_string(path)
            .map_err(|e| crate::Error::Config(format!("读取配置文件失败: {}", e)))?;
        Self::from_toml(&source)
    }

    /// 返回将 IP 字符串解析为小端序整数的结果。
    pub fn ip_to_u32(ip: &str) -> Option<u32> {
        ip.parse::<std::net::Ipv4Addr>()
            .ok()
            .map(|addr| u32::from_le_bytes(addr.octets()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_known_ports() {
        let cfg = Config::default();
        assert_eq!(cfg.login.port, 6900);
        assert_eq!(cfg.char.port, 6121);
        assert_eq!(cfg.map.port, 5121);
    }

    #[test]
    fn parse_distributed_mode_from_toml() {
        let source = r#"
mode = "distributed"
database_path = "data/morroc.db"

[login]
listen = "0.0.0.0:6900"
ip = "192.168.1.10"
port = 6900

[char]
listen = "0.0.0.0:6121"
ip = "192.168.1.10"
port = 6121

[map]
listen = "0.0.0.0:5121"
ip = "192.168.1.10"
port = 5121
"#;
        let cfg = Config::from_toml(source).unwrap();
        assert_eq!(cfg.mode, RunMode::Distributed);
        assert_eq!(cfg.database_path, "data/morroc.db");
        assert_eq!(cfg.login.ip, "192.168.1.10");
        assert_eq!(Config::ip_to_u32(&cfg.login.ip), Some(0x0a01a8c0));
    }

    #[test]
    fn ip_to_u32_localhost() {
        assert_eq!(Config::ip_to_u32("127.0.0.1"), Some(0x0100007f));
    }
}
