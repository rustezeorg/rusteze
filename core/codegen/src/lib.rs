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

    // Split by comma and parse each key-value pair
    for arg in attr_str.split(',') {
        let arg = arg.trim();
        if let Some((key, value)) = arg.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');

            match key {
                "method" => method = value.to_string(),
                "path" => path = value.to_string(),
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

    // panic!("hello world! {:?}", bin_dir);

    // Ensure directories exist
    create_dir_all(&bin_dir).unwrap();
    let manifest_json_path = rusteze_dir.join("manifest.json");

    // Extract parameter information from function signature
    let params = extract_function_params(&func, &path);

    // Use file locking to prevent race conditions when multiple routes are processed
    crate::config::update_manifest_safely(
        &manifest_json_path,
        &binary_name,
        &method,
        &path,
        func_name,
        &params,
    );

    // === Generate proxy binary source file (Shuttle-style) ===
    let proxy_path = bin_dir.join(format!("{}.rs", binary_name));
    let func_ident = &func.sig.ident;

    // Extract the function definition as a string (without the #[route] attribute)
    let func_str = quote! { #func }.to_string();

    // Generate generic parameter handling and function call
    let (param_setup, function_call) = generate_generic_proxy_code(&func, &path);

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
        r#"// Auto-generated binary for route: {} {}
use serde::{{Deserialize, Serialize}};

// Include all types from the main crate
use {}::*;

{}

#[tokio::main]
async fn main() {{
{}
    let result = {}.await;
    println!("{{}}", result);
}}
"#,
        method, path, crate_name, func_str, param_setup, function_call
    );

    let mut f = File::create(&proxy_path).unwrap();
    f.write_all(proxy_code.to_string().as_bytes()).unwrap();

    // === Generate or update Cargo.toml (Shuttle-style - no external dependencies) ===
    let cargo_toml_path = rusteze_dir.join("Cargo.toml");
    if !cargo_toml_path.exists() {
        // Create a new Cargo.toml without dependency on the main crate (Shuttle-style)
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

[workspace]

[dependencies]
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
tokio = {{ version = "1", features = ["macros", "rt-multi-thread"] }}
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

fn extract_params(path: &str) -> Vec<String> {
    path.split('/')
        .filter_map(|segment| {
            // Handle both :param and {param} syntax
            if segment.starts_with(':') {
                Some(segment[1..].to_string())
            } else if segment.starts_with('{') && segment.ends_with('}') {
                Some(segment[1..segment.len() - 1].to_string())
            } else {
                None
            }
        })
        .collect()
}

fn extract_function_params(func: &ItemFn, route_path: &str) -> Vec<(String, ParameterConfig)> {
    let path_params = extract_params(route_path);
    let mut params = Vec::new();

    for input in &func.sig.inputs {
        if let syn::FnArg::Typed(pat_type) = input {
            if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                let param_name = pat_ident.ident.to_string();
                let param_type = quote! { #pat_type.ty }.to_string();

                // Determine if this is a path parameter or body parameter
                let param_source = if path_params.contains(&param_name) {
                    "path"
                } else {
                    "body" // Could be query, body, or command line depending on HTTP method
                };

                params.push((
                    param_name,
                    ParameterConfig {
                        param_type: clean_type_string(&param_type),
                        source: param_source.to_string(),
                        required: Some(true), // Default to required
                        description: None,    // Could be extracted from doc comments in the future
                    },
                ));
            }
        }
    }

    params
}

fn clean_type_string(type_str: &str) -> String {
    // Clean up the type string from quote! output
    // Input might be like "word : String . ty" or "a : i32 . ty"
    let cleaned = type_str
        .replace(" . ty", "")
        .replace(".ty", "")
        .replace(" : ", ":");

    // Split by colon and take the type part
    if let Some(type_part) = cleaned.split(':').nth(1) {
        type_part.trim().to_string()
    } else {
        // Fallback: just clean up and return
        cleaned.trim().to_string()
    }
}

fn generate_generic_proxy_code(func: &ItemFn, route_path: &str) -> (String, String) {
    let func_name = &func.sig.ident;
    let inputs = &func.sig.inputs;

    // Extract path parameters from route (e.g., "/users/{id}" -> ["id"])
    let path_params = extract_params(route_path);

    let mut param_setup = String::new();
    let mut function_args = Vec::new();

    // Check if this function has a single structured input parameter
    let non_self_inputs: Vec<_> = inputs
        .iter()
        .filter(|input| !matches!(input, syn::FnArg::Receiver(_)))
        .collect();

    if non_self_inputs.len() == 1 {
        if let syn::FnArg::Typed(pat_type) = non_self_inputs[0] {
            if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                let param_name = &pat_ident.ident;
                let param_type = &pat_type.ty;
                let type_str = quote! { #param_type }.to_string();

                // Check if this is a path parameter (simple types like String, i32, bool)
                if path_params.contains(&param_name.to_string()) {
                    // Handle path parameter
                    if type_str.contains("String") {
                        param_setup.push_str(&format!(
                            "    // Extract {} from URL path parameter\n    let args: Vec<String> = std::env::args().collect();\n    let {} = if args.len() > 1 {{\n        args[1].clone()\n    }} else {{\n        String::new()\n    }};\n",
                            param_name, param_name
                        ));
                    } else if type_str.contains("i32")
                        || type_str.contains("u32")
                        || type_str.contains("i64")
                        || type_str.contains("u64")
                    {
                        param_setup.push_str(&format!(
                            "    // Extract {} from URL path parameter\n    let args: Vec<String> = std::env::args().collect();\n    let {} = if args.len() > 1 {{\n        args[1].parse().unwrap_or(0)\n    }} else {{\n        0\n    }};\n",
                            param_name, param_name
                        ));
                    } else if type_str.contains("bool") {
                        param_setup.push_str(&format!(
                            "    // Extract {} from URL path parameter\n    let args: Vec<String> = std::env::args().collect();\n    let {} = if args.len() > 1 {{\n        args[1] == \"true\" || args[1] == \"1\" || args[1] == \"yes\"\n    }} else {{\n        false\n    }};\n",
                            param_name, param_name
                        ));
                    }
                } else {
                    // This is likely a structured input type - parse from JSON
                    param_setup.push_str(&format!(
                        "    // Parse structured input from JSON\n    let args: Vec<String> = std::env::args().collect();\n    let {} = if args.len() > 1 {{\n        serde_json::from_str(&args[1]).unwrap_or_else(|e| {{\n            eprintln!(\"Failed to parse JSON input: {{}}\", e);\n            std::process::exit(1);\n        }})\n    }} else {{\n        eprintln!(\"No JSON input provided\");\n        std::process::exit(1);\n    }};\n",
                        param_name
                    ));
                }

                function_args.push(param_name.to_string());
            }
        }
    } else {
        // Handle multiple parameters (legacy approach)
        for (i, input) in non_self_inputs.iter().enumerate() {
            if let syn::FnArg::Typed(pat_type) = input {
                if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                    let param_name = &pat_ident.ident;
                    let param_type = &pat_type.ty;
                    let type_str = quote! { #param_type }.to_string();

                    if path_params.contains(&param_name.to_string()) {
                        // Path parameter
                        if type_str.contains("String") {
                            param_setup.push_str(&format!(
                                "    // Extract {} from URL path parameter\n    let args: Vec<String> = std::env::args().collect();\n    let {} = if args.len() > {} {{\n        args[{}].clone()\n    }} else {{\n        String::new()\n    }};\n",
                                param_name, param_name, i + 1, i + 1
                            ));
                        } else if type_str.contains("i32")
                            || type_str.contains("u32")
                            || type_str.contains("i64")
                            || type_str.contains("u64")
                        {
                            param_setup.push_str(&format!(
                                "    // Extract {} from URL path parameter\n    let args: Vec<String> = std::env::args().collect();\n    let {} = if args.len() > {} {{\n        args[{}].parse().unwrap_or(0)\n    }} else {{\n        0\n    }};\n",
                                param_name, param_name, i + 1, i + 1
                            ));
                        } else if type_str.contains("bool") {
                            param_setup.push_str(&format!(
                                "    // Extract {} from URL path parameter\n    let args: Vec<String> = std::env::args().collect();\n    let {} = if args.len() > {} {{\n        args[{}] == \"true\" || args[{}] == \"1\" || args[{}] == \"yes\"\n    }} else {{\n        false\n    }};\n",
                                param_name, param_name, i + 1, i + 1, i + 1, i + 1
                            ));
                        }
                    }

                    function_args.push(param_name.to_string());
                }
            }
        }
    }

    // Generate function call
    let function_call = if function_args.is_empty() {
        format!("{}()", func_name)
    } else {
        format!("{}({})", func_name, function_args.join(", "))
    };

    (param_setup, function_call)
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
        r#"// Auto-generated subscriber binary for topic: {}

use serde::{{Deserialize, Serialize}};
use {}::*;

{}

#[tokio::main]
async fn main() {{
    // Parse SQS message from command line arguments
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {{
        eprintln!("No message payload provided");
        std::process::exit(1);
    }}

    let message_json = &args[1];
    
    // Parse the SQS message which contains the SNS notification
    match serde_json::from_str::<serde_json::Value>(message_json) {{
        Ok(sqs_message) => {{
            // Extract SNS message from SQS record
            if let Some(records) = sqs_message.get("Records").and_then(|r| r.as_array()) {{
                for record in records {{
                    if let Some(sns) = record.get("Sns") {{
                        if let Some(message) = sns.get("Message").and_then(|m| m.as_str()) {{
                            // Parse the actual message payload
                            match serde_json::from_str(message) {{
                                Ok(payload) => {{
                                    println!("SENDING DATA TO HANDLER!: {{:?}}", payload);
                                    let result = {}(payload).await;
                                    println!("Subscriber processed message: {{:?}}", result);
                                }}
                                Err(e) => {{
                                    eprintln!("Failed to parse message payload: {{}}", e);
                                    std::process::exit(1);
                                }}
                            }}
                        }}
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
                        std::process::exit(1);
                    }}
                }}
            }}
        }}
        Err(e) => {{
            eprintln!("Failed to parse SQS message: {{}}", e);
            std::process::exit(1);
        }}
    }}
}}
"#,
        topic, crate_name, func_str, func_name, func_name
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
