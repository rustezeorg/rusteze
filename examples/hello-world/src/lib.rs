use rusteze::route;

#[route(method = "GET", path = "/hello/{word}")]
pub fn get_hello(word: String) -> String {
    if word.is_empty() {
        return format!("Hello World!");
    }
    return format!("Hello {}", word);
}

#[route(method = "POST", path = "/calculate")]
pub fn add_numbers(a: i32, b: i32) -> String {
    let result = a + b;
    format!("The sum of {} and {} is {}", a, b, result)
}

#[route(method = "GET", path = "/status")]
pub fn get_status() -> String {
    "My amazing Service is running!".to_string()
}

#[route(method = "PUT", path = "/toggle/{enabled}")]
pub fn toggle_feature(enabled: bool) -> String {
    if enabled {
        "Feature enabled".to_string()
    } else {
        "Feature disabled".to_string()
    }
}

#[route(method = "POST", path = "/test")]
pub fn test_post_fn() -> String {
    format!("Testing")
}
