//! 全局错误类型。

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("配置解析错误: {0}")]
    Config(String),

    #[error("未知错误: {0}")]
    Unknown(String),
}
