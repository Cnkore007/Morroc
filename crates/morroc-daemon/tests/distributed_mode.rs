//! 分布式模式集成测试：验证 login/char/map 服务客户端通过消息总线转发消息。

use morroc_daemon::broker::{Message, MessageBroker, MessageClient};
use std::time::Duration;

#[tokio::test]
async fn distributed_services_forward_messages_via_broker() {
    // 在随机端口启动消息总线。
    let broker = MessageBroker::new("127.0.0.1:0".parse().unwrap())
        .await
        .expect("应能启动消息总线");
    let addr = broker.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = broker.run().await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 模拟三个独立服务客户端。
    let login = MessageClient::connect(addr).await.unwrap();
    let char_client = MessageClient::connect(addr).await.unwrap();
    let map_client = MessageClient::connect(addr).await.unwrap();

    login
        .subscribe(&["login".to_string(), "broadcast".to_string()])
        .await
        .unwrap();
    char_client
        .subscribe(&[
            "char".to_string(),
            "login".to_string(),
            "broadcast".to_string(),
        ])
        .await
        .unwrap();
    map_client
        .subscribe(&[
            "map".to_string(),
            "char".to_string(),
            "broadcast".to_string(),
        ])
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Login 发布跨服务事件，Char 订阅了 "login" 主题，应能收到。
    login
        .publish(
            "login",
            serde_json::json!({"event": "account_ready", "account_id": 1}),
        )
        .await
        .unwrap();

    let mut received_by_char = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        if let Some(Message::Forward { topic, payload }) = char_client.try_recv() {
            assert_eq!(topic, "login");
            received_by_char = Some(payload);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(received_by_char.is_some(), "Char 服务应收到 Login 事件");

    // Char 继续转发到 Map。
    char_client
        .publish(
            "char",
            serde_json::json!({
                "event": "char_selected",
                "account_id": 1,
                "map": "prontera",
            }),
        )
        .await
        .unwrap();

    let mut received_by_map = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        if let Some(Message::Forward { topic, payload }) = map_client.try_recv() {
            assert_eq!(topic, "char");
            received_by_map = Some(payload);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(received_by_map.is_some(), "Map 服务应收到 Char 事件");

    // 发布者不应收到自己发布的消息。
    assert!(
        login.try_recv().is_none(),
        "Login 不应收到自己发布的 login 消息"
    );
    assert!(
        char_client.try_recv().is_none(),
        "Char 不应收到自己发布的 char 消息"
    );
}
