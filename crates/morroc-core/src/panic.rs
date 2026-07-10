//! 中文崩溃报告。
//!
//! 当发生 panic 时，捕获 backtrace 并写入 `crashes/crash-<timestamp>.txt`。

use backtrace::Backtrace;
use std::fs;
use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::error;

/// 安装自定义 panic hook。
pub fn install_hook() {
    std::panic::set_hook(Box::new(move |info| {
        let report = generate_report(info);
        // 先打印到 stderr，确保即使文件写入失败也能看到。
        eprintln!("{}", report);

        if let Err(e) = write_report(&report) {
            error!("无法写入崩溃报告: {}", e);
        }
    }));
}

/// 生成带中文说明的崩溃报告文本。
#[allow(clippy::incompatible_msrv)]
fn generate_report(info: &PanicHookInfo<'_>) -> String {
    let location = info
        .location()
        .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
        .unwrap_or_else(|| "未知位置".to_string());

    let message = if let Some(s) = info.payload().downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = info.payload().downcast_ref::<&str>() {
        s.to_string()
    } else {
        "未知 panic 信息".to_string()
    };

    let bt = Backtrace::new();
    let mut frames = String::new();

    for (idx, frame) in bt.frames().iter().enumerate() {
        for symbol in frame.symbols() {
            let name = symbol
                .name()
                .map(|n| n.to_string())
                .unwrap_or_else(|| "???".to_string());
            let file = symbol
                .filename()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "??".to_string());
            let line = symbol
                .lineno()
                .map(|l| format!("{}", l))
                .unwrap_or_else(|| "??".to_string());

            frames.push_str(&format!("  #{:<2} {}  ({}:{})", idx, name, file, line));
            frames.push('\n');
        }
    }

    format!(
        "==================== Morroc 崩溃报告 ====================\n\
         发生时间：{}\n\
         崩溃位置：{}\n\
         错误信息：{}\n\
         \n\
         调用栈（函数 -> 文件:行号）：\n{}\n\
         ========================================================\n\
         请将此文件提交给开发团队以便定位问题。\n",
        format_time(),
        location,
        message,
        frames
    )
}

/// 将报告写入 `crashes/` 目录。
///
/// 可通过 `MORROC_CRASH_DIR` 环境变量覆盖输出目录，便于测试。
fn write_report(report: &str) -> std::io::Result<()> {
    let dir = std::env::var("MORROC_CRASH_DIR").unwrap_or_else(|_| "crashes".to_string());
    let dir = Path::new(&dir);
    fs::create_dir_all(dir)?;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = dir.join(format!("crash-{}.txt", timestamp));

    let mut file = fs::File::create(path)?;
    file.write_all(report.as_bytes())?;
    file.flush()?;

    Ok(())
}

fn format_time() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 简单转换为北京时间 (UTC+8)。
    let secs = now + 8 * 3600;
    let days = secs / 86400;
    let rem = secs % 86400;
    let h = rem / 3600;
    let m = (rem % 3600) / 60;
    let s = rem % 60;

    format!("{} 日 {}:{:02}:{:02} (UTC+8)", days, h, m, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::catch_unwind;
    use std::path::Path;

    #[test]
    fn panic_writes_chinese_report_with_location() {
        let dir = Path::new("target/morroc-core-panic-test");
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir).unwrap();
        std::env::set_var("MORROC_CRASH_DIR", dir.as_os_str());
        install_hook();

        fn trigger_panic() {
            panic!("测试崩溃");
        }

        let result = catch_unwind(trigger_panic);
        assert!(result.is_err(), "panic 应被 catch_unwind 捕获");

        let entries: Vec<_> = fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).collect();
        assert!(
            !entries.is_empty(),
            "崩溃报告文件应被写入 {}",
            dir.display()
        );

        let report = fs::read_to_string(entries[0].path()).unwrap();
        assert!(report.contains("崩溃报告"), "缺少中文标题");
        assert!(report.contains("崩溃位置"), "缺少崩溃位置");
        assert!(report.contains("panic.rs:"), "应包含源文件和行号");
        assert!(report.contains("trigger_panic"), "应包含函数名");
        assert!(report.contains("测试崩溃"), "应包含 panic 信息");
    }
}
