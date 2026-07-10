//! Morroc 包协议定义。
//!
//! 包长度表、密钥等数据从 Hercules 源文件生成。

pub mod char;
pub mod login;
pub mod map;

include!(concat!(env!("OUT_DIR"), "/lengths.rs"));

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
