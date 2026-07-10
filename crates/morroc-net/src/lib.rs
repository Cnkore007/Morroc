//! Morroc 网络层：TCP 会话、包编解码。
//!
//! 当前实现基于 Hercules 20190530 main 的包长度表。后续会扩展为多版本。

use bytes::{Buf, BufMut, BytesMut};
use futures::StreamExt;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{Decoder, Encoder, Framed};
use tracing::{error, info, warn};

/// 一个已解码的客户端包。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub packet_id: u16,
    pub payload: Vec<u8>,
}

impl Packet {
    /// 构造一个包（payload 不包含 packet_id）。
    pub fn new(packet_id: u16, payload: Vec<u8>) -> Self {
        Self { packet_id, payload }
    }
}

/// RO 包协议编解码器。
///
/// 包格式：前 2 字节小端序 packet_id，后续长度由长度表决定。
/// 动态长度包（表项为 -1）在 packet_id 后紧跟 2 字节小端总长度。
pub struct PacketCodec;

impl Decoder for PacketCodec {
    type Item = Packet;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 2 {
            return Ok(None);
        }

        let packet_id = u16::from_le_bytes([src[0], src[1]]);
        let len = morroc_packets::packet_len(packet_id).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("未知包 ID: 0x{:04x}", packet_id),
            )
        })?;

        let total_len = if len == -1 {
            // 动态长度包：packet_id(2) + packet_len(2) + ...
            if src.len() < 4 {
                return Ok(None);
            }
            u16::from_le_bytes([src[2], src[3]]) as usize
        } else {
            len as usize
        };

        if total_len < 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("包长度异常: 0x{:04x} = {}", packet_id, total_len),
            ));
        }

        if src.len() < total_len {
            // 预留足够空间，避免反复扩容。
            src.reserve(total_len - src.len());
            return Ok(None);
        }

        let mut buf = src.split_to(total_len);
        buf.advance(2); // 跳过 packet_id
        if len == -1 {
            buf.advance(2); // 跳过动态长度字段
        }
        let payload = buf.to_vec();

        Ok(Some(Packet { packet_id, payload }))
    }
}

impl Encoder<Packet> for PacketCodec {
    type Error = io::Error;

    fn encode(&mut self, item: Packet, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let total_len = 2 + item.payload.len();
        dst.reserve(total_len);
        dst.put_u16_le(item.packet_id);
        dst.extend_from_slice(&item.payload);
        Ok(())
    }
}

/// TCP 会话，对应一个客户端连接。
pub type FramedSession = Framed<TcpStream, PacketCodec>;

/// 启动 TCP 服务器，使用自定义 handler 处理每个连接。
pub async fn serve_with<H, Fut>(addr: SocketAddr, handler: H) -> anyhow::Result<()>
where
    H: Fn(FramedSession, SocketAddr) -> Fut + Send + Clone + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let listener = TcpListener::bind(addr).await?;
    serve_with_listener(listener, handler).await
}

/// 使用已绑定的 TCP 监听器启动服务器。
pub async fn serve_with_listener<H, Fut>(listener: TcpListener, handler: H) -> anyhow::Result<()>
where
    H: Fn(FramedSession, SocketAddr) -> Fut + Send + Clone + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(v) => v,
            Err(e) => {
                error!("接受连接失败: {}", e);
                continue;
            }
        };

        let handler = handler.clone();
        tokio::spawn(async move {
            let framed = Framed::new(stream, PacketCodec);
            if let Err(e) = handler(framed, peer).await {
                warn!("客户端 {} 处理异常: {}", peer, e);
            }
        });
    }
}

/// 启动 TCP 服务器，对每个连接读取并打印包。
pub async fn serve(addr: SocketAddr) -> anyhow::Result<()> {
    serve_with(addr, |mut framed, peer| async move {
        info!("客户端连接: {}", peer);

        while let Some(result) = framed.next().await {
            match result {
                Ok(packet) => {
                    info!(
                        "收到包 0x{:04x}，来自 {}，payload {} 字节",
                        packet.packet_id,
                        peer,
                        packet.payload.len()
                    );
                }
                Err(e) => {
                    warn!("解析客户端 {} 数据失败: {}", peer, e);
                    break;
                }
            }
        }

        info!("客户端断开: {}", peer);
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_ca_login() {
        // CA_LOGIN 包 ID 0x0064，长度 55。
        let mut buf = BytesMut::from(&[0x64, 0x00][..]);
        buf.extend_from_slice(&[0u8; 53]);

        let mut codec = PacketCodec;
        let packet = codec.decode(&mut buf).unwrap().unwrap();
        assert_eq!(packet.packet_id, 0x0064);
        assert_eq!(packet.payload.len(), 53);
    }

    #[test]
    fn encode_packet() {
        let packet = Packet::new(0x1234, vec![1, 2, 3]);
        let mut codec = PacketCodec;
        let mut buf = BytesMut::new();
        codec.encode(packet, &mut buf).unwrap();
        assert_eq!(buf.len(), 5);
        assert_eq!(&buf[..2], &[0x34, 0x12]);
        assert_eq!(&buf[2..], &[1, 2, 3]);
    }
}
