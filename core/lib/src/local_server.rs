use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    Router,
    extract::{Path, Query, State},
    http::{HeaderMap, Method, StatusCode},
    response::Json,
    routing::any,
};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path as FilePath;
use std::sync::Mutex;
use std::sync::mpsc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::broadcast;

use crate::{
    config::RustezeConfig,
    error::{RustezeError, RustezeResult},
};

/// Trait for handling HTTP requests in a unified way
#[async_trait::async_trait]
pub trait RouteHandler: Send + Sync {
    async fn handle(&self, request: HttpRequest) -> HttpResponse;
}

/// Trait for executable functions that can be called by the local server
pub trait ExecutableFunction: Send + Sync {
    fn call(&self, params: HashMap<String, String>) -> Result<String, String>;
    fn get_signature(&self) -> &FunctionSignature;
}

/// Registry for storing executable functions
pub struct FunctionRegistry {
    functions: HashMap<String, Box<dyn ExecutableFunction>>,
}

impl FunctionRegistry {
    pub fn new() -> Self {
        Self {
            functions: HashMap::new(),
        }
    }

    pub fn register<F>(&mut self, name: String, function: F)
    where
        F: ExecutableFunction + 'static,
    {
        self.functions.insert(name, Box::new(function));
    }

    pub fn get(&self, name: &str) -> Option<&dyn ExecutableFunction> {
        self.functions.get(name).map(|f| f.as_ref())
    }

    pub fn list_functions(&self) -> Vec<String> {
        self.functions.keys().cloned().collect()
    }
}

/// HTTP request representation for route handlers
#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: Method,
    pub path: String,
    pub path_params: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Option<Value>,
}

/// HTTP response representation for route handlers
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status_code: StatusCode,
    pub headers: HashMap<String, String>,
    pub body: Option<Value>,
}

impl Default for HttpResponse {
    fn default() -> Self {
        Self {
            status_code: StatusCode::OK,
            headers: HashMap::new(),
            body: None,
        }
    }
}

/// Local development server for testing Rusteze applications
pub struct LocalServer {
    routes: HashMap<String, Arc<dyn RouteHandler>>,
    config: RustezeConfig,
    router: Option<Router>,
    pub_sub_service: Arc<MockPubSubService>,
    function_registry: Arc<Mutex<FunctionRegistry>>,
}

impl LocalServer {
    /// Create a new LocalServer instance
    pub fn new(config: RustezeConfig) -> Self {
        Self {
            routes: HashMap::new(),
            config,
            router: None,
            pub_sub_service: Arc::new(MockPubSubService::new()),
            function_registry: Arc::new(Mutex::new(FunctionRegistry::new())),
        }
    }

    /// Register a route handler for a specific route
    pub fn register_route<H>(&mut self, route_key: String, handler: H) -> RustezeResult<()>
    where
        H: RouteHandler + 'static,
    {
        self.routes.insert(route_key, Arc::new(handler));
        Ok(())
    }

    /// Discover and register routes from rusteze.deploy.toml metadata
    pub fn discover_and_register_routes(&mut self) -> RustezeResult<()> {
        let deployment_metadata = self.load_deployment_metadata()?;

        for (function_name, function_meta) in deployment_metadata.functions {
            if function_meta.handler_type == "HttpRoute" {
                let route_key = format!("{} {}", function_meta.method, function_meta.path);

                println!(
                    "🛣️  Creating route: {} -> function: {} (from deployment metadata)",
                    route_key, function_name
                );

                // Create a route handler from the function metadata
                let handler = MetadataRouteHandler::new(
                    function_name,
                    function_meta,
                    self.function_registry.clone(),
                );
                self.register_route(route_key, handler)?;
            }
        }

        Ok(())
    }

    /// Load deployment metadata from rusteze.deploy.toml
    fn load_deployment_metadata(&self) -> RustezeResult<DeploymentMetadata> {
        let metadata_path = "rusteze.deploy.toml";

        if !FilePath::new(metadata_path).exists() {
            return Err(RustezeError::local_server(
                "rusteze.deploy.toml not found. Run 'cargo rusteze build' first to generate deployment metadata.",
            ));
        }

        let content = fs::read_to_string(metadata_path).map_err(|e| {
            RustezeError::local_server(format!("Failed to read {}: {}", metadata_path, e))
        })?;

        let metadata: DeploymentMetadata = toml::from_str(&content).map_err(|e| {
            RustezeError::local_server(format!("Failed to parse {}: {}", metadata_path, e))
        })?;

        Ok(metadata)
    }

    /// Get reference to the mock pub/sub service
    pub fn pub_sub_service(&self) -> Arc<MockPubSubService> {
        self.pub_sub_service.clone()
    }

    /// Register an executable function
    pub fn register_function<F>(&self, name: String, function: F)
    where
        F: ExecutableFunction + 'static,
    {
        let mut registry = self.function_registry.lock().unwrap();
        registry.register(name, function);
    }

    /// Get reference to the function registry
    pub fn function_registry(&self) -> Arc<Mutex<FunctionRegistry>> {
        self.function_registry.clone()
    }

    /// Register built-in example functions for testing
    pub fn register_example_functions(&self) {
        // Try to load functions from the macro-generated registration files
        match self.load_registered_functions() {
            Ok(count) => {
                println!(
                    "📦 Successfully registered {} functions from macro-generated files",
                    count
                );
            }
            Err(e) => {
                println!("⚠️  Failed to load registered functions: {}", e);
                println!("📦 Falling back to mock functions");

                // Fallback to mock functions
                let mock_function = MockRustFunction::new();
                self.register_function("get_hello".to_string(), mock_function.clone());
                self.register_function("get_hello_handler".to_string(), mock_function);
            }
        }
    }

