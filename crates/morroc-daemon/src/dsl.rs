//! Morroc DSL 运行时集成：加载脚本、热重载、暴露 native 函数。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use morroc_dsl::{compile, RuntimeError, Value, Vm};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// 脚本运行时。持有已编译的 VM 与 notify watcher，支持脚本热重载。
pub struct ScriptRuntime {
    vm: Arc<Mutex<Vm>>,
    _watcher: RecommendedWatcher,
}

impl ScriptRuntime {
    /// 从 `scripts_dir` 加载所有 `.ro` 脚本，并启动文件监控。
    pub async fn load(scripts_dir: &Path) -> Result<Self> {
        info!("开始加载脚本...");
        let vm = Arc::new(Mutex::new(Self::build_vm(scripts_dir)?));
        info!("脚本编译完成，启动 watcher...");
        let (tx, mut rx) = mpsc::channel::<notify::Result<Event>>(32);
        let scripts_dir = scripts_dir.to_path_buf();
        let vm_clone = Arc::clone(&vm);

        info!("创建 notify watcher...");
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Err(e) = tx.try_send(res) {
                tracing::debug!("脚本 watcher 事件发送失败: {}", e);
            }
        })
        .context("创建 notify watcher 失败")?;
        info!("notify watcher 创建完成，开始监听...");

        watcher
            .watch(&scripts_dir, RecursiveMode::Recursive)
            .context("监听 scripts 目录失败")?;
        info!("notify watcher 监听已启动");

        tokio::spawn(async move {
            while let Some(res) = rx.recv().await {
                match res {
                    Ok(event) => {
                        if event.kind.is_modify()
                            || event.kind.is_create()
                            || event.kind.is_remove()
                        {
                            info!("检测到脚本变化，准备热重载...");
                            match Self::build_vm(&scripts_dir) {
                                Ok(new_vm) => {
                                    let mut guard = vm_clone.lock().unwrap();
                                    *guard = new_vm;
                                    info!("脚本热重载完成。");
                                }
                                Err(e) => error!("脚本热重载失败: {}", e),
                            }
                        }
                    }
                    Err(e) => warn!("notify 事件错误: {}", e),
                }
            }
        });

        Ok(Self {
            vm,
            _watcher: watcher,
        })
    }

    /// 调用一个 DSL 函数或事件。
    pub fn call(&self, name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
        let mut vm = self.vm.lock().unwrap();
        vm.call(name, args)
    }

    /// 立即重新编译 `scripts_dir` 下所有脚本并替换当前 VM。
    pub fn reload(&self, scripts_dir: &Path) -> Result<()> {
        let new_vm = Self::build_vm(scripts_dir)?;
        let mut vm = self.vm.lock().unwrap();
        *vm = new_vm;
        Ok(())
    }

    fn build_vm(scripts_dir: &Path) -> Result<Vm> {
        if !scripts_dir.exists() {
            std::fs::create_dir_all(scripts_dir)
                .with_context(|| format!("创建脚本目录失败: {}", scripts_dir.display()))?;
        }

        tracing::debug!("正在扫描脚本目录: {}", scripts_dir.display());
        let mut entries: Vec<PathBuf> = std::fs::read_dir(scripts_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("ro"))
            .collect();
        entries.sort();
        tracing::debug!("找到 {} 个脚本文件", entries.len());

        let mut combined = String::new();
        for path in &entries {
            let src = std::fs::read_to_string(path)
                .with_context(|| format!("读取脚本失败: {}", path.display()))?;
            combined.push_str(&src);
            combined.push('\n');
        }

        tracing::debug!("开始编译脚本...");
        let program = compile(&combined).context("编译脚本失败")?;
        tracing::debug!("脚本编译完成");
        let mut vm = Vm::with_program(program);
        register_natives(&mut vm);
        info!("已加载 {} 个脚本文件", entries.len());
        Ok(vm)
    }
}

fn register_natives(vm: &mut Vm) {
    vm.register_native(
        "print",
        Box::new(|_vm, args| {
            let msg = args
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            info!("[script] {}", msg);
            Ok(Value::Nil)
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[tokio::test]
    async fn loads_and_runs_script() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ro");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(b"fn double(x) { return x * 2; }").unwrap();

        let runtime = ScriptRuntime::load(dir.path()).await.unwrap();
        let result = runtime.call("double", &[Value::Int(21)]).unwrap();
        assert_eq!(result, Value::Int(42));
    }
}
