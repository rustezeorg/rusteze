#[doc(inline)]
pub use rusteze_codegen::*;

use anyhow::Result;
use axum::{
    Router,
    extract::{Json, Path},
    http::StatusCode,
    response::IntoResponse,
    routing::{IntoMakeService, MethodRouter},
};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    process::Stdio,
    sync::{Arc, mpsc},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    signal,
    sync::{Notify, RwLock},
};

use aws_lambda_events::chrono::{self, DateTime, Utc};
use rusteze_config::{DeploymentConfig, ParameterConfig, ProjectConfig, RoutesConfig};

use crate::pubsub::service::{PubSubMessage, PubSubService, SnsRecord, SnsRecords};

#[derive(Debug, Deserialize, Clone)]
struct RouteEntry {
    method: String,
    path: String,
    binary: String,
    function: String,
    parameters: Option<serde_json::Value>,
}

type Routes = Vec<RouteEntry>;

pub struct LocalDevServerOptions {
    pub hot_reload: bool,
}

pub struct LocalDevServer {
    pub port: u32,
    pub options: Option<LocalDevServerOptions>,
}

#[derive(Clone)]
struct AppState {
    pubsub: Arc<PubSubService>,
}

async fn setup_server() -> Result<Router> {
    println!("🚀 Starting Rusteze development server...");
    let routes = Arc::new(load_routes()?);

    // Build router dynamically based on routes.json
    let mut app = Router::new();

    for route in routes.iter() {
        let binary = route.binary.clone();

        let handler = create_handler_with_path_params(binary, route.clone());

        // match method string to Axum MethodRouter
        let method = route.method.to_uppercase();
        let method_router = match method.as_str() {
            "GET" => MethodRouter::new().get(handler),
            "POST" => MethodRouter::new().post(handler),
            "PUT" => MethodRouter::new().put(handler),
            "DELETE" => MethodRouter::new().delete(handler),
            _ => {
                println!("Unsupported method {} for route {}", method, route.path);
                continue;
            }
        };

        app = app.route(&route.path, method_router);
    }

    // Build the rusteze crate binaries (runs cargo build -p rusteze)
    build_rusteze().await?;

    // Log all the routes we're handling
    println!("📝 Available routes:");
    for route in routes.iter() {
        println!("  - {} {}", &route.method, &route.path);
    }

    // Create a single catch-all handler that can dispatch to any route
    let fallback_handler = move || async move {
        println!("Route not found");
        (StatusCode::NOT_FOUND, "Not found")
    };

    // Register the fallback handler
    app = app.fallback(fallback_handler);

    // Register the pubsub service.
    let pub_sub_service = load_pubsubs().await?.unwrap();

    let pubsub_arc = Arc::new(pub_sub_service);

    let p = "/rusteze/topic/{topic-name}";

    let state = AppState {
        pubsub: pubsub_arc, // clone the Arc for each route
    };

    let app = app
        .route(&p, MethodRouter::new().post(pub_sub_route_handler))
        .with_state(state);

    println!("  - {} {}", "POST", &p);

    Ok(app)
}

pub async fn start_dev_server(port: u16, hot_reload: bool) -> Result<()> {
    if hot_reload {
        println!("🔥 Starting development server with hot reload...");

        let (reload_tx, mut reload_rx) = tokio::sync::mpsc::channel(100);
        let _watcher = setup_file_watcher(reload_tx)?;

        loop {
            let router = match setup_server().await {
                Ok(res) => res,
                Err(e) => {
                    println!("❌ Failed to build router: {}", e);
                    println!("⏳ Waiting for file changes...");

                    // Wait for reload signal before trying again
                    if let Some(_) = reload_rx.recv().await {
                        println!("🔄 Retrying after file changes...");
                        continue;
                    } else {
                        println!("🛑 Reload channel closed");
                        break;
                    }
                }
            };

            let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
            println!("🚀 Local development server running at http://{}", addr);

            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

            // Create a graceful shutdown signal
            let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
            let server_future = axum::serve(listener, router.into_make_service())
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                });

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
                        println!("💡 Note: Hot reload restarts the server to pick up code changes");

                        // Signal graceful shutdown
                        let _ = shutdown_tx.send(());

                        // Give the server a moment to shut down
                        tokio::time::sleep(Duration::from_millis(100)).await;

                        // Continue the loop to restart the server
                        continue;
                    } else {
                        println!("🛑 Reload channel closed");
                        break;
                    }
                }
                _ = signal::ctrl_c() => {
                    println!("\n🛑 Gracefully shutting down server...");
                    let _ = shutdown_tx.send(());
                    break;
                }
            }
        }
    } else {
        let router = setup_server().await?;
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        println!("🚀 Local development server running at http://{}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

        axum::serve(listener, router).await.unwrap();
    }

    Ok(())
}

