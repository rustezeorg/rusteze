use rusteze::route;

#[route(method = "GET", path = "/hello/{word}")]
pub async fn get_hello(word: String) -> String {
    if word.is_empty() {
        return format!("Hello World!");
    }
    return format!("Hello {}", word);
}
