//! 最小分布式消息总线。
//!
//! 提供基于 TCP 的发布/订阅消息转发，使 distributed 模式下的 login/char/map
//! 等服务可以通过 TCP 相互通信。standalone 模式仍使用进程内直接调用，不经过此总线。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;
use tracing::{info, warn};

/// 总线消息类型。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    /// 客户端订阅一组主题。
    Subscribe { topics: Vec<String> },
    /// 向某个主题发布消息。
    Publish { topic: String, payload: Value },
    /// 定向发送到某个服务标识。
    Direct { target: String, payload: Value },
    /// 服务端转发给订阅者的消息。
    Forward { topic: String, payload: Value },
}

type ClientId = usize;

/// 消息总线，维护客户端连接与订阅关系。
pub struct MessageBroker {
    listener: TcpListener,
    clients: Arc<AsyncMutex<HashMap<ClientId, mpsc::UnboundedSender<Message>>>>,
    subscriptions: Arc<AsyncMutex<HashMap<String, HashSet<ClientId>>>>,
    next_id: AtomicUsize,
}

impl MessageBroker {
    /// 在指定地址启动消息总线。
    pub async fn new(addr: SocketAddr) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(addr).await?;
        Ok(Self {
            listener,
            clients: Arc::new(AsyncMutex::new(HashMap::new())),
            subscriptions: Arc::new(AsyncMutex::new(HashMap::new())),
            next_id: AtomicUsize::new(1),
        })
    }

    /// 返回实际监听地址。
    pub fn local_addr(&self) -> anyhow::Result<SocketAddr> {
        self.listener.local_addr().map_err(|e| e.into())
    }

    /// 运行总线，接受连接并转发消息。
    pub async fn run(self) -> anyhow::Result<()> {
        let addr = self.local_addr()?;
        info!("分布式消息总线已启动: {}", addr);
        loop {
            let (stream, peer) = self.listener.accept().await?;
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let clients = Arc::clone(&self.clients);
            let subscriptions = Arc::clone(&self.subscriptions);
            let (tx, rx) = mpsc::unbounded_channel();
            {
                let mut clients = self.clients.lock().await;
                clients.insert(id, tx);
            }
            tokio::spawn(async move {
                info!("消息总线新连接: {} (client {})", peer, id);
                if let Err(e) = handle_client(id, stream, rx, clients, subscriptions).await {
                    warn!("消息总线 client {} 连接异常: {}", id, e);
                }
                info!("消息总线连接断开: {} (client {})", peer, id);
            });
        }
    }
}

async fn handle_client(
    id: ClientId,
    stream: TcpStream,
    mut rx: mpsc::UnboundedReceiver<Message>,
    clients: Arc<AsyncMutex<HashMap<ClientId, mpsc::UnboundedSender<Message>>>>,
    subscriptions: Arc<AsyncMutex<HashMap<String, HashSet<ClientId>>>>,
) -> anyhow::Result<()> {
    let (mut reader, mut writer) = stream.into_split();

    let read_task = async {
        loop {
            let mut len_buf = [0u8; 4];
            reader.read_exact(&mut len_buf).await?;
            let len = u32::from_le_bytes(len_buf) as usize;
            if len == 0 || len > 1024 * 1024 {
                return Err::<(), anyhow::Error>(anyhow::anyhow!("消息长度异常: {}", len));
            }
            let mut buf = vec![0u8; len];
            reader.read_exact(&mut buf).await?;
            let msg: Message = serde_json::from_slice(&buf)?;

            match msg {
                Message::Subscribe { topics } => {
                    let mut subs = subscriptions.lock().await;
                    for topic in topics {
                        subs.entry(topic).or_default().insert(id);
                    }
                }
                Message::Publish { topic, payload } => {
                    let subs = subscriptions.lock().await;
                    let clients = clients.lock().await;
                    if let Some(set) = subs.get(&topic) {
                        for subscriber in set.iter() {
                            if *subscriber == id {
                                continue;
                            }
                            if let Some(tx) = clients.get(subscriber) {
                                let _ = tx.send(Message::Forward {
                                    topic: topic.clone(),
                                    payload: payload.clone(),
                                });
                            }
                        }
                    }
                }
                Message::Direct { target, payload } => {
                    // 定向发送：目标为服务名前缀，如 "char"、"map"。
                    let clients = clients.lock().await;
                    // 最小实现：直接广播给所有客户端，由接收端按 target 过滤。
                    for (client_id, tx) in clients.iter() {
                        if *client_id == id {
                            continue;
                        }
                        let _ = tx.send(Message::Direct {
                            target: target.clone(),
                            payload: payload.clone(),
                        });
                    }
                }
                Message::Forward { .. } => {
                    // 客户端不应发送 FORWARD。
                }
            }
        }
    };

    let write_task = async {
        while let Some(msg) = rx.recv().await {
            let data = serde_json::to_vec(&msg)?;
            let len = data.len() as u32;
            writer.write_all(&len.to_le_bytes()).await?;
            writer.write_all(&data).await?;
        }
        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        res = read_task => res,
        res = write_task => res,
    }
}

