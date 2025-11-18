---
title: Auth
description: Auth intro
---

# Authentication System

Rusteze provides a powerful authentication system that allows you to protect your routes with custom auth handlers.

## Overview

The auth system consists of two main components:

1. **Auth Handlers** - Functions decorated with `#[auth]` that perform authentication logic
2. **Protected Routes** - Routes that specify `auth = true` to require authentication

## Creating Auth Handlers

Use the `#[auth]` macro to create authentication handlers:

```rust
use rusteze::auth;

#[auth]
pub async fn my_auth_handler() {
    // Your authentication logic here
    // This function receives auth context as JSON from command line args
    // You can validate tokens, check permissions, etc.
    println!("Auth check passed!");
}
```

### Auth Handler Features

- **Automatic Binary Generation**: Each auth handler generates a standalone binary
- **JSON Context**: Auth handlers receive authentication context as JSON
- **Fast Execution**: Optimized for quick auth checks (10s timeout by default)
- **Error Handling**: Failed auth checks exit with non-zero status codes

### Advanced Auth Handler

```rust
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct AuthContext {
    token: String,
    user_id: Option<String>,
    permissions: Vec<String>,
}

#[auth]
pub async fn jwt_auth_handler() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("No auth context provided");
        std::process::exit(1);
    }

    let auth_context: AuthContext = match serde_json::from_str(&args[1]) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("Invalid auth context: {}", e);
            std::process::exit(1);
        }
    };

    // Validate JWT token
    if !validate_jwt(&auth_context.token) {
        eprintln!("Invalid token");
        std::process::exit(1);
    }

    // Check permissions
    if !auth_context.permissions.contains(&"read".to_string()) {
        eprintln!("Insufficient permissions");
        std::process::exit(1);
    }

    println!("Authentication successful");
}

fn validate_jwt(token: &str) -> bool {
    // Your JWT validation logic
    !token.is_empty()
}
```

## Protecting Routes

Add `auth = true` to your route macro to require authentication:

```rust
use rusteze::route;

// Protected route that requires authentication
#[route(method = "GET", path = "/protected", auth = true)]
pub async fn protected_endpoint() -> String {
    "This is a protected endpoint!".to_string()
}

// Public route (no auth required)
#[route(method = "GET", path = "/public")]
pub async fn public_endpoint() -> String {
    "This is a public endpoint!".to_string()
}
```

## Route Parameters

The `auth` parameter in the route macro controls authentication:

- `auth = true` - Route requires authentication using the default auth handler
- No `auth` parameter - Route is public (no authentication required)

The system automatically uses the first available auth handler as the default. If you have multiple auth handlers, the first one defined will be used for all `auth = true` routes.

## Generated Configuration

When you use auth handlers and protected routes, Rusteze generates configuration in `manifest.json`:

### Auth Handlers Section

```json
{
  "auth": {
    "my_auth_handler": {
      "binary": "my_auth_handler_auth",
      "description": "Auth handler: my_auth_handler",
      "deployment": {
        "runtime": "rust",
        "memory": "128MB",
        "timeout": "10s",
        "environment": "auth",
        "arn": null
      }
    }
  }
}
```

### Protected Routes

```json
{
  "route": {
    "protected_endpoint": {
      "method": "GET",
      "path": "/protected",
      "binary": "protected_endpoint_get",
      "auth": {
        "handler": "my_auth_handler",
        "required": true
      }
    }
  }
}
```

## How It Works

1. **Development**: Auth handlers run as separate processes during local development
2. **Deployment**: Auth handlers become Lambda functions that validate requests
3. **Context**: Authentication context (headers, tokens, etc.) is passed as JSON
4. **Validation**: Auth handlers return success (exit 0) or failure (exit 1)

## Best Practices

### Security

- Always validate input in auth handlers
- Use proper JWT validation libraries
- Implement rate limiting for auth endpoints
- Log authentication attempts for monitoring

### Performance

- Keep auth handlers lightweight and fast
- Cache validation results when appropriate
- Use efficient token validation methods
- Minimize external API calls in auth handlers

### Error Handling

```rust
#[auth]
pub async fn robust_auth_handler() {
    let args: Vec<String> = std::env::args().collect();

    // Always validate input
    if args.len() < 2 {
        eprintln!("No auth context provided");
        std::process::exit(1);
    }

    // Parse with proper error handling
    let context: serde_json::Value = match serde_json::from_str(&args[1]) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("Failed to parse auth context: {}", e);
            std::process::exit(1);
        }
    };

    // Validate required fields
    let token = match context.get("token").and_then(|t| t.as_str()) {
        Some(t) => t,
        None => {
            eprintln!("Missing token in auth context");
            std::process::exit(1);
        }
    };

    // Your validation logic here
    if !is_valid_token(token) {
        eprintln!("Invalid token");
        std::process::exit(1);
    }

    println!("Authentication successful");
}
```

## Multiple Auth Handlers

You can create multiple auth handlers, but the system will use the first one as the default for all `auth = true` routes:

```rust
// This will be the default auth handler (first defined)
#[auth]
pub async fn jwt_auth() {
    // JWT validation logic
}

// Additional auth handlers (for future extensibility)
#[auth]
pub async fn api_key_auth() {
    // API key validation logic
}

// All routes with auth = true will use jwt_auth (the first handler)
#[route(method = "GET", path = "/user/profile", auth = true)]
pub async fn user_profile() -> String { "User profile".to_string() }

#[route(method = "GET", path = "/api/data", auth = true)]
pub async fn api_data() -> String { "API data".to_string() }

#[route(method = "DELETE", path = "/admin/users", auth = true)]
pub async fn delete_user() -> String { "User deleted".to_string() }
```

**Note**: Currently, all protected routes use the same default auth handler. Future versions may support specifying different handlers per route.

## Testing Auth Handlers

You can test auth handlers directly by running their generated binaries:

```bash
# Test with valid auth context
echo '{"token": "valid-jwt-token", "user_id": "123"}' | \
  cargo run --bin my_auth_handler_auth

# Test with invalid context
echo '{"invalid": "context"}' | \
  cargo run --bin my_auth_handler_auth
```

## Integration with Local Development

During local development, the Rusteze dev server automatically:

1. Intercepts requests to protected routes
2. Extracts authentication context from headers
3. Calls the appropriate auth handler binary
4. Allows or denies the request based on the auth result

This provides a seamless development experience that mirrors production behavior.
