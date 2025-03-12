use serde::{Deserialize, Serialize};
use std::{
    fs::{File, read_to_string},
    io::Write,
    path::{Path, PathBuf},
};

// use rust
use rusteze_config::{
    DeploymentConfig, ParameterConfig, ProjectConfig, RouteConfig, RoutesConfig, SubscriberConfig,
    TopicConfig,
};

pub fn create_default_routes_config() -> RoutesConfig {
    RoutesConfig {
        project: Some(ProjectConfig {
            name: "rusteze-project".to_string(),
            version: "0.1.0".to_string(),
        }),
        deployment: Some(DeploymentConfig {
            runtime: Some("rust".to_string()),
            memory: Some("128MB".to_string()),
            timeout: Some("30s".to_string()),
            environment: Some("development".to_string()),
            arn: None,
        }),
        route: std::collections::HashMap::new(),
        topic: Some(std::collections::HashMap::new()),
        db: Some(std::collections::HashMap::new()),
    }
}

pub fn get_config(manifest_path: &Path) -> RoutesConfig {
    let config: RoutesConfig = if manifest_path.exists() {
        let content = read_to_string(manifest_path).unwrap_or_default();
        if content.trim().is_empty() {
            create_default_routes_config()
        } else {
            serde_json::from_str(&content).unwrap_or_else(|_| create_default_routes_config())
        }
    } else {
        create_default_routes_config()
    };

    config
}

pub fn update_publisher_manifest(manifest_path: &Path, topic: &str) {
    let mut config = get_config(manifest_path);

    if config.topic.is_none() {
        config.topic = Some(std::collections::HashMap::new())
    }

    if let Some(ref mut topics) = config.topic {
        if !topics.contains_key(topic) {
            topics.insert(
                topic.to_string(),
                TopicConfig {
                    name: topic.to_string(),
                    subscribers: std::collections::HashMap::new(),
                    description: Some(format!("Topic: {}", topic)),
                },
            );
        }
    }

    let json_string = serde_json::to_string_pretty(&config).unwrap();
    let mut f = File::create(manifest_path).unwrap();
    f.write_all(json_string.as_bytes()).unwrap();
}

pub fn update_subscriber_manifest(
    manifest_path: &Path,
    binary_name: &str,
    topic: &str,
    func_name: &syn::Ident,
) {
    let mut config = get_config(manifest_path);

    if config.topic.is_none() {
        config.topic = Some(std::collections::HashMap::new())
    }

    if let Some(ref mut topics) = config.topic {
        let topic_config = topics.entry(topic.to_string()).or_insert(TopicConfig {
            name: topic.to_string(),
            subscribers: std::collections::HashMap::new(),
            description: Some(format!("Topic: {}", topic)),
        });

        // Add subscriber with full configuration
        let subscriber_name = func_name.to_string();
        topic_config.subscribers.insert(
            subscriber_name,
            SubscriberConfig {
                binary: binary_name.to_string(),
                description: Some(format!("Subscriber for topic: {}", topic)),
                deployment: Some(DeploymentConfig {
                    runtime: Some("rust".to_string()),
                    memory: Some("128MB".to_string()),
                    timeout: Some("30s".to_string()),
                    environment: Some(format!("subscriber,topic:{}", topic)),
                    arn: None,
                }),
            },
        );
    }

    let json_string = serde_json::to_string_pretty(&config).unwrap();
    let mut f = File::create(manifest_path).unwrap();
    f.write_all(json_string.as_bytes()).unwrap();
}

// Helper struct to ensure lock file cleanup
struct LockFileCleanup {
    path: PathBuf,
}

impl Drop for LockFileCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn try_update_manifest(
    manifest_json_path: &Path,
    binary_name: &str,
    method: &str,
    path: &str,
    func_name: &syn::Ident,
    params: &Vec<(String, ParameterConfig)>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs::OpenOptions;

    // Create a lock file to coordinate access
    let lock_path = manifest_json_path.with_extension("json.lock");

    // Try to create the lock file (fails if it already exists)
    let _lock_file = match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(file) => file,
        Err(_) => return Err("Lock file exists".into()),
    };

    // Ensure we clean up the lock file when we're done
    let _cleanup = LockFileCleanup { path: lock_path };

    // Now safely read, modify, and write the manifest.json file
    let mut config = get_config(manifest_json_path);

    // Convert parameters to HashMap
    let parameters_map = if params.is_empty() {
        None
    } else {
        let mut map = std::collections::HashMap::new();
        for (param_name, param) in params {
            map.insert(
                param_name.clone(),
                ParameterConfig {
                    param_type: param.param_type.clone(),
                    source: param.source.clone(),
                    required: param.required,
                    description: param.description.clone(),
                },
            );
        }
        Some(map)
    };

    // Create new route
    let new_route = RouteConfig {
        method: method.to_string(),
        path: path.to_string(),
        binary: binary_name.to_string(),
        description: None, // Could be extracted from doc comments
        parameters: parameters_map,
        deployment: Some(DeploymentConfig {
            runtime: None,
            memory: Some("128MB".to_string()), // @todo - allow this to be overridden
            timeout: Some("30s".to_string()),  // @todo - allow this to be overridden
            environment: None,
            arn: None, // Will be populated after deployment
        }),
    };

    // Insert or update the route using function name as key
    config.route.insert(func_name.to_string(), new_route);

    // Write updated manifest.json
    let json_string = serde_json::to_string_pretty(&config)?;
    let mut f = File::create(manifest_json_path)?;
    f.write_all(json_string.as_bytes())?;

    Ok(())
}

pub fn update_manifest_safely(
    manifest_json_path: &Path,
    binary_name: &str,
    method: &str,
    path: &str,
    func_name: &syn::Ident,
    params: &Vec<(String, ParameterConfig)>,
) {
    use std::time::Duration;

    // Simple retry mechanism with exponential backoff for file locking
    let mut attempts = 0;
    let max_attempts = 10;

    while attempts < max_attempts {
        match try_update_manifest(
            manifest_json_path,
            binary_name,
            method,
            path,
            func_name,
            params,
        ) {
            Ok(()) => return,
            Err(_) => {
                attempts += 1;
                if attempts < max_attempts {
                    // Wait with exponential backoff: 10ms, 20ms, 40ms, etc.
                    let delay = Duration::from_millis(10 * (1 << attempts.min(6)));
                    std::thread::sleep(delay);
                }
            }
        }
    }

    // If all attempts failed, panic with a helpful message
    panic!(
        "Failed to update manifest.json after {} attempts. This might indicate a persistent file locking issue.",
        max_attempts
    );
}
