# Rusteze

<p align="center">
  <img src="./logo.webp" alt="rusteze-logo" width="400"/>
</p>
<p align="center">"You too can deploy with ease"</p>
<br>
<p align="center">
  <a href="https://github.com/rustezeorg/rusteze/search?l=rust">
    <img alt="language" src="https://img.shields.io/badge/language-Rust-orange.svg">
  </a>
  <img alt="GitHub Actions Workflow Status" src="https://img.shields.io/github/actions/workflow/status/rustezeorg/rusteze/test">
</p>

> [!CAUTION]
> This framework is currently under early active development and is likely to change.
> It should not be used for production(yet).

## What is Rusteze

The goal of rusteze is to write once deploy anywhere, so write your api endpoints and deploy
to aws lambda functions, containers, cloudflare or just to a docker container you can run yourself.

## Installation

### Prerequisites

For AWS Lambda deployments, you'll need:

```bash
# Install cargo-lambda for Lambda function building and packaging
cargo install cargo-lambda
```

### Install Rusteze

```bash
# Install from source (for now)
cargo install --git https://github.com/rustezeorg/rusteze/tree/main/core/cargo-rusteze rusteze
```

## Quick Look

Below is a basic "Hello World" application:

`rusteze.toml` config file:

```toml
service_name = "hello-world"

[deployment]
provider = "aws"
region = "ap-southeast-2"
```

`lib.rs` file:

```rust
use rusteze::{route};

#[route(method = "GET", path = "/hello/{word}")]
pub fn get_hello(word: String) -> String {
  if word.is_empty() {
      return format!("Hello World!")
  }
  return format!("Hello {}", word);
}
```

You can now deploy this with `cargo rusteze deploy` which will create an API with API gateway backed by lambda.

## PubSub

Rusteze provides two macros for implementing publish-subscribe patterns: `#[publisher]` and `#[subscriber]`.

### Publisher Macro

The `#[publisher]` macro marks a function as a publisher for a specific topic. Publishers don't generate binary files but register the topic in the manifest.json configuration.

```rust
use rusteze::publisher;

#[publisher(topic = "user-events")]
pub fn publish_user_event(event: serde_json::Value) -> String {
    format!("Publishing user event: {:?}", event)
}
```

### Subscriber Macro

The `#[subscriber]` macro marks a function as a subscriber for a specific topic. Subscribers generate their own binary files and are registered in the manifest.json configuration.

```rust
use rusteze::subscriber;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct UserEvent {
    pub user_id: String,
    pub action: String,
    pub timestamp: String,
}

#[subscriber(topic = "user-events")]
pub fn handle_user_event(event: UserEvent) -> String {
    format!("Processed user event: {} performed {} at {}",
            event.user_id, event.action, event.timestamp)
}
```

### Testing Subscribers

You can test subscriber binaries directly:

```bash
# Build the subscriber
cargo build --bin handle_user_event_subscriber

# Test with a direct message
./target/debug/handle_user_event_subscriber '{"user_id": "user123", "action": "login", "timestamp": "2024-01-15T10:30:00Z"}'
```

## Deployments

### Development

- [x] Local dev server
  - [x] API
  - [x] run command to start dev server
  - [x] hot path reload
  - [x] dynamically call rust binary
  - [x] routes.json to lock
  - [x] Path param pass through
  - [x] Body on post
  - [x] Auth
- [ ] pub/sub setup
  - [x] local server to allow local development.
  - [x] local pubsub sending
- [ ] Deployments
  - [x] AWS
    - [x] Api Gateway
    - [x] lambda
    - [ ] SNS + SQS (pub/sub)
  - [ ] Cloudflare
  - [ ] GCP
  - [ ] Container
- [ ] Release to crates

- need to fix api gateway updates.
  - Need to fix removal of routes + integrations when not in code anymore.
- implement tracing: https://crates.io/crates/tracing
- Issue when ARN hasn't been saved, or when .rusteze folder is deleted, you lose the ARN's leading to multiple deployments, wonder if the manifest.json should move outside.
- Issue when running serve locally with missing env var: WS_LAMBDA_FUNCTION_NAME
