use proc_macro::TokenStream;
use quote::quote;
use rusteze_config::ParameterConfig;
use serde::{Deserialize, Serialize};
use std::fs::read_to_string;
use std::io::Write;
use std::path::Path;
use std::{
    fs::{File, create_dir_all},
    path::PathBuf,
};
use syn::{
    Attribute, Expr, ItemFn, ItemStruct, Lit, Meta, Token, parse_macro_input,
    punctuated::Punctuated,
};

mod api;
mod config;
mod db;
mod util;

#[proc_macro_attribute]
pub fn route(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let func_name = &func.sig.ident;
    // Parse the attribute arguments more robustly
    let attr_str = attr.to_string();

    // if func.sig.asyncness.is_none() {
    //     return syn::Error::new_spanned(
    //         func.sig.fn_token,
    //         "Functions used with #[route] need to be async. Try adding `async fn`.",
    //     )
    //     .to_compile_error()
    //     .into();
    // }

    let mut method = "GET".to_string();
    let mut path = "/".to_string();
    let mut requires_auth = false;

    // Split by comma and parse each key-value pair
    for arg in attr_str.split(',') {
        let arg = arg.trim();
        if let Some((key, value)) = arg.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match key {
                "method" => method = value.to_string(),
                "path" => path = value.to_string(),
                "auth" => requires_auth = value == "true",
                _ => {} // ignore unknown attributes
            }
        }
    }

    // Generate binary name, e.g. get_user_get
    let binary_name = format!("{}_{}", func_name, method.to_lowercase());

    // Paths
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let rusteze_dir = Path::new(&manifest_dir).join(".rusteze");
    let bin_dir = rusteze_dir.join("src/bin");

    // Ensure directories exist
    create_dir_all(&bin_dir).unwrap();
    let manifest_json_path = rusteze_dir.join("manifest.json");

    // Extract parameter information from function signature
    let params = api::extract_function_params(&func, &path);

    // Use file locking to prevent race conditions when multiple routes are processed
    crate::config::update_manifest_safely(
        &manifest_json_path,
        &binary_name,
        &method,
        &path,
        func_name,
        &params,
        requires_auth,
    );

    // === Generate proxy binary source file (Shuttle-style) ===
    let proxy_path = bin_dir.join(format!("{}.rs", binary_name));
    let func_ident = &func.sig.ident;

    // Extract the function definition as a string (without the #[route] attribute)
    let func_str = quote! { #func }.to_string();

    // Generate generic parameter handling and function call
    let (param_setup, function_call) = api::generate_generic_proxy_code(&func, &path);
    let (local_param_setup, local_function_call) =
        api::generate_local_mode_proxy_code(&func, &path);

    // Get the crate name from Cargo.toml
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
    let crate_name = if manifest_path.exists() {
        let cargo_content = read_to_string(&manifest_path).unwrap_or_default();
        // Extract crate name from Cargo.toml
        cargo_content
            .lines()
            .find(|line| line.starts_with("name = "))
            .and_then(|line| line.split('"').nth(1))
            .unwrap_or("main_crate")
            .replace("-", "_") // Convert kebab-case to snake_case for Rust imports
    } else {
        "main_crate".to_string()
    };

    // Generate the binary code that includes the function directly
    let proxy_code = format!(
        r#"// Auto-generated Lambda binary for route: {} {}
use lambda_http::{{run, service_fn, Error, Request, RequestExt}};
use lambda_http::http::{{Response, StatusCode}};
use serde::{{Deserialize, Serialize}};
use serde_json::json;

// Include all types from the main crate
use {}::*;

{}

async fn function_handler(event: Request) -> Result<Response<String>, Error> {{
    println!("Processing Lambda request");
{}
    let result = {}.await;
    
    // Convert result to JSON response
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(serde_json::to_string(&result).unwrap_or_else(|_| result.to_string()))
        .map_err(Box::new)?;
    
    Ok(response)
}}

#[tokio::main]
async fn main() -> Result<(), Error> {{
    // Check if running in local mode
    if std::env::var("RUSTEZE_LOCAL").unwrap_or_default() == "true" {{
        eprintln!("Running in local mode (not Lambda runtime)");
{}
        let result = {}.await;
        
        // Print result to stdout for local server to capture
        println!("{{}}", serde_json::to_string(&result).unwrap_or_else(|_| result.to_string()));
        Ok(())
    }} else {{
        // Run in Lambda mode
        run(service_fn(function_handler)).await
    }}
}}
"#,
        method,
        path,
        crate_name,
        func_str,
        param_setup,
        function_call,
        local_param_setup,
        local_function_call
    );

    let mut f = File::create(&proxy_path).unwrap();
    f.write_all(proxy_code.to_string().as_bytes()).unwrap();

    // === Generate or update Cargo.toml ===
    let cargo_toml_path = rusteze_dir.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        // Create a new Cargo.toml without dependency on the main crate
        // Get the original crate name (with dashes) for the dependency
        let original_crate_name = if manifest_path.exists() {
            let cargo_content = read_to_string(&manifest_path).unwrap_or_default();
            cargo_content
                .lines()
                .find(|line| line.starts_with("name = "))
                .and_then(|line| line.split('"').nth(1))
                .unwrap_or("main-crate")
                .to_string()
        } else {
            "main-crate".to_string()
        };

        let cargo_toml_contents = format!(
            r#"[package]
name = "rusteze_proxy"
version = "0.1.0"
edition = "2021"

# Explicitly declare this is not part of a workspace
[workspace]

[dependencies]
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
tokio = {{ version = "1", features = ["macros", "rt-multi-thread"] }}
lambda_http = "0.14"
lambda_runtime = "0.13"
aws_lambda_events = "0.16"
{} = {{ path = "../" }}
rusteze = {{ path = "../../../core/lib" }}

[[bin]]
name = "{}"
path = "src/bin/{}.rs"
"#,
            original_crate_name, binary_name, binary_name
        );

        let mut f = File::create(&cargo_toml_path).unwrap();
        f.write_all(cargo_toml_contents.as_bytes()).unwrap();
    } else {
        // Append new [[bin]] entry if not present
        let mut cargo_toml = read_to_string(&cargo_toml_path).unwrap();

        let bin_entry = format!(
            r#"
[[bin]]
name = "{}"
path = "src/bin/{}.rs"
"#,
            binary_name, binary_name
        );

        if !cargo_toml.contains(&bin_entry) {
            cargo_toml.push_str(&bin_entry);
            let mut f = File::create(&cargo_toml_path).unwrap();
            f.write_all(cargo_toml.as_bytes()).unwrap();
        }
    }

    // Return the original function unchanged so it stays callable in dev app
    TokenStream::from(quote! { #func })
}

#[proc_macro_attribute]
pub fn publisher(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let func_name = &input.sig.ident;

    let mut topic = "default-topic".to_string();

    // Convert TokenStream to string and parse manually
    let attr_str = attr.to_string();

    // Split by comma and parse each key-value pair
    for pair in attr_str.split(',') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'').trim();

            match key {
                "topic" => topic = value.to_string(),
                _ => {} // Ignore unknown attributes
            }
        }
    }

    // Paths
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let rusteze_dir = Path::new(&manifest_dir).join(".rusteze");
    let manifest_json_path = rusteze_dir.join("manifest.json");

    // Update manifest.json with publisher configuration (topic only, no binary)
    crate::config::update_publisher_manifest(&manifest_json_path, &topic);

    // Return the original function unchanged - publishers don't generate binaries
    TokenStream::from(quote! { #input })
}

#[proc_macro_attribute]
pub fn subscriber(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let func_name = &input.sig.ident;

    // Parse subscriber attributes
    let mut topic = "default-topic".to_string();

    // Convert TokenStream to string and parse manually
    let attr_str = attr.to_string();

    // Split by comma and parse each key-value pair
    for pair in attr_str.split(',') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'').trim();

            match key {
                "topic" => topic = value.to_string(),
                _ => {} // Ignore unknown attributes
            }
        }
    }

    // Generate binary name for subscriber
    let binary_name = format!("{}_subscriber", func_name);

    // Paths
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let rusteze_dir = Path::new(&manifest_dir).join(".rusteze");
    let bin_dir = rusteze_dir.join("src/bin");

    // Ensure directories exist
    create_dir_all(&bin_dir).unwrap();
    let manifest_json_path = rusteze_dir.join("manifest.json");

    // Update manifest.json with subscriber configuration
    crate::config::update_subscriber_manifest(&manifest_json_path, &binary_name, &topic, func_name);

    // Generate subscriber binary
    let proxy_path = bin_dir.join(format!("{}.rs", binary_name));
    let func_str = quote! { #input }.to_string();

    // Get the crate name from Cargo.toml
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
    let crate_name = if manifest_path.exists() {
        let cargo_content = read_to_string(&manifest_path).unwrap_or_default();
        cargo_content
            .lines()
            .find(|line| line.starts_with("name = "))
            .and_then(|line| line.split('"').nth(1))
            .unwrap_or("main_crate")
            .replace("-", "_")
    } else {
        "main_crate".to_string()
    };

    // Generate the subscriber binary code
    let subscriber_code = format!(
        r#"// Auto-generated Lambda subscriber binary for topic: {}
use lambda_runtime::{{run, service_fn, Error, LambdaEvent}};
use aws_lambda_events::event::sqs::SqsEvent;
use serde::{{Deserialize, Serialize}};
use serde_json::json;
use {}::*;

{}

async fn function_handler(event: LambdaEvent<SqsEvent>) -> Result<(), Error> {{
    for record in event.payload.records {{
        // Parse the SQS message which contains the SNS notification
        if let Some(body) = record.body {{
            match serde_json::from_str::<serde_json::Value>(&body) {{
                Ok(sqs_message) => {{
                    // Extract SNS message from SQS record
                    if let Some(message) = sqs_message.get("Message").and_then(|m| m.as_str()) {{
                        // Parse the actual message payload
                        match serde_json::from_str(message) {{
                            Ok(payload) => {{
                                println!("Processing message: {{:?}}", payload);
                                let result = {}(payload).await;
                                println!("Subscriber processed message: {{:?}}", result);
                            }}
                            Err(e) => {{
                                eprintln!("Failed to parse message payload: {{}}", e);
                                return Err(e.into());
                            }}
                        }}
                    }} else {{
                        // Direct message format (for testing)
                        match serde_json::from_value(sqs_message) {{
                            Ok(payload) => {{
                                let result = {}(payload).await;
                                println!("Subscriber processed message: {{:?}}", result);
                            }}
                            Err(e) => {{
                                eprintln!("Failed to parse direct message: {{}}", e);
                                return Err(e.into());
                            }}
                        }}
                    }}
                }}
                Err(e) => {{
                    eprintln!("Failed to parse SQS message: {{}}", e);
                    return Err(e.into());
                }}
            }}
        }}
    }}
    Ok(())
}}

#[tokio::main]
async fn main() -> Result<(), Error> {{
    // Check if running in local mode
    if std::env::var("RUSTEZE_LOCAL").unwrap_or_default() == "true" {{
        eprintln!("Running subscriber in local mode (not Lambda runtime)");
        
        // Read message from command line args
        let args: Vec<String> = std::env::args().collect();
        if args.len() > 1 {{
            let message_json = &args[1];
            match serde_json::from_str(message_json) {{
                Ok(payload) => {{
                    let result = {}(payload).await;
                    eprintln!("Subscriber processed message: {{:?}}", result);
                    Ok(())
                }}
                Err(e) => {{
                    eprintln!("Failed to parse message: {{}}", e);
                    Err(e.into())
                }}
            }}
        }} else {{
            eprintln!("No message provided");
            Ok(())
        }}
    }} else {{
        // Run in Lambda mode
        run(service_fn(function_handler)).await
    }}
}}
"#,
        topic, crate_name, func_str, func_name, func_name, func_name
    );

    let mut f = File::create(&proxy_path).unwrap();
    f.write_all(subscriber_code.as_bytes()).unwrap();

    // Update Cargo.toml with new binary
    let cargo_toml_path = rusteze_dir.join("Cargo.toml");
    if cargo_toml_path.exists() {
        let mut cargo_toml = read_to_string(&cargo_toml_path).unwrap();

        let bin_entry = format!(
            r#"
[[bin]]
name = "{}"
path = "src/bin/{}.rs"
"#,
            binary_name, binary_name
        );

        if !cargo_toml.contains(&bin_entry) {
            cargo_toml.push_str(&bin_entry);
            let mut f = File::create(&cargo_toml_path).unwrap();
            f.write_all(cargo_toml.as_bytes()).unwrap();
        }
    }

    // Return the original function unchanged
    TokenStream::from(quote! { #input })
}

#[proc_macro_attribute]
pub fn auth(attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);
    let func_name = &func.sig.ident;

    // Parse optional attributes (for future extensibility)
    let attr_str = attr.to_string();
    let mut _auth_type = "default".to_string();

    // Parse attributes if provided
    for arg in attr_str.split(',') {
        let arg = arg.trim();
        if let Some((key, value)) = arg.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match key {
                "type" => _auth_type = value.to_string(),
                _ => {} // ignore unknown attributes
            }
        }
    }

    // Generate binary name for auth handler
    let binary_name = format!("{}_auth", func_name);

    // Paths
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let rusteze_dir = Path::new(&manifest_dir).join(".rusteze");
    let bin_dir = rusteze_dir.join("src/bin");

    // Ensure directories exist
    create_dir_all(&bin_dir).unwrap();
    let manifest_json_path = rusteze_dir.join("manifest.json");

    // Update manifest.json with auth handler configuration
    crate::config::update_auth_manifest(&manifest_json_path, func_name, &binary_name);

    // Generate auth handler binary
    let proxy_path = bin_dir.join(format!("{}.rs", binary_name));
    let func_str = quote! { #func }.to_string();

    // Get the crate name from Cargo.toml
    let manifest_path = Path::new(&manifest_dir).join("Cargo.toml");
    let crate_name = if manifest_path.exists() {
        let cargo_content = read_to_string(&manifest_path).unwrap_or_default();
        cargo_content
            .lines()
            .find(|line| line.starts_with("name = "))
            .and_then(|line| line.split('"').nth(1))
            .unwrap_or("main_crate")
            .replace("-", "_")
    } else {
        "main_crate".to_string()
    };

    // Generate the auth handler binary code
    let auth_code = format!(
        r#"// Auto-generated Lambda auth handler binary
use lambda_http::{{run, service_fn, Error, Request, RequestExt}};
use lambda_http::http::{{Response, StatusCode}};
use serde::{{Deserialize, Serialize}};
use serde_json::json;
use {}::*;

async fn function_handler(event: Request) -> Result<Response<String>, Error> {{
    println!("Auth context: {{:?}}", event.headers());
    
    // Call the auth handler from the imported crate
    {}().await;
    println!("Auth check passed");
    
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(json!({{"authorized": true}}).to_string())
        .map_err(Box::new)?;
    
    Ok(response)
}}

#[tokio::main]
async fn main() -> Result<(), Error> {{
    // Check if running in local mode
    if std::env::var("RUSTEZE_LOCAL").unwrap_or_default() == "true" {{
        eprintln!("Running auth handler in local mode (not Lambda runtime)");
        
        // Call the auth handler directly
        {}().await;
        eprintln!("Auth check passed");
        
        // Print result to stdout
        println!("{{}}", json!({{"authorized": true}}));
        Ok(())
    }} else {{
        // Run in Lambda mode
        run(service_fn(function_handler)).await
    }}
}}
"#,
        crate_name, func_name, func_name
    );

    let mut f = File::create(&proxy_path).unwrap();
    f.write_all(auth_code.as_bytes()).unwrap();

    // Update Cargo.toml with new binary
    let cargo_toml_path = rusteze_dir.join("Cargo.toml");
    if cargo_toml_path.exists() {
        let mut cargo_toml = read_to_string(&cargo_toml_path).unwrap();

        let bin_entry = format!(
            r#"
[[bin]]
name = "{}"
path = "src/bin/{}.rs"
"#,
            binary_name, binary_name
        );

        if !cargo_toml.contains(&bin_entry) {
            cargo_toml.push_str(&bin_entry);
            let mut f = File::create(&cargo_toml_path).unwrap();
            f.write_all(cargo_toml.as_bytes()).unwrap();
        }
    }

    // Return the original function unchanged
    TokenStream::from(quote! { #func })
}

// #[proc_macro_attribute]
// pub fn db(args: TokenStream, input: TokenStream) -> TokenStream {
//     let item = parse_macro_input!(input as ItemStruct);
//     let ident = &item.ident;

//     let metas = parse_macro_input!(args with Punctuated::<Meta, Token![,]>::parse_terminated);

//     // // let input = parse_macro_input!(item as ItemFn);
//     // let args = parse_macro_input!(args as Attribute);
//     // // let func_name = &input.sig.ident;
//     // let input = parse_macro_input!(input as ItemStruct);

//     // // Convert TokenStream to string and parse manually
//     // // let attr_str = attr.to_string();
//     // let a = args.parse_args().unwrap();
//     let db_name = get_db_attributes(attr_str);

//     let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
//     let rusteze_dir = Path::new(&manifest_dir).join(".rusteze");
//     let manifest_path = rusteze_dir.join("manifest.json");

//     // Update manifest_for_db
//     update_db_manifest(&manifest_path, &db_name);

//     TokenStream::from(quote! { #input })
// }