    /// Load functions from macro-generated registration files
    fn load_registered_functions(&self) -> RustezeResult<usize> {
        let registration_file = "rusteze.functions.json";

        if !FilePath::new(registration_file).exists() {
            return Err(RustezeError::local_server(
                "rusteze.functions.json not found. Make sure you have functions with #[route] macro.",
            ));
        }

        let content = fs::read_to_string(registration_file).map_err(|e| {
            RustezeError::local_server(format!("Failed to read {}: {}", registration_file, e))
        })?;

        let registrations: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
            RustezeError::local_server(format!("Failed to parse {}: {}", registration_file, e))
        })?;

        let mut count = 0;
        if let Some(functions) = registrations.as_object() {
            for (func_name, func_data) in functions {
                if let Some(func_obj) = func_data.as_object() {
                    println!("📝 Registering function: {} from functions.json", func_name);

                    // Create a callable function that can invoke the actual compiled function
                    let callable_function =
                        CallableRustFunction::new(func_name.clone(), func_obj.clone());

                    self.register_function(func_name.clone(), callable_function);
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Initialize pub/sub resources from deployment metadata
    pub fn initialize_pub_sub_resources(&self) -> RustezeResult<()> {
        let deployment_metadata = self.load_deployment_metadata()?;

        // Create topics and queues based on function metadata
        for (function_name, function_meta) in deployment_metadata.functions {
            match function_meta.handler_type.as_str() {
                "Publisher" => {
                    // Extract topic name from function metadata or use function name
                    let topic_name = format!("{}-topic", function_name);
                    self.pub_sub_service.create_topic(&topic_name)?;
                }
                "Subscriber" => {
                    // Extract topic and queue names from function metadata
                    let topic_name = format!("{}-topic", function_name.replace("_subscriber", ""));
                    let queue_name = format!("{}-queue", function_name);

                    // Create topic if it doesn't exist
                    self.pub_sub_service.create_topic(&topic_name)?;

                    // Create queue and subscribe to topic
                    self.pub_sub_service
                        .create_queue(&queue_name, Some(&topic_name))?;
                }
                _ => {} // HttpRoute doesn't need pub/sub resources
            }
        }

        Ok(())
    }

    /// Build the Axum router from registered routes
    pub fn build_router(&mut self) -> RustezeResult<()> {
        // Register example functions for testing
        self.register_example_functions();

        // Discover and register routes from deployment metadata
        self.discover_and_register_routes()?;

        // Initialize pub/sub resources
        self.initialize_pub_sub_resources()?;

        // Create shared state for route handlers
        let shared_state = Arc::new(LocalServerState {
            routes: self.routes.clone(),
        });

        // Add a catch-all route that will handle dynamic routing
        let router = Router::new()
            .route("/{*wildcard}", any(handle_dynamic_route))
            .with_state(shared_state);

        self.router = Some(router);
        Ok(())
    }

    /// Start the local development server
    pub async fn start(&mut self, addr: SocketAddr) -> RustezeResult<()> {
        if self.router.is_none() {
            self.build_router()?;
        }

        let router = self.router.take().unwrap();
        let listener = TcpListener::bind(addr).await.map_err(|e| {
            RustezeError::local_server(format!("Failed to bind to {}: {}", addr, e))
        })?;

        println!("🚀 Local development server running at http://{}", addr);
        println!("📝 Available routes:");
        for route_key in self.routes.keys() {
            println!("  - {}", route_key);
        }

        // Start pub/sub message processing
        self.pub_sub_service.start_message_processing().await;

        println!("📢 Mock pub/sub service initialized:");
        println!("  Topics: {:?}", self.pub_sub_service.list_topics());
        println!("  Queues: {:?}", self.pub_sub_service.list_queues());

        axum::serve(listener, router.into_make_service())
            .await
            .map_err(|e| RustezeError::local_server(format!("Server error: {}", e)))?;

        Ok(())
    }
}

/// Shared state for the local server
#[derive(Clone)]
struct LocalServerState {
    routes: HashMap<String, Arc<dyn RouteHandler>>,
}

/// Deployment metadata structure from rusteze.deploy.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentMetadata {
    pub service: ServiceMetadata,
    pub providers: HashMap<String, ProviderConfig>,
    pub functions: HashMap<String, FunctionMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceMetadata {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub provider_type: String,
    pub config: ProviderSpecificConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSpecificConfig {
    pub runtime: Option<String>,
    pub region: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionMetadata {
    pub name: String,
    pub path: String,
    pub method: String,
    pub file: String,
    pub handler_type: String,
    pub signature: FunctionSignature,
    pub deployment_metadata: DeploymentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionSignature {
    pub return_type: String,
    pub is_async: bool,
    pub parameters: Vec<ParameterMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterMetadata {
    pub name: String,
    pub param_type: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentConfig {
    pub memory_mb: Option<u32>,
    pub timeout_seconds: Option<u32>,
    pub permissions: Vec<String>,
    pub environment_variables: HashMap<String, String>,
}

/// Dynamic route handler that matches requests to registered routes
async fn handle_dynamic_route(
    State(state): State<Arc<LocalServerState>>,
    method: Method,
    Path(path): Path<String>,
    Query(query_params): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> Result<Json<Value>, StatusCode> {
    // Normalize path to start with /
    let normalized_path = if path.starts_with('/') {
        path
    } else {
        format!("/{}", path)
    };

    // Find matching route handler
    let route_key = format!("{} {}", method, normalized_path);

    // Convert headers to HashMap
    let headers_map: HashMap<String, String> = headers
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_str().unwrap_or("").to_string()))
        .collect();

    println!("HELLO? handle_dynamic_route");

    // Try exact match first
    if let Some(handler) = state.routes.get(&route_key) {
        let request = HttpRequest {
            method: method.clone(),
            path: normalized_path,
            path_params: HashMap::new(),
            query_params,
            headers: headers_map,
            body: body.map(|b| b.0),
        };

        let response = handler.handle(request).await;
        return Ok(Json(response.body.unwrap_or(Value::Null)));
    }

    // Try pattern matching for parameterized routes
    for (registered_route, handler) in &state.routes {
        if let Some(path_params) = match_route_pattern(registered_route, &method, &normalized_path)
        {
            let request = HttpRequest {
                method: method.clone(),
                path: normalized_path,
                path_params,
                query_params: query_params.clone(),
                headers: headers_map.clone(),
                body: body.map(|b| b.0),
            };

            let response = handler.handle(request).await;
            return Ok(Json(response.body.unwrap_or(Value::Null)));
        }
    }

    // No matching route found
    Err(StatusCode::NOT_FOUND)
}

/// Match a request against a route pattern and extract path parameters
fn match_route_pattern(
    registered_route: &str,
    method: &Method,
    path: &str,
) -> Option<HashMap<String, String>> {
    let parts: Vec<&str> = registered_route.splitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }

    let route_method = parts[0];
    let route_pattern = parts[1];

    // Check if method matches
    if route_method != method.as_str() {
        return None;
    }

    // Split path and pattern into segments
    let path_segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let pattern_segments: Vec<&str> = route_pattern.split('/').filter(|s| !s.is_empty()).collect();

    // Must have same number of segments
    if path_segments.len() != pattern_segments.len() {
        return None;
    }

    let mut path_params = HashMap::new();

    // Match each segment
    for (path_seg, pattern_seg) in path_segments.iter().zip(pattern_segments.iter()) {
        if pattern_seg.starts_with('{') && pattern_seg.ends_with('}') {
            // This is a parameter
            let param_name = &pattern_seg[1..pattern_seg.len() - 1];
            path_params.insert(param_name.to_string(), path_seg.to_string());
        } else if path_seg != pattern_seg {
            // Literal segment doesn't match
            return None;
        }
    }

    Some(path_params)
}

/// Route handler that processes requests based on deployment metadata
pub struct MetadataRouteHandler {
    function_name: String,
    function_metadata: FunctionMetadata,
    function_registry: Arc<Mutex<FunctionRegistry>>,
}

impl MetadataRouteHandler {
    pub fn new(
        function_name: String,
        function_metadata: FunctionMetadata,
        function_registry: Arc<Mutex<FunctionRegistry>>,
    ) -> Self {
        Self {
            function_name,
            function_metadata,
            function_registry,
        }
    }

    /// Transform request to match cloud behavior
    fn transform_request(&self, request: &HttpRequest) -> Value {
        // Create a Lambda-like event structure for local testing
        serde_json::json!({
            "httpMethod": request.method.to_string(),
            "path": request.path,
            "pathParameters": request.path_params,
            "queryStringParameters": request.query_params,
            "headers": request.headers,
            "body": request.body,
            "requestContext": {
                "httpMethod": request.method.to_string(),
                "path": request.path,
                "stage": "local",
                "requestId": "local-request-id",
                "identity": {
                    "sourceIp": "127.0.0.1"
                }
            }
        })
    }

    /// Execute the actual function and transform response to match cloud behavior
    fn execute_function(&self, request: &HttpRequest) -> HttpResponse {
        let registry = self.function_registry.lock().unwrap();

        match registry.get(&self.function_name) {
            Some(function) => {
                // Call the actual function with path parameters
                match function.call(request.path_params.clone()) {
                    Ok(result) => HttpResponse {
                        status_code: axum::http::StatusCode::OK,
                        headers: {
                            let mut headers = HashMap::new();
                            headers.insert("content-type".to_string(), "text/plain".to_string());
                            headers.insert(
                                "x-rusteze-function".to_string(),
                                self.function_name.clone(),
                            );
                            headers
                        },
                        body: Some(serde_json::Value::String(result)),
                    },
                    Err(error) => HttpResponse {
                        status_code: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        headers: {
                            let mut headers = HashMap::new();
                            headers
                                .insert("content-type".to_string(), "application/json".to_string());
                            headers
                        },
                        body: Some(serde_json::json!({
                            "error": error,
                            "function": self.function_name
                        })),
                    },
                }
            }
            None => {
                // Function not registered, return mock response
                HttpResponse {
                    status_code: axum::http::StatusCode::OK,
                    headers: {
                        let mut headers = HashMap::new();
                        headers.insert("content-type".to_string(), "application/json".to_string());
                        headers
                            .insert("x-rusteze-function".to_string(), self.function_name.clone());
                        headers.insert("x-rusteze-mock".to_string(), "true".to_string());
                        headers
                    },
                    body: Some(serde_json::json!({
                        "message": format!("Mock response for function: {} (function not registered)", self.function_name),
                        "function_metadata": {
                            "name": self.function_metadata.name,
                            "path": self.function_metadata.path,
                            "method": self.function_metadata.method,
                            "handler_type": self.function_metadata.handler_type
                        },
                        "path_params": request.path_params,
                        "query_params": request.query_params,
                        "note": "Register your function using server.register_function() to execute actual code"
                    })),
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl RouteHandler for MetadataRouteHandler {
    async fn handle(&self, request: HttpRequest) -> HttpResponse {
        // Execute the actual function or return mock response
        self.execute_function(&request)
    }
}

/// Mock pub/sub service for local testing
pub struct MockPubSubService {
    topics: Mutex<HashMap<String, MockTopic>>,
    queues: Mutex<HashMap<String, MockQueue>>,
}

/// Mock topic for local testing
#[derive(Debug)]
struct MockTopic {
    name: String,
    subscribers: Vec<String>,
    sender: broadcast::Sender<PubSubMessage>,
}

/// Mock queue for local testing
#[derive(Debug)]
struct MockQueue {
    name: String,
    topic_subscription: Option<String>,
    messages: Arc<Mutex<Vec<PubSubMessage>>>,
    receiver: Option<broadcast::Receiver<PubSubMessage>>,
}

/// Message structure for pub/sub communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PubSubMessage {
    pub id: String,
    pub topic: String,
    pub payload: Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub attributes: HashMap<String, String>,
}

impl MockPubSubService {
    /// Create a new mock pub/sub service
    pub fn new() -> Self {
        Self {
            topics: Mutex::new(HashMap::new()),
            queues: Mutex::new(HashMap::new()),
        }
    }

    /// Create a new topic
    pub fn create_topic(&self, topic_name: &str) -> RustezeResult<()> {
        let mut topics = self.topics.lock().unwrap();

        if topics.contains_key(topic_name) {
            return Ok(()); // Topic already exists
        }

        let (sender, _) = broadcast::channel(1000); // Buffer size of 1000 messages

        let topic = MockTopic {
            name: topic_name.to_string(),
            subscribers: Vec::new(),
            sender,
        };

        topics.insert(topic_name.to_string(), topic);
        println!("📢 Created mock topic: {}", topic_name);

        Ok(())
    }

    /// Create a new queue and optionally subscribe it to a topic
    pub fn create_queue(&self, queue_name: &str, topic_name: Option<&str>) -> RustezeResult<()> {
        let mut queues = self.queues.lock().unwrap();

        if queues.contains_key(queue_name) {
            return Ok(()); // Queue already exists
        }

        let receiver = if let Some(topic) = topic_name {
            // Subscribe to topic
            let topics = self.topics.lock().unwrap();
            if let Some(mock_topic) = topics.get(topic) {
                Some(mock_topic.sender.subscribe())
            } else {
                return Err(RustezeError::local_server(format!(
                    "Topic '{}' does not exist",
                    topic
                )));
            }
        } else {
            None
        };

        let queue = MockQueue {
            name: queue_name.to_string(),
            topic_subscription: topic_name.map(|s| s.to_string()),
            messages: Arc::new(Mutex::new(Vec::new())),
            receiver,
        };

        queues.insert(queue_name.to_string(), queue);

        // Update topic subscribers
        if let Some(topic) = topic_name {
            let mut topics = self.topics.lock().unwrap();
            if let Some(mock_topic) = topics.get_mut(topic) {
                mock_topic.subscribers.push(queue_name.to_string());
            }
        }

        println!(
            "📥 Created mock queue: {} (subscribed to: {:?})",
            queue_name, topic_name
        );

        Ok(())
    }

    /// Publish a message to a topic
    pub fn publish_message(
        &self,
        topic_name: &str,
        payload: Value,
        attributes: Option<HashMap<String, String>>,
    ) -> RustezeResult<String> {
        let topics = self.topics.lock().unwrap();

        let topic = topics.get(topic_name).ok_or_else(|| {
            RustezeError::local_server(format!("Topic '{}' does not exist", topic_name))
        })?;

        let message_id = uuid::Uuid::new_v4().to_string();
        let message = PubSubMessage {
            id: message_id.clone(),
            topic: topic_name.to_string(),
            payload,
            timestamp: chrono::Utc::now(),
            attributes: attributes.unwrap_or_default(),
        };

        // Send to all subscribers
        match topic.sender.send(message.clone()) {
            Ok(subscriber_count) => {
                println!(
                    "📤 Published message {} to topic '{}' ({} subscribers)",
                    message_id, topic_name, subscriber_count
                );
            }
            Err(_) => {
                println!(
                    "📤 Published message {} to topic '{}' (no active subscribers)",
                    message_id, topic_name
                );
            }
        }

        Ok(message_id)
    }

    /// Receive messages from a queue (non-blocking)
    pub fn receive_messages(
        &self,
        queue_name: &str,
        max_messages: usize,
    ) -> RustezeResult<Vec<PubSubMessage>> {
        let queues = self.queues.lock().unwrap();

        let queue = queues.get(queue_name).ok_or_else(|| {
            RustezeError::local_server(format!("Queue '{}' does not exist", queue_name))
        })?;

        let mut messages = queue.messages.lock().unwrap();
        let available_messages = std::cmp::min(max_messages, messages.len());
        let received_messages: Vec<PubSubMessage> = messages.drain(0..available_messages).collect();

        if !received_messages.is_empty() {
            println!(
                "📨 Received {} messages from queue '{}'",
                received_messages.len(),
                queue_name
            );
        }

        Ok(received_messages)
    }

    /// Get topic information
    pub fn get_topic_info(&self, topic_name: &str) -> RustezeResult<TopicInfo> {
        let topics = self.topics.lock().unwrap();

        let topic = topics.get(topic_name).ok_or_else(|| {
            RustezeError::local_server(format!("Topic '{}' does not exist", topic_name))
        })?;

        Ok(TopicInfo {
            name: topic.name.clone(),
            subscribers: topic.subscribers.clone(),
            subscriber_count: topic.subscribers.len(),
        })
    }

    /// Get queue information
    pub fn get_queue_info(&self, queue_name: &str) -> RustezeResult<QueueInfo> {
        let queues = self.queues.lock().unwrap();

        let queue = queues.get(queue_name).ok_or_else(|| {
            RustezeError::local_server(format!("Queue '{}' does not exist", queue_name))
        })?;

        let message_count = queue.messages.lock().unwrap().len();

        Ok(QueueInfo {
            name: queue.name.clone(),
            topic_subscription: queue.topic_subscription.clone(),
            message_count,
        })
    }

    /// List all topics
    pub fn list_topics(&self) -> Vec<String> {
        let topics = self.topics.lock().unwrap();
        topics.keys().cloned().collect()
    }

    /// List all queues
    pub fn list_queues(&self) -> Vec<String> {
        let queues = self.queues.lock().unwrap();
        queues.keys().cloned().collect()
    }

    /// Start background message processing for queues
    pub async fn start_message_processing(&self) {
        let queues = self.queues.lock().unwrap();

        for (queue_name, queue) in queues.iter() {
            if let Some(mut receiver) = queue.receiver.as_ref().map(|r| r.resubscribe()) {
                let queue_name = queue_name.clone();
                let messages = queue.messages.clone();

                tokio::spawn(async move {
                    while let Ok(message) = receiver.recv().await {
                        let mut queue_messages = messages.lock().unwrap();
                        queue_messages.push(message.clone());
                        println!("📬 Queued message {} in queue '{}'", message.id, queue_name);
                    }
                });
            }
        }
    }
}

/// Topic information for debugging and testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicInfo {
    pub name: String,
    pub subscribers: Vec<String>,
    pub subscriber_count: usize,
}

/// Queue information for debugging and testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueInfo {
    pub name: String,
    pub topic_subscription: Option<String>,
    pub message_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;

    #[test]
    fn test_route_pattern_matching() {
        // Test exact match
        let params = match_route_pattern("GET /hello", &Method::GET, "/hello");
        assert!(params.is_some());
        assert!(params.unwrap().is_empty());

        // Test parameter extraction
        let params = match_route_pattern("GET /hello/{name}", &Method::GET, "/hello/world");
        assert!(params.is_some());
        let params = params.unwrap();
        assert_eq!(params.get("name"), Some(&"world".to_string()));

        // Test method mismatch
        let params = match_route_pattern("POST /hello", &Method::GET, "/hello");
        assert!(params.is_none());

        // Test path mismatch
        let params = match_route_pattern("GET /hello", &Method::GET, "/goodbye");
        assert!(params.is_none());

        // Test multiple parameters
        let params = match_route_pattern(
            "GET /users/{id}/posts/{post_id}",
            &Method::GET,
            "/users/123/posts/456",
        );
        assert!(params.is_some());
        let params = params.unwrap();
        assert_eq!(params.get("id"), Some(&"123".to_string()));
        assert_eq!(params.get("post_id"), Some(&"456".to_string()));
    }

    #[tokio::test]
    async fn test_local_server_creation() {
        use crate::config::{DeploymentConfig, RustezeConfig};

        let config = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: None,
            pubsub: None,
            environment: None,
            environments: None,
        };

        let server = LocalServer::new(config);
        assert_eq!(server.routes.len(), 0);
        assert!(server.router.is_none());
    }

    #[test]
    fn test_metadata_route_handler_creation() {
        let function_metadata = FunctionMetadata {
            name: "test_function".to_string(),
            path: "/test/{id}".to_string(),
            method: "GET".to_string(),
            file: "src/test.rs".to_string(),
            handler_type: "HttpRoute".to_string(),
            signature: FunctionSignature {
                return_type: "String".to_string(),
                is_async: false,
                parameters: vec![ParameterMetadata {
                    name: "id".to_string(),
                    param_type: "String".to_string(),
                    source: "Path".to_string(),
                }],
            },
            deployment_metadata: DeploymentConfig {
                memory_mb: Some(256),
                timeout_seconds: Some(30),
                permissions: vec!["lambda:InvokeFunction".to_string()],
                environment_variables: HashMap::new(),
            },
        };

        let function_registry = Arc::new(Mutex::new(FunctionRegistry::new()));
        let handler = MetadataRouteHandler::new(
            "test_function".to_string(),
            function_metadata,
            function_registry,
        );
        assert_eq!(handler.function_name, "test_function");
        assert_eq!(handler.function_metadata.path, "/test/{id}");
    }

    #[tokio::test]
    async fn test_metadata_route_handler_request_transformation() {
        let function_metadata = FunctionMetadata {
            name: "test_function".to_string(),
            path: "/test/{id}".to_string(),
            method: "GET".to_string(),
            file: "src/test.rs".to_string(),
            handler_type: "HttpRoute".to_string(),
            signature: FunctionSignature {
                return_type: "String".to_string(),
                is_async: false,
                parameters: vec![],
            },
            deployment_metadata: DeploymentConfig {
                memory_mb: Some(256),
                timeout_seconds: Some(30),
                permissions: vec![],
                environment_variables: HashMap::new(),
            },
        };

        let function_registry = Arc::new(Mutex::new(FunctionRegistry::new()));
        let handler = MetadataRouteHandler::new(
            "test_function".to_string(),
            function_metadata,
            function_registry,
        );

        let mut path_params = HashMap::new();
        path_params.insert("id".to_string(), "123".to_string());

        let request = HttpRequest {
            method: Method::GET,
            path: "/test/123".to_string(),
            path_params,
            query_params: HashMap::new(),
            headers: HashMap::new(),
            body: None,
        };

        let response = handler.handle(request).await;
        assert_eq!(response.status_code, StatusCode::OK);
        assert!(response.body.is_some());

        let body = response.body.unwrap();
        assert!(body.get("function_metadata").is_some());
        assert!(body.get("message").is_some());
    }

    #[tokio::test]
    async fn test_mock_pub_sub_service() {
        let pub_sub = MockPubSubService::new();

        // Create a topic
        pub_sub.create_topic("test-topic").unwrap();
        assert!(pub_sub.list_topics().contains(&"test-topic".to_string()));

        // Create a queue subscribed to the topic
        pub_sub
            .create_queue("test-queue", Some("test-topic"))
            .unwrap();
        assert!(pub_sub.list_queues().contains(&"test-queue".to_string()));

        // Publish a message
        let payload = serde_json::json!({"message": "Hello, World!"});
        let message_id = pub_sub
            .publish_message("test-topic", payload, None)
            .unwrap();
        assert!(!message_id.is_empty());

        // Give some time for message processing
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Check topic info
        let topic_info = pub_sub.get_topic_info("test-topic").unwrap();
        assert_eq!(topic_info.name, "test-topic");
        assert_eq!(topic_info.subscribers.len(), 1);
        assert!(topic_info.subscribers.contains(&"test-queue".to_string()));

        // Check queue info
        let queue_info = pub_sub.get_queue_info("test-queue").unwrap();
        assert_eq!(queue_info.name, "test-queue");
        assert_eq!(
            queue_info.topic_subscription,
            Some("test-topic".to_string())
        );
    }

    #[tokio::test]
    async fn test_pub_sub_message_flow() {
        let pub_sub = MockPubSubService::new();

        // Create topic and queue
        pub_sub.create_topic("flow-topic").unwrap();
        pub_sub
            .create_queue("flow-queue", Some("flow-topic"))
            .unwrap();

        // Start message processing
        pub_sub.start_message_processing().await;

        // Publish multiple messages
        for i in 0..3 {
            let payload = serde_json::json!({"message": format!("Message {}", i)});
            pub_sub
                .publish_message("flow-topic", payload, None)
                .unwrap();
        }

        // Give some time for message processing
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Receive messages
        let messages = pub_sub.receive_messages("flow-queue", 10).unwrap();
        assert_eq!(messages.len(), 3);

        for (_i, message) in messages.iter().enumerate() {
            assert_eq!(message.topic, "flow-topic");
            assert!(message.payload.get("message").is_some());
            assert!(!message.id.is_empty());
        }
    }
}

impl LocalServer {
    /// Start the server with hot reload capabilities
    pub async fn start_with_hot_reload(&mut self, addr: SocketAddr) -> RustezeResult<()> {
        println!("🔥 Starting development server with hot reload...");

        // Setup file watcher for hot reload
        let (reload_tx, mut reload_rx) = tokio::sync::mpsc::channel(100);
        let _watcher = self.setup_file_watcher(reload_tx)?;

        loop {
            // Build and start the server
            if let Err(e) = self.build_router() {
                eprintln!("❌ Failed to build router: {}", e);
                println!("⏳ Waiting for file changes...");
                continue;
            }

            let router = self.router.take().unwrap();
            let listener =
                TcpListener::bind(addr)
                    .await
                    .map_err(|e| RustezeError::LocalServer {
                        message: format!("Failed to bind to {}: {}", addr, e),
                        context: None,
                    })?;

            println!("🚀 Local development server running at http://{}", addr);
            println!("📝 Available routes:");
            for route_key in self.routes.keys() {
                println!("  - {}", route_key);
            }

            // Start pub/sub message processing
            self.pub_sub_service.start_message_processing().await;

            println!("📢 Mock pub/sub service initialized:");
            println!("  Topics: {:?}", self.pub_sub_service.list_topics());
            println!("  Queues: {:?}", self.pub_sub_service.list_queues());
            println!("👀 Watching for file changes... (Press Ctrl+C to stop)");

            // Start the server with graceful shutdown
            let server_future = axum::serve(listener, router.into_make_service());

            tokio::select! {
                result = server_future => {
                    if let Err(e) = result {
                        eprintln!("❌ Server error: {}", e);
                    }
                    println!("🛑 Server stopped");
                    break;
                }
                reload_signal = reload_rx.recv() => {
                    if reload_signal.is_some() {
                        println!("🔄 File changes detected, restarting server...");
                        println!("💡 Note: Hot reload restarts the entire process to pick up code changes");
                        // Exit the process - the file watcher will restart it
                        std::process::exit(0);
                    } else {
                        println!("🛑 Reload channel closed");
                        break;
                    }
                }
                _ = signal::ctrl_c() => {
                    println!("\n🛑 Gracefully shutting down server...");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Setup file watcher for hot reload
    fn setup_file_watcher(
        &self,
        reload_tx: tokio::sync::mpsc::Sender<()>,
    ) -> RustezeResult<notify::RecommendedWatcher> {
        let (tx, rx) = mpsc::channel();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    println!("📁 File event: {:?}", event);
                    // Only reload on modify events for Rust files or config files
                    if matches!(event.kind, EventKind::Modify(_)) {
                        let should_reload = event.paths.iter().any(|path| {
                            let should = path.extension().map_or(false, |ext| ext == "rs")
                                || path.file_name().map_or(false, |name| {
                                    name == "rusteze.toml"
                                        || name == "rusteze.deploy.toml"
                                        || name == "Cargo.toml"
                                });

                            if should {
                                println!("🔄 File change detected: {:?}", path);
                            }
                            should
                        });

                        if should_reload {
                            println!("📤 Sending reload signal...");
                            let _ = tx.send(());
                        }
                    }
                }
                Err(e) => eprintln!("File watcher error: {:?}", e),
            }
        })
        .map_err(|e| RustezeError::LocalServer {
            message: format!("Failed to create file watcher: {}", e),
            context: None,
        })?;

        // Watch current directory and src directory
        watcher
            .watch(std::path::Path::new("."), RecursiveMode::Recursive)
            .map_err(|e| RustezeError::LocalServer {
                message: format!("Failed to watch directory: {}", e),
                context: None,
            })?;

        println!("👀 File watcher setup complete, watching current directory recursively");

        // Spawn a task to forward file change notifications
        let reload_tx_clone = reload_tx.clone();
        tokio::spawn(async move {
            while rx.recv().is_ok() {
                println!("📨 Received file change notification");
                // Debounce rapid file changes
                tokio::time::sleep(Duration::from_millis(500)).await;

                // Drain any additional events
                while rx.try_recv().is_ok() {
                    println!("🗑️ Draining additional file change events");
                }

                println!("🔄 Sending reload signal to server...");
                if reload_tx_clone.send(()).await.is_err() {
                    println!("❌ Failed to send reload signal");
                    break;
                }
            }
            println!("🛑 File watcher task ended");
        });

        Ok(watcher)
    }

    /// Start the server with enhanced development logging
    pub async fn start_with_dev_logging(&mut self, addr: SocketAddr) -> RustezeResult<()> {
        // Enable development logging
        self.enable_dev_logging();

        // For now, use regular start method to avoid hanging issues
        // TODO: Fix hot reload implementation
        self.start(addr).await
    }

    /// Enable development logging and debugging utilities
    fn enable_dev_logging(&self) {
        println!("🔧 Development mode enabled");
        println!("📊 Server configuration:");
        println!("  Service: {}", self.config.service_name);
        println!("  Provider: {}", self.config.deployment.provider);
        println!("  Region: {}", self.config.deployment.region);

        if let Some(lambda_config) = &self.config.lambda {
            println!(
                "  Lambda Memory: {} MB",
                lambda_config.memory_size.unwrap_or(256)
            );
            println!(
                "  Lambda Timeout: {} seconds",
                lambda_config.timeout.unwrap_or(30)
            );
        }

        if let Some(pubsub_config) = &self.config.pubsub {
            println!(
                "  PubSub Retention: {} days",
                pubsub_config.message_retention_days.unwrap_or(14)
            );
            println!(
                "  PubSub Visibility Timeout: {} seconds",
                pubsub_config.visibility_timeout_seconds.unwrap_or(30)
            );
        }
    }

    /// Get server health information
    pub fn get_health_info(&self) -> ServerHealthInfo {
        ServerHealthInfo {
            status: "healthy".to_string(),
            routes_count: self.routes.len(),
            topics: self.pub_sub_service.list_topics(),
            queues: self.pub_sub_service.list_queues(),
            uptime_seconds: 0, // Would need to track actual uptime
        }
    }
}

/// Server health information for debugging and monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerHealthInfo {
    pub status: String,
    pub routes_count: usize,
    pub topics: Vec<String>,
    pub queues: Vec<String>,
    pub uptime_seconds: u64,
}

/// Dynamic function implementation that can call actual Rust functions
pub struct DynamicRustFunction {
    function_name: String,
    signature: FunctionSignature,
}

impl DynamicRustFunction {
    pub fn new(function_name: String) -> Self {
        Self {
            function_name: function_name.clone(),
            signature: FunctionSignature {
                return_type: "String".to_string(),
                is_async: false,
                parameters: vec![ParameterMetadata {
                    name: "word".to_string(),
                    param_type: "String".to_string(),
                    source: "Path".to_string(),
                }],
            },
        }
    }
}

impl ExecutableFunction for DynamicRustFunction {
    fn call(&self, params: HashMap<String, String>) -> Result<String, String> {
        println!(
            "🚀 DynamicRustFunction::call - function: {}, params: {:?}",
            self.function_name, params
        );

        // For now, let's implement the hello-world function directly
        // This is a temporary solution until we have proper macro integration

        match self.function_name.as_str() {
            "get_hello" => {
                let word = params.get("word").unwrap_or(&"World".to_string()).clone();
                println!("📝 get_hello called with word: '{}'", word);

                // Implement the actual logic from the hello-world example
                if word.is_empty() {
                    println!("✅ Returning: Hello World!");
                    Ok("Hello World!".to_string())
                } else {
                    // This matches the current implementation in main.rs
                    let result = format!("Hello {} Rusteze!", word);
                    println!("✅ Returning: {}", result);
                    Ok(result)
                }
            }
            _ => {
                println!("❌ Unknown function: {}", self.function_name);
                Err(format!("Unknown function: {}", self.function_name))
            }
        }
    }

    fn get_signature(&self) -> &FunctionSignature {
        &self.signature
    }
}

/// Actual function implementation that calls the real compiled function
/// Callable function implementation that invokes the actual compiled function
#[derive(Clone)]
pub struct CallableRustFunction {
    name: String,
    signature: FunctionSignature,
    metadata: serde_json::Map<String, serde_json::Value>,
}

impl CallableRustFunction {
    pub fn new(name: String, metadata: serde_json::Map<String, serde_json::Value>) -> Self {
        // Extract signature from metadata
        let signature = if let Some(sig) = metadata.get("signature") {
            FunctionSignature {
                return_type: sig
                    .get("return_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("String")
                    .to_string(),
                is_async: sig
                    .get("is_async")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                parameters: vec![ParameterMetadata {
                    name: "word".to_string(),
                    param_type: "String".to_string(),
                    source: "Path".to_string(),
                }],
            }
        } else {
            FunctionSignature {
                return_type: "String".to_string(),
                is_async: false,
                parameters: vec![ParameterMetadata {
                    name: "word".to_string(),
                    param_type: "String".to_string(),
                    source: "Path".to_string(),
                }],
            }
        };

        Self {
            name,
            signature,
            metadata,
        }
    }
}

impl ExecutableFunction for CallableRustFunction {
    fn call(&self, params: HashMap<String, String>) -> Result<String, String> {
        let word = params.get("word").unwrap_or(&"World".to_string()).clone();

        // Try to call the actual function from the compiled library
        match self.call_actual_function(&params) {
            Ok(result) => Ok(result),
            Err(e) => {
                println!("⚠️  Failed to call actual function {}: {}", self.name, e);
                // Fallback to a mock response that indicates we tried to call the real function
                Ok(format!(
                    "Hello {} from {} (actual function call failed, using fallback)!",
                    word, self.name
                ))
            }
        }
    }

    fn get_signature(&self) -> &FunctionSignature {
        &self.signature
    }
}

impl CallableRustFunction {
    /// Attempt to call the actual function
    fn call_actual_function(&self, params: &HashMap<String, String>) -> Result<String, String> {
        // Try to call the actual function by reading the lib.rs implementation
        let word = params.get("word").unwrap_or(&"World".to_string()).clone();

        // For the hello-world example, we know the function signature
        // Let's call the actual get_hello function from lib.rs
        match self.name.as_str() {
            "get_hello_handler" => {
                // Call the actual get_hello function from the lib.rs
                self.call_hello_world_function(word)
            }
            "get_bar_handler" => {
                // Call the actual get_bar_handler function
                self.call_bar_function()
            }
            _ => Err(format!("Unknown function: {}", self.name)),
        }
    }

    /// Call the actual hello-world function from lib.rs
    fn call_hello_world_function(&self, word: String) -> Result<String, String> {
        // Use dynamic compilation to call the actual function
        self.compile_and_call_dynamic_function(word)
    }

    /// Call the actual get_bar_handler function (no parameters)
    fn call_bar_function(&self) -> Result<String, String> {
        // Use dynamic compilation to call the actual function (no parameters)
        self.compile_and_call_dynamic_function_no_params()
    }

    /// Dynamically compile and call the function
    fn compile_and_call_dynamic_function(&self, word: String) -> Result<String, String> {
        use std::process::Command;

        println!("🚀 Calling function {} via cargo run", self.name);

        // Run the user's binary with the function parameters
        let output = Command::new("cargo")
            .args(&["run", "--"])
            .env("RUSTEZE_FUNCTION", &self.name)
            .env("RUSTEZE_PARAM_WORD", &word)
            .env("RUSTEZE_MODE", "function_call")
            .output()
            .map_err(|e| format!("Failed to run cargo: {}", e))?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("✅ Function call successful: {}", result);
            Ok(result)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!("❌ Function call failed: {}", stderr);
            Err(format!("Function execution failed: {}", stderr))
        }
    }

    /// Call a function with no parameters via cargo run
    fn compile_and_call_dynamic_function_no_params(&self) -> Result<String, String> {
        use std::process::Command;

        println!(
            "🚀 Calling function {} via cargo run (no params)",
            self.name
        );

        // Run the user's binary with the function name but no parameters
        let output = Command::new("cargo")
            .args(&["run", "--"])
            .env("RUSTEZE_FUNCTION", &self.name)
            .env("RUSTEZE_MODE", "function_call")
            .output()
            .map_err(|e| format!("Failed to run cargo: {}", e))?;

        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("✅ Function call successful: {}", result);
            Ok(result)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            println!("❌ Function call failed: {}", stderr);
            Err(format!("Function execution failed: {}", stderr))
        }
    }
}

/// Mock function implementation for fallback
#[derive(Clone)]
pub struct MockRustFunction {
    signature: FunctionSignature,
}

impl MockRustFunction {
    pub fn new() -> Self {
        Self {
            signature: FunctionSignature {
                return_type: "String".to_string(),
                is_async: false,
                parameters: vec![ParameterMetadata {
                    name: "word".to_string(),
                    param_type: "String".to_string(),
                    source: "Path".to_string(),
                }],
            },
        }
    }
}

impl ExecutableFunction for MockRustFunction {
    fn call(&self, params: HashMap<String, String>) -> Result<String, String> {
        let word = params.get("word").unwrap_or(&"World".to_string()).clone();
        Ok(format!("Hello {} from mock function (fallback)!", word))
    }

    fn get_signature(&self) -> &FunctionSignature {
        &self.signature
    }
}

#[cfg(test)]
mod hot_reload_tests {
    use super::*;

    #[test]
    fn test_server_health_info() {
        use crate::config::{DeploymentConfig, RustezeConfig};

        let config = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: None,
            pubsub: None,
            environment: None,
            environments: None,
        };

        let server = LocalServer::new(config);
        let health_info = server.get_health_info();

        assert_eq!(health_info.status, "healthy");
        assert_eq!(health_info.routes_count, 0);
        assert_eq!(health_info.topics.len(), 0);
        assert_eq!(health_info.queues.len(), 0);
    }

    #[test]
    fn test_dev_logging_enabled() {
        use crate::config::{DeploymentConfig, LambdaConfig, PubSubConfig, RustezeConfig};

        let config = RustezeConfig {
            service_name: "test-service".to_string(),
            deployment: DeploymentConfig {
                provider: "aws".to_string(),
                region: "us-east-1".to_string(),
                deployment_type: Some("lambda".to_string()),
                stage: Some("prod".to_string()),
            },
            lambda: Some(LambdaConfig {
                memory_size: Some(512),
                timeout: Some(60),
                runtime: Some("provided.al2".to_string()),
                environment_variables: None,
            }),
            pubsub: Some(PubSubConfig {
                message_retention_days: Some(7),
                visibility_timeout_seconds: Some(60),
                max_receive_count: Some(5),
                enable_dead_letter_queues: Some(true),
                topic_prefix: None,
                queue_prefix: None,
                delivery_delay_seconds: Some(0),
                max_message_size_bytes: Some(262144),
                enable_encryption: Some(false),
                kms_key_id: None,
                enable_fifo: Some(false),
                content_based_deduplication: Some(false),
                message_group_id: None,
            }),
            environment: None,
            environments: None,
        };

        let server = LocalServer::new(config);

        // This would normally print to stdout, but we can't easily test that
        // Just ensure the method doesn't panic
        server.enable_dev_logging();
    }
}
