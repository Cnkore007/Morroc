# Morroc

高性能 Ragnarok Online 服务端 Rust 重写版。单文件 Tauri 可执行程序，同时支持 headless 运行与 macOS 风格桌面 UI。

## 特性

- **原版 RO 客户端兼容**：基于 Hercules 20190530 main 包长度表实现登录、角色、地图三层协议。
- **单文件 SQLite**：彻底抛弃 MySQL，自动执行嵌入式迁移。
- **Headless + UI 双模式**：`morroc --headless` 无窗口运行；直接双击启动则进入 Tauri 桌面控制台。
- **自定义 DSL + 热重载**：`.ro` 脚本语法简洁，修改后自动重新编译。
- **内置远程 LLM Agent**：通过 OpenAI 兼容 API 分析服务端状态、管理账户、生成脚本/道具/怪物。
- **中文崩溃报告**：panic 时自动生成 `crashes/crash-<时间>.txt`。
- **彩色模块日志**：`tracing` 输出带模块标签的彩色日志。

## 运行

```bash
# 无窗口 headless 模式
cargo run --bin morroc -- --headless

# 桌面 UI 模式（需要 macOS/Windows/Linux 图形环境）
cargo run --bin morroc
```

## Agent 配置

Agent 默认启用本地 fallback。如需接入远程 LLM，设置环境变量：

```bash
export MORROC_AGENT_API_BASE=https://api.openai.com/v1
export MORROC_AGENT_API_KEY=sk-...
export MORROC_AGENT_MODEL=gpt-4o-mini
```

## 构建发布包

```bash
cargo install tauri-cli --version '^2.0.0'

# macOS universal .app + .dmg
cargo tauri build --target universal-apple-darwin

# Windows .msi
cargo tauri build
```

## 工作区结构

| Crate | 说明 |
|-------|------|
| `crates/morroc-core` | 日志、中文崩溃报告 |
| `crates/morroc-db` | SQLite 连接与迁移 |
| `crates/morroc-packets` | RO 包结构与长度表 |
| `crates/morroc-net` | TCP 包编解码 |
| `crates/morroc-login` | 登录服务器 |
| `crates/morroc-char` | 角色服务器 |
| `crates/morroc-map` | 地图服务器骨架 |
| `crates/morroc-dsl` | 自定义脚本 DSL 编译器与 VM |
| `crates/morroc-daemon` | headless / UI 共用服务启动与状态 |
| `crates/morroc-agent` | 远程 LLM Agent 客户端与 HTTP 服务 |
| `src-tauri` | Tauri 桌面应用与 UI 命令 |
| `ui` | macOS 风格前端 |

## 协议端口

| 服务 | 默认端口 |
|------|----------|
| 登录服务器 | 6900 |
| 角色服务器 | 6121 |
| 地图服务器 | 5121 |
| Agent HTTP | 3000 |

## 许可

MIT OR Apache-2.0