fn setup_file_watcher(
    reload_tx: tokio::sync::mpsc::Sender<()>,
) -> Result<notify::RecommendedWatcher> {
    let (tx, rx) = mpsc::channel();

    let mut watcher =
        notify::recommended_watcher(move |res: Result<Event, notify::Error>| match res {
            Ok(event) => {
                if matches!(event.kind, EventKind::Modify(_)) {
                    let should_reload = event.paths.iter().any(|path| {
                        let should = path.extension().map_or(false, |ext| ext == "rs")
                            || path.file_name().map_or(false, |name| {
                                name == "rusteze.toml"
                                    || name == "cargo.toml"
                                    || name == "routes.json"
                                    || name == "manifest.json"
                            });

                        should
                    });

                    if should_reload {
                        let _ = tx.send(());
                    }
                }
            }
            Err(e) => println!("File watcher error: {:?}", e),
        })?;

    let _ = watcher.watch(std::path::Path::new("."), RecursiveMode::Recursive);

    println!("👀 File watcher setup complete, watching current directory recursively");

    // Spawn a task to forward file change notifications
    let reload_tx_clone = reload_tx.clone();

    tokio::spawn(async move {
        while rx.recv().is_ok() {
            println!("📨 Received file change notification");

            tokio::time::sleep(Duration::from_millis(2000)).await;

            while rx.try_recv().is_ok() {
                // println!("🗑️ Draining additional file change events");
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

async fn build_rusteze() -> Result<()> {
    println!("Building rusteze binaries...");
    let manifest_path = ".rusteze/Cargo.toml";
    let status = Command::new("cargo")
        .env("RUSTFLAGS", "-Awarnings")
        .args(["build", "--manifest-path", manifest_path])
        .status()
        .await?;

    if !status.success() {
        anyhow::bail!("Failed to build rusteze handlers");
    }
    Ok(())
}

fn get_binary_path(binary_name: &str) -> PathBuf {
    let binary_path = {
        let mut p = PathBuf::from(".rusteze/target/debug");
        if cfg!(windows) {
            p.push(format!("{}.exe", binary_name));
        } else {
            p.push(binary_name);
        }
        p
    };
    binary_path
}

async fn run_binary_handler(binary_name: &str, args: Vec<String>) -> Result<()> {
    let binary_path = get_binary_path(binary_name);

    println!(
        "Final args for binary {:?} {}: {:?}",
        &binary_path, binary_name, args
    );

    let mut child = Command::new(binary_path)
        .env("RUSTEZE_LOCAL", "true")
        .env("RUSTEZE_PORT", "3000")
        // These are lambda specific
        .env("AWS_LAMBDA_FUNCTION_NAME", &binary_name)
        .env("AWS_LAMBDA_FUNCTION_MEMORY_SIZE", "128")
        .env("AWS_LAMBDA_FUNCTION_VERSION", "1.0")
        .env("AWS_LAMBDA_RUNTIME_API", "1.0")
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(stdout) = child.stdout.take() {
        let mut reader = BufReader::new(stdout).lines();
        let stdout_handler_name = binary_name.to_string();
        tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                println!("[stdout {}] {}", &stdout_handler_name, line)
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        let mut reader = BufReader::new(stderr).lines();
        let stderr_handler_name = binary_name.to_string();
        tokio::spawn(async move {
            while let Ok(Some(line)) = reader.next_line().await {
                println!("[stderr {}] {}", &stderr_handler_name, line)
            }
        });
    }

    let status = child.wait().await?;

    if status.success() {
        println!("Handler finished successfully.");
        Ok(())
    } else {
        anyhow::bail!("Handler binary failed: {}", status);
    }
}

async fn run_handler_binary_with_params(
    binary_name: &str,
    route: &RouteEntry,
    path_params: std::collections::HashMap<String, String>,
    body: &[u8],
) -> Result<()> {
    // let binary_path = get_binary_path(binary_name);

    // Build command line arguments based on function parameters
    let mut args = Vec::new();

    println!(
        "Building args for binary {} with route params: {:?} and path_params: {:?}",
        binary_name, route.parameters, path_params
    );

    // Determine if this route uses structured input (POST/PUT with single struct parameter)
    // or individual parameters (GET with path params)
    let method = &route.method;
    let has_body_params = route
        .parameters
        .as_ref()
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .any(|param| param["source"].as_str() == Some("body"))
        })
        .unwrap_or(false);

    if (method == "POST" || method == "PUT") && !body.is_empty() {
        // For POST/PUT methods, pass the entire JSON body as a single argument
        let body_str = String::from_utf8_lossy(body);
        println!("Passing JSON body as single argument: {}", body_str);
        args.push(body_str.to_string());
    } else if let Some(params) = &route.parameters {
        // For GET methods or methods with path parameters, extract individual parameters
        if let Some(param_array) = params.as_array() {
            for param in param_array {
                let param_name = param["name"].as_str().unwrap_or("");
                let param_source = param["source"].as_str().unwrap_or("body");

                println!(
                    "Processing param: {} (source: {})",
                    param_name, param_source
                );

                if param_source == "path" {
                    // Extract from path parameters
                    if let Some(value) = path_params.get(param_name) {
                        println!("Found path param {}: {}", param_name, value);
                        args.push(value.clone());
                    } else {
                        println!("Path param {} not found, using empty string", param_name);
                        args.push(String::new()); // Default empty value
                    }
                }
            }
        }
    }

    let _ = run_binary_handler(binary_name, args).await;
    Ok(())
}

fn extract_path_params_from_uri(
    uri_path: &str,
    route_pattern: &str,
) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();

    let uri_segments: Vec<&str> = uri_path.split('/').collect();
    let pattern_segments: Vec<&str> = route_pattern.split('/').collect();

    for (uri_seg, pattern_seg) in uri_segments.iter().zip(pattern_segments.iter()) {
        if pattern_seg.starts_with('{') && pattern_seg.ends_with('}') {
            let param_name = &pattern_seg[1..pattern_seg.len() - 1]; // Remove the '{' and '}'
            params.insert(param_name.to_string(), uri_seg.to_string());
        }
    }

    params
}

fn load_routes() -> Result<Routes> {
    let rusteze_dir = std::path::Path::new(".rusteze");

    // Try to load from manifest.json first, fallback to routes.json for backward compatibility
    let manifest_file = rusteze_dir.join("manifest.json");
    let json_file = rusteze_dir.join("routes.json");

    if manifest_file.exists() {
        let data = match std::fs::read_to_string(&manifest_file) {
            Ok(f) => f,
            Err(e) => {
                println!("Unable to read file: {:?}", &manifest_file);
                return Err(e.into());
            }
        };

        // Parse JSON and extract routes
        let config: RoutesConfig = serde_json::from_str(&data)?;
        let routes: Routes = config
            .route
            .into_iter()
            .map(|(function_name, route)| RouteEntry {
                method: route.method,
                path: route.path,
                binary: route.binary,
                function: function_name,
                parameters: route.parameters.map(|params| {
                    serde_json::Value::Array(
                        params
                            .into_iter()
                            .map(|(param_name, param)| {
                                serde_json::json!({
                                    "name": param_name,
                                    "type": param.param_type,
                                    "source": param.source,
                                })
                            })
                            .collect(),
                    )
                }),
            })
            .collect();

        Ok(routes)
    } else if json_file.exists() {
        // Fallback to JSON for backward compatibility
        let data = match std::fs::read_to_string(&json_file) {
            Ok(f) => f,
            Err(e) => {
                println!("Unable to find file: {:?}", &json_file);
                return Err(e.into());
            }
        };

        let routes: Routes = serde_json::from_str(&data)?;
        Ok(routes)
    } else {
        Err(anyhow::anyhow!(
            "No routes file found. Expected manifest.json or routes.json in .rusteze directory"
        ))
    }
}

// Helper function to create handlers with proper path parameter extraction
fn create_handler_with_path_params(
    binary: String,
    route: RouteEntry,
) -> impl Fn(
    axum::extract::Request,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = axum::response::Response> + Send>>
+ Clone
+ Send
+ 'static {
    move |req: axum::extract::Request| {
        let binary = binary.clone();
        let route = route.clone();

        Box::pin(async move {
            // Extract path parameters manually from the request URI
            let uri = req.uri().clone();
            let path_params = extract_path_params_from_uri(uri.path(), &route.path);

            // Extract body
            let body = match axum::body::to_bytes(req.into_body(), usize::MAX).await {
                Ok(bytes) => bytes,
                Err(e) => {
                    eprintln!("Failed to read request body: {:?}", e);
                    return (StatusCode::BAD_REQUEST, "Failed to read request body")
                        .into_response();
                }
            };

            println!(
                "Handling request for route: {} {} with params: {:?}",
                route.method, route.path, path_params
            );

            match run_handler_binary_with_params(&binary, &route, path_params, &body).await {
                Ok(output) => (StatusCode::OK, output).into_response(),
                Err(e) => {
                    println!("Error running handler binary: {:?}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Handler error").into_response()
                }
            }
        })
    }
}

async fn load_manifest_config() -> anyhow::Result<RoutesConfig> {
    let rusteze_dir = std::path::Path::new(".rusteze");
    let manifest_file = rusteze_dir.join("manifest.json");

    if manifest_file.exists() {
        let data = match std::fs::read_to_string(&manifest_file) {
            Ok(f) => f,
            Err(e) => {
                println!("Unable to read file: {:?}", &manifest_file);
                return Err(e.into());
            }
        };

        let config: RoutesConfig = serde_json::from_str(&data)?;
        return Ok(config);
    }

    return Err(anyhow::anyhow!("Unable to load manifest.json file"));
}

// @todo - look into this as i don't think we really need it?
async fn load_pubsubs() -> anyhow::Result<Option<PubSubService>> {
    let manifest = load_manifest_config().await?;

    if manifest.topic.is_none() {
        return Ok(None);
    }

    // Here we need to create a HashMap of topics, and then attach a subscriber to each
    // so that we can then spawn a process to invoke each binary with it.
    let service = PubSubService::new();

    for (topic_name, topic_config) in manifest.topic.unwrap().into_iter() {
        let topic = service.create_topic(&topic_name, None).await?;

        for (queue_name, queue_config) in topic_config.subscribers.into_iter() {
            let _ = service
                .create_queue(&queue_name, Some(&topic_name), &queue_config.binary)
                .await;

            let mut receiver = topic.sender.subscribe();
            let subscriber_name = queue_name.clone();
            let queue_binary = queue_config.binary.clone();

            let handle = tokio::spawn(async move {
                while let Ok(msg) = receiver.recv().await {
                    // @todo - need to now call the binary.
                    println!("Subscriber {} received: {:?}", subscriber_name, msg);

                    let mut args = Vec::new();
                    args.push(json!(msg).to_string());

                    let _ = run_binary_handler(&queue_binary, args).await;
                }
            });
        }
    }

    Ok(Some(service))
}

// @todo - need to accept the payload.
#[axum::debug_handler]
async fn pub_sub_route_handler(
    axum::extract::State(state): axum::extract::State<AppState>,
    Path(topic_name): Path<String>,
    Json(payload): Json<Value>,
) -> axum::response::Response {
    println!("topic-name: {}", &topic_name);
    let topic_sender = {
        let topics = state.pubsub.topics.lock().await;
        println!("TOPICS! {:?}", &topics);
        let topic = match topics.get(&topic_name) {
            Some(topic) => topic,
            None => return (StatusCode::BAD_REQUEST, "Topic doesn't exist").into_response(),
        };

        topic.sender.clone()
    };

    let now_utc: DateTime<Utc> = Utc::now();

    // @todo - Need to update this to switch between different payload types.
    let message = SnsRecords {
        records: vec![PubSubMessage {
            event_version: String::from("1"),
            event_subscription_arn: String::from(""),
            event_source: String::from(""),
            sns: SnsRecord {
                record_type: String::from(""),
                message_id: String::from(""),
                topic_arn: String::from(""),
                subject: String::from(""),
                timestamp: now_utc.to_string(),
                signature_version: String::from(""),
                signature: String::from(""),
                signing_cert_url: String::from(""),
                unsubscribe_url: String::from(""),
                message: payload.to_string(),
            },
        }],
    };

    println!("data sent: {:?}", &message);

    topic_sender.send(message).unwrap();

    (StatusCode::OK, "Message sent").into_response()
}
