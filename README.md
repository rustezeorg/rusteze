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

## What is Rusteze

The goal of rusteze is to write once deploy anywhere, so write your api endpoints and deploy
to aws lambda functions, containers, cloudflare or just to a docker container you can run yourself.

## Quick Look

Below is a basic "Hello Wolrd" application:

```rust
use rusteze::{route};

#[route(method = "GET", path = "/hello/{word}")]
fn get_hello(word: String) -> String {
  if word.is_empty() {
      return format!("Hello World!")
  }
  return format!("Hello {}", word);
}
```

You can now deploy this with `cargo rusteze deploy`

## Deployments

- [ ] AWS
  - [ ] Lambda + Api Gateway
- [ ] Cloudflare
- [ ] GCP

### Development
