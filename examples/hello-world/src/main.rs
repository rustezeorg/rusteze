use rusteze::route;
use rusteze_codegen::{publisher, subscriber};
use serde_json::Value;

#[route(method = "GET", path = "/test/{word}", memory = 256, timeout = 30)]
fn get_hello_handler(word: String) -> String {
    // Setup dependencies such as db connections to be re-used.
    get_hello(word)
}

#[route(method = "GET", path = "/bar", memory = 256, timeout = 30)]
fn get_bar_handler() -> String {
    format!("Foo Bar")
}

#[route(method = "GET", path = "/brad", memory = 256, timeout = 30)]
fn get_foo_handler() -> String {
    format!("Foo")
}

fn get_hello(word: String) -> String {
    if word.is_empty() {
        format!("Hello World!")
    } else {
        format!("Hello, my name is {}!", word)
    }
}

#[publisher(topic = "user-events")]
async fn publish_user_event(event: Value) -> Result<String, String> {
    Ok(format!("Published event: {}", event))
}

#[subscriber(topic = "user-events", queue = "user-processor")]
async fn handle_user_event(event: Value) -> Result<(), String> {
    println!("Processing event: {}", event);
    Ok(())
}
