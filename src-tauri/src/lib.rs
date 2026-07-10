#![cfg_attr(mobile, tauri::mobile_entry_point)]

use morroc_core::config::RunMode;
use serde::Serialize;
use serde_json::Value;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tokio::process::Child;
use tracing::{error, info, Subscriber};
use tracing_subscriber::layer::Layer as LayerTrait;
use tracing_subscriber::{prelude::*, EnvFilter, Registry};

/// 服务端控制状态，包含当前运行的 ServiceManager / AppState 以及分布式子进程。
pub struct ServerState {
    manager: Mutex<Option<morroc_daemon::ServiceManager>>,
    app_state: Mutex<Option<morroc_daemon::AppState>>,
    mode: Mutex<RunMode>,
    start_time: Mutex<Option<std::time::Instant>>,
    distributed_children: Mutex<Vec<Child>>,
}

impl ServerState {
    fn new() -> Self {
        Self {
            manager: Mutex::new(None),
            app_state: Mutex::new(None),
            mode: Mutex::new(RunMode::Standalone),
            start_time: Mutex::new(None),
            distributed_children: Mutex::new(Vec::new()),
        }
    }

    fn is_running(&self) -> bool {
        self.manager.lock().unwrap().is_some()
            || !self.distributed_children.lock().unwrap().is_empty()
    }
}

#[derive(Clone, Serialize)]
struct ServerStatus {
    running: bool,
    mode: String,
    uptime_seconds: u64,
    accounts: i64,
    sessions: i64,
    addresses: Vec<String>,
}

#[derive(Clone, Serialize)]
struct SystemMetrics {
    cpu_percent: f32,
    total_memory_mb: u64,
    used_memory_mb: u64,
    memory_percent: f32,
}

fn build_status(state: &ServerState) -> ServerStatus {
    let running = state.is_running();
    let mode = *state.mode.lock().unwrap();
    let uptime_seconds = state
        .start_time
        .lock()
        .unwrap()
        .map(|t| t.elapsed().as_secs())
        .unwrap_or(0);
    let (accounts, sessions, addresses) =
        if let Some(app) = state.app_state.lock().unwrap().as_ref() {
            (0, 0, app.listen_addresses())
        } else {
            (0, 0, Vec::new())
        };
    ServerStatus {
        running,
        mode: format!("{:?}", mode).to_lowercase(),
        uptime_seconds,
        accounts,
        sessions,
        addresses,
    }
}

async fn fill_status_counts(state: &ServerState, status: &mut ServerStatus) {
    let app = state.app_state.lock().unwrap().clone();
    if let Some(app) = app {
        status.accounts = app.account_count().await.unwrap_or(0);
        status.sessions = app.session_count().await;
        status.addresses = app.listen_addresses();
    }
}

/// 返回服务端运行状态（含模式、在线数、监听地址等）。
#[tauri::command]
async fn get_server_status(state: tauri::State<'_, ServerState>) -> Result<ServerStatus, String> {
    let mut status = build_status(&state);
    fill_status_counts(&state, &mut status).await;
    Ok(status)
}

/// 返回服务端状态摘要。
#[tauri::command]
async fn get_status(state: tauri::State<'_, ServerState>) -> Result<Value, String> {
    let app_state = {
        let guard = state.app_state.lock().unwrap();
        guard.clone().ok_or("服务未运行")?
    };
    app_state.status_summary().await.map_err(|e| e.to_string())
}

fn parse_mode(mode: &str) -> RunMode {
    match mode {
        "distributed" => RunMode::Distributed,
        "headless" => RunMode::Headless,
        _ => RunMode::Standalone,
    }
}

fn dist_bin_path(name: &str) -> std::path::PathBuf {
    let dir = if cfg!(debug_assertions) {
        "target/debug"
    } else {
        "target/release"
    };
    let mut path = std::path::Path::new(dir).join(name);
    if !std::env::consts::EXE_SUFFIX.is_empty() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        path.set_file_name(format!("{}{}", name, std::env::consts::EXE_SUFFIX));
    }
    path
}

fn spawn_dist_bin(name: &str) -> Result<Child, String> {
    let path = dist_bin_path(name);
    if !path.exists() {
        return Err(format!("找不到分布式服务二进制: {}", path.display()));
    }
    tokio::process::Command::new(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("启动 {} 失败: {}", name, e))
}