/// 消息总线客户端。
#[derive(Clone)]
pub struct MessageClient {
    writer: Arc<AsyncMutex<tokio::net::tcp::OwnedWriteHalf>>,
    inbound: Arc<std::sync::Mutex<mpsc::UnboundedReceiver<Message>>>,
}

impl MessageClient {
    /// 连接到消息总线。
    pub async fn connect(addr: SocketAddr) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let (reader, writer) = stream.into_split();
        let writer = Arc::new(AsyncMutex::new(writer));
        let (tx, inbound) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            let mut reader = reader;
            loop {
                let mut len_buf = [0u8; 4];
                match reader.read_exact(&mut len_buf).await {
                    Ok(_) => {}
                    Err(_) => break,
                }
                let len = u32::from_le_bytes(len_buf) as usize;
                if len == 0 || len > 1024 * 1024 {
                    break;
                }
                let mut buf = vec![0u8; len];
                if reader.read_exact(&mut buf).await.is_err() {
                    break;
                }
                if let Ok(msg) = serde_json::from_slice::<Message>(&buf) {
                    let _ = tx.send(msg);
                }
            }
        });

        Ok(Self {
            writer,
            inbound: Arc::new(std::sync::Mutex::new(inbound)),
        })
    }

    /// 订阅主题。
    pub async fn subscribe(&self, topics: &[String]) -> anyhow::Result<()> {
        self.send(Message::Subscribe {
            topics: topics.to_vec(),
        })
        .await
    }

    /// 发布消息。
    pub async fn publish(&self, topic: &str, payload: Value) -> anyhow::Result<()> {
        self.send(Message::Publish {
            topic: topic.to_string(),
            payload,
        })
        .await
    }

    /// 定向发送。
    pub async fn direct(&self, target: &str, payload: Value) -> anyhow::Result<()> {
        self.send(Message::Direct {
            target: target.to_string(),
            payload,
        })
        .await
    }

    /// 尝试接收一条消息。
    pub fn try_recv(&self) -> Option<Message> {
        self.inbound.lock().unwrap().try_recv().ok()
    }

    async fn send(&self, msg: Message) -> anyhow::Result<()> {
        let data = serde_json::to_vec(&msg)?;
        let len = data.len() as u32;
        let mut writer = self.writer.lock().await;
        writer.write_all(&len.to_le_bytes()).await?;
        writer.write_all(&data).await?;
        Ok(())
    }
}

