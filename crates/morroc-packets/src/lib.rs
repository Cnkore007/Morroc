//! Morroc 包协议定义。
//!
//! 包长度表、密钥等数据以静态表形式维护，便于支持多版本切换。

pub mod char;
pub mod lengths;
pub mod login;
pub mod map;

pub use lengths::{packet_len, CRYPTO_KEYS};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ca_login_length() {
        // CA_LOGIN 是登录请求包，经典长度为 55。
        assert_eq!(packet_len(0x0064), Some(55));
    }

    #[test]
    fn unknown_packet_len() {
        assert_eq!(packet_len(0xffff), None);
    }
}