async fn stop_server_inner(state: &ServerState) -> Result<(), String> {
    if let Some(manager) = state.manager.lock().unwrap().take() {
        manager.shutdown();
    }
    *state.app_state.lock().unwrap() = None;
    *state.start_time.lock().unwrap() = None;
    let mut children: Vec<Child> = {
        let mut guard = state.distributed_children.lock().unwrap();
        std::mem::take(&mut *guard)
    };
    for child in children.iter_mut() {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    Ok(())
}

/// 启动服务端（单实例或分布式）。
#[tauri::command]
async fn start_server(
    state: tauri::State<'_, ServerState>,
    mode: String,
) -> Result<ServerStatus, String> {
    stop_server_inner(&state).await?;
    let mode = parse_mode(&mode);
    *state.mode.lock().unwrap() = mode;

    match mode {
        RunMode::Standalone => match morroc_daemon::start_services().await {
            Ok(manager) => {
                let app_state = manager.state.clone();
                *state.manager.lock().unwrap() = Some(manager);
                *state.app_state.lock().unwrap() = Some(app_state);
                *state.start_time.lock().unwrap() = Some(std::time::Instant::now());
                info!("Morroc 单实例服务已启动");
            }
            Err(e) => return Err(format!("启动服务失败: {}", e)),
        },
        RunMode::Distributed | RunMode::Headless => {
            let mut children = Vec::new();
            children.push(spawn_dist_bin("message_broker")?);
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            children.push(spawn_dist_bin("login_server")?);
            children.push(spawn_dist_bin("char_server")?);
            children.push(spawn_dist_bin("map_server")?);
            *state.distributed_children.lock().unwrap() = children;
            *state.start_time.lock().unwrap() = Some(std::time::Instant::now());
            info!("Morroc 分布式服务已启动");
        }
    }

    let mut status = build_status(&state);
    fill_status_counts(&state, &mut status).await;
    Ok(status)
}

/// 停止服务端。
#[tauri::command]
async fn stop_server(state: tauri::State<'_, ServerState>) -> Result<ServerStatus, String> {
    stop_server_inner(&state).await?;
    Ok(build_status(&state))
}

/// 与内置 Agent 对话。
#[tauri::command]
async fn agent_chat(
    state: tauri::State<'_, ServerState>,
    message: String,
) -> Result<morroc_agent::ChatResponse, String> {
    let app_state = {
        let guard = state.app_state.lock().unwrap();
        guard.clone().ok_or("服务未运行")?
    };
    app_state
        .agent_chat(&message)
        .await
        .map_err(|e| e.to_string())
}

/// 列出所有 `.ro` 脚本文件名。
#[tauri::command]
fn list_scripts(state: tauri::State<'_, ServerState>) -> Result<Vec<String>, String> {
    let app_state = state
        .app_state
        .lock()
        .unwrap()
        .clone()
        .ok_or("服务未运行")?;
    app_state.list_scripts().map_err(|e| e.to_string())
}

/// 读取脚本内容。
#[tauri::command]
fn load_script(
    state: tauri::State<'_, ServerState>,
    name: String,
) -> Result<Option<String>, String> {
    let app_state = state
        .app_state
        .lock()
        .unwrap()
        .clone()
        .ok_or("服务未运行")?;
    app_state.load_script(&name).map_err(|e| e.to_string())
}

/// 保存脚本并触发热重载。
#[tauri::command]
fn save_script(
    state: tauri::State<'_, ServerState>,
    name: String,
    content: String,
) -> Result<(), String> {
    let app_state = state
        .app_state
        .lock()
        .unwrap()
        .clone()
        .ok_or("服务未运行")?;
    app_state
        .save_script(&name, &content)
        .map_err(|e| e.to_string())
}

/// 列出所有账户用户名。
#[tauri::command]
async fn list_accounts(state: tauri::State<'_, ServerState>) -> Result<Vec<String>, String> {
    let app_state = state
        .app_state
        .lock()
        .unwrap()
        .clone()
        .ok_or("服务未运行")?;
    app_state.list_accounts().await.map_err(|e| e.to_string())
}

/// 创建新账户。
#[tauri::command]
async fn create_account(
    state: tauri::State<'_, ServerState>,
    userid: String,
    password: String,
    sex: String,
) -> Result<i64, String> {
    let app_state = state
        .app_state
        .lock()
        .unwrap()
        .clone()
        .ok_or("服务未运行")?;
    app_state
        .create_account(&userid, &password, &sex)
        .await
        .map_err(|e| e.to_string())
}

/// 返回当前地图状态快照。
#[tauri::command]
fn get_map_state(state: tauri::State<'_, ServerState>) -> Result<Value, String> {
    let app_state = state
        .app_state
        .lock()
        .unwrap()
        .clone()
        .ok_or("服务未运行")?;
    Ok(app_state.map_state())
}

/// 返回 CPU / RAM 使用率。
#[tauri::command]
fn get_system_metrics() -> Result<SystemMetrics, String> {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_cpu_usage();
    let total = sys.total_memory();
    let used = sys.used_memory();
    let memory_percent = if total > 0 {
        (used as f32 / total as f32) * 100.0
    } else {
        0.0
    };
    Ok(SystemMetrics {
        cpu_percent: sys.global_cpu_usage(),
        total_memory_mb: total / 1024 / 1024,
        used_memory_mb: used / 1024 / 1024,
        memory_percent,
    })
}

/// 将 tracing 日志事件转发到 Tauri 前端的 `log` 事件。
struct TauriLogLayer {
    handle: tauri::AppHandle,
}

impl TauriLogLayer {
    fn new(handle: tauri::AppHandle) -> Self {
        Self { handle }
    }
}

impl<S> LayerTrait<S> for TauriLogLayer
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let (tag, _) = morroc_core::logging::tag_for_target(event.metadata().target());
        let line = format!("[{}] {} {}", tag, event.metadata().level(), visitor.message);
        let _ = self.handle.emit("log", line);
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    has_fields: bool,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else {
            if self.has_fields {
                self.message.push(' ');
            } else {
                self.has_fields = true;
                if !self.message.is_empty() {
                    self.message.push_str("; ");
                }
            }
            self.message
                .push_str(&format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.record_debug(field, &value);
        }
    }
}