/// 启动一个服务专用的消息总线客户端。
///
/// 客户端会订阅自身服务名与 `broadcast` 主题，并启动一个后台循环消费入站消息。
/// 返回的 `Arc<MessageClient>` 可用于后续发布事件。
pub async fn start_service_client(
    service_name: &str,
    broker_addr: SocketAddr,
) -> anyhow::Result<Arc<MessageClient>> {
    let client = MessageClient::connect(broker_addr).await?;
    let topics = vec![service_name.to_string(), "broadcast".to_string()];
    client.subscribe(&topics).await?;
    info!("{} 服务已连接消息总线，订阅 {:?}", service_name, topics);

    let client = Arc::new(client);
    let client_for_loop = Arc::clone(&client);
    let name = service_name.to_string();
    tokio::spawn(async move {
        loop {
            match client_for_loop.try_recv() {
                Some(Message::Forward { topic, payload }) => {
                    info!("[{}] 收到主题 {} 消息: {}", name, topic, payload);
                }
                Some(Message::Direct { target, payload }) => {
                    info!("[{}] 收到定向消息 target={}: {}", name, target, payload);
                }
                Some(_) => {}
                None => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
    });

    client
        .publish(
            "service.announce",
            serde_json::json!({"service": service_name, "status": "ready"}),
        )
        .await?;

    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn broker_forwards_published_messages_between_clients() {
        let broker = MessageBroker::new("127.0.0.1:0".parse().unwrap())
            .await
            .expect("应能启动总线");
        let addr = broker.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = broker.run().await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let a = MessageClient::connect(addr).await.unwrap();
        let b = MessageClient::connect(addr).await.unwrap();
        a.subscribe(&["chat".to_string()]).await.unwrap();
        b.subscribe(&["chat".to_string()]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        a.publish("chat", serde_json::json!({"text": "hello"}))
            .await
            .unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        let mut received = None;
        while tokio::time::Instant::now() < deadline {
            if let Some(Message::Forward { topic, payload }) = b.try_recv() {
                received = Some((topic, payload));
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(received.is_some(), "订阅客户端应收到转发消息");
        let (topic, payload) = received.unwrap();
        assert_eq!(topic, "chat");
        assert_eq!(payload["text"], "hello");
    }

    #[tokio::test]
    async fn publisher_does_not_receive_its_own_message() {
        let broker = MessageBroker::new("127.0.0.1:0".parse().unwrap())
            .await
            .expect("应能启动总线");
        let addr = broker.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = broker.run().await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let client = MessageClient::connect(addr).await.unwrap();
        client.subscribe(&["echo".to_string()]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        client
            .publish("echo", serde_json::json!({"ok": true}))
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(client.try_recv().is_none(), "发布者不应收到自己发布的消息");
    }

    #[tokio::test]
    async fn broker_forward_latency_under_one_millisecond() {
        let broker = MessageBroker::new("127.0.0.1:0".parse().unwrap())
            .await
            .expect("应能启动总线");
        let addr = broker.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = broker.run().await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let publisher = MessageClient::connect(addr).await.unwrap();
        let subscriber = MessageClient::connect(addr).await.unwrap();
        subscriber
            .subscribe(&["latency".to_string()])
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let iterations = 100;
        let mut latencies = Vec::with_capacity(iterations);
        for i in 0..iterations {
            let start = tokio::time::Instant::now();
            publisher
                .publish("latency", serde_json::json!({"seq": i}))
                .await
                .unwrap();

            let deadline = start + Duration::from_millis(10);
            while tokio::time::Instant::now() < deadline {
                if subscriber.try_recv().is_some() {
                    latencies.push(start.elapsed());
                    break;
                }
                tokio::task::yield_now().await;
            }
        }

        assert!(
            latencies.len() == iterations,
            "应收到全部 {} 条消息，实际收到 {}",
            iterations,
            latencies.len()
        );
        let avg = latencies.iter().sum::<Duration>() / latencies.len() as u32;
        assert!(
            avg < Duration::from_micros(1000),
            "消息总线平均转发延迟 {:?}，应小于 1ms",
            avg
        );
    }
}
