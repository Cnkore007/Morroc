//! Morroc 共享核心：错误类型、配置、日志、崩溃报告等基础设施。

pub mod config;
pub mod error;
pub mod logging;
pub mod panic;

pub use error::{Error, Result};
