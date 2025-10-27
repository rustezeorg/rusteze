use rusteze::{auth, route};
use serde::{Deserialize, Serialize};

#[auth]
pub async fn my_auth_handler() {
    // Here we can run auth checks
    // This function will receive auth context as JSON from command line args
    // You can validate tokens, check permissions, etc.
    println!("Auth check passed!");
}

// Example of routes that require authentication
#[route(method = "GET", path = "/protected", auth = true)]
pub async fn protected_endpoint() -> String {
    "This is a protected endpoint!".to_string()
}

#[route(method = "POST", path = "/admin/users", auth = true)]
pub async fn create_user() -> String {
    "User created (admin only)!".to_string()
}
