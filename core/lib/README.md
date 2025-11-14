# Rusteze

<p align="center">
  <img src="../../logo.webp" alt="rusteze-logo" width="400"/>
</p>
<p align="center">"You too can deploy with ease"</p>

## What is Rusteze

The goal of rusteze is to write once deploy anywhere, so write your api endpoints and deploy
to aws lambda functions, containers, cloudflare or just to a docker container you can run yourself.

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
