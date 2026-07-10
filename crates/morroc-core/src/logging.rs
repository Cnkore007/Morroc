//! 日志初始化与彩色模块标签。
//!
//! 默认输出格式会在每条日志前加上 `[Login]` / `[Char]` / `[Map]` 等彩色标签，
//! 便于在多模块服务端中快速区分日志来源。

use std::fmt;
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::format::{DefaultFields, Format, Writer};
use tracing_subscriber::fmt::{FmtContext, FormatEvent};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{layer::Layer as LayerTrait, prelude::*, EnvFilter, Registry};

/// 自定义事件格式化器：在默认格式前输出彩色模块标签。
#[derive(Debug, Clone, Default)]
pub struct ModuleTagFormatter;

impl<S> FormatEvent<S, DefaultFields> for ModuleTagFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, DefaultFields>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let (tag, color) = tag_for_target(event.metadata().target());
        if writer.has_ansi_escapes() {
            write!(writer, "\x1b[{}m[{}]\x1b[0m ", color, tag)?;
        } else {
            write!(writer, "[{}] ", tag)?;
        }
        Format::default().format_event(ctx, writer, event)
    }
}

/// 根据日志目标路径返回模块标签与 ANSI 颜色码。
pub fn tag_for_target(target: &str) -> (&'static str, &'static str) {
    match target.split("::").next() {
        Some("morroc_login") => ("Login", "34"),   // blue
        Some("morroc_char") => ("Char", "32"),     // green
        Some("morroc_map") => ("Map", "33"),       // yellow
        Some("morroc_daemon") => ("Daemon", "35"), // magenta
        Some("morroc_agent") => ("Agent", "36"),   // cyan
        Some("morroc_core") => ("Core", "37"),     // white
        Some("morroc_db") => ("Db", "90"),         // bright black
        Some("morroc_net") => ("Net", "94"),       // bright blue
        Some("morroc_dsl") => ("Dsl", "96"),       // bright cyan
        Some("morroc_converter") => ("Converter", "95"),
        Some("morroc_lib") | Some("morroc") => ("App", "93"),
        _ => ("App", "93"),
    }
}

/// 返回默认的彩色模块标签 fmt 层。
///
/// 可直接与 `Registry` 组合；调用方自行决定是否追加其他层（例如 Tauri 日志流）。
pub fn fmt_layer() -> impl LayerTrait<Registry> {
    tracing_subscriber::fmt::Layer::default()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .with_level(true)
        .with_ansi(true)
        .event_format(ModuleTagFormatter)
}

/// 初始化全局 tracing 订阅器。
///
/// 输出格式包含彩色模块标签、时间、日志级别与消息。
/// 环境变量 `RUST_LOG` 可控制级别，例如 `RUST_LOG=info,morroc=debug`。
pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    Registry::default().with(fmt_layer()).with(filter).init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_for_known_modules() {
        assert_eq!(tag_for_target("morroc_login::lib"), ("Login", "34"));
        assert_eq!(tag_for_target("morroc_char::server"), ("Char", "32"));
        assert_eq!(tag_for_target("morroc_map::entity"), ("Map", "33"));
        assert_eq!(tag_for_target("morroc_daemon::agent"), ("Daemon", "35"));
        assert_eq!(tag_for_target("morroc_agent::chat"), ("Agent", "36"));
        assert_eq!(tag_for_target("unknown_crate::foo"), ("App", "93"));
    }
}
