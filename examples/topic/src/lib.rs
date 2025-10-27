use rusteze::{auth, route, subscriber};
use serde::{Deserialize, Serialize};

#[route(method = "POST", path = "/test")]
pub async fn test_post_fn(body: TestBodyPayload) -> String {
    let publisher = match rusteze::pubsub::service::PublisherBuilder::new()
        .build()
        .await
    {
        Ok(p) => p,
        Err(e) => panic!("Error loading publisher: {:?}", e),
    };

    let m = serde_json::json!({
        "payload": {
            "Hello": "world"
        }
    });
    let topic = String::from("user-events");

    println!("Sending to topic: {}", &topic);

    match publisher.publish(&topic, &m).await {
        Ok(t) => println!("Message sent!"),
        Err(e) => println!("Unable to send message: {:?}", e),
    }

    format!("Testing {:?}", body)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserEvent {
    pub user_id: String,
    pub action: String,
    pub timestamp: String,
}

#[subscriber(topic = "user-events")]
pub async fn handle_user_event(event: UserEvent) -> String {
    println!("HELLO FROM THE HANDLER SIDE!");

    println!(
        "HELLO FROM THE LIB!!! Processed user event: {} performed {} at {}",
        event.user_id, event.action, event.timestamp
    );

    format!(
        "HELLO FROM THE LIB!!! Processed user event: {} performed {} at {}",
        event.user_id, event.action, event.timestamp
    )
}