pub fn run() {
    let context = tauri::generate_context!();
    let server_state = Arc::new(ServerState::new());
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            get_server_status,
            get_status,
            start_server,
            stop_server,
            get_system_metrics,
            get_map_state,
            agent_chat,
            list_scripts,
            load_script,
            save_script,
            list_accounts,
            create_account
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            // 在 Tauri UI 模式下初始化带前端日志流的订阅器。
            let _ = tracing::subscriber::set_global_default(
                Registry::default()
                    .with(morroc_core::logging::fmt_layer())
                    .with(
                        EnvFilter::try_from_default_env()
                            .unwrap_or_else(|_| EnvFilter::new("info")),
                    )
                    .with(TauriLogLayer::new(handle)),
            );

            let _handle = app.handle().clone();
            let state_for_setup = Arc::clone(&server_state);
            tauri::async_runtime::spawn(async move {
                match morroc_daemon::start_services().await {
                    Ok(manager) => {
                        let app_state = manager.state.clone();
                        *state_for_setup.manager.lock().unwrap() = Some(manager);
                        *state_for_setup.app_state.lock().unwrap() = Some(app_state);
                        info!("Morroc 服务端已在后台启动");
                    }
                    Err(e) => {
                        error!("启动 Morroc 服务失败: {}", e);
                    }
                }
            });

            app.manage(server_state);
            Ok(())
        })
        .build(context)
        .expect("构建 Tauri 应用失败");

    app.run(|handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            if let Some(state) = handle.try_state::<Arc<ServerState>>() {
                let state = state.inner();
                let _ = tauri::async_runtime::block_on(stop_server_inner(state));
                info!("Morroc 服务端已收到关闭请求");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_metrics_returns_sane_values() {
        let metrics = get_system_metrics().expect("应能获取系统指标");
        assert!(metrics.total_memory_mb > 0, "总内存应大于 0");
        assert!(
            metrics.used_memory_mb <= metrics.total_memory_mb,
            "已用内存不应超过总内存"
        );
        assert!(
            (0.0..=100.0).contains(&metrics.memory_percent),
            "内存百分比应在 0-100 之间"
        );
    }

    #[test]
    fn server_state_defaults_to_stopped() {
        let state = ServerState::new();
        assert!(!state.is_running());
    }
}
