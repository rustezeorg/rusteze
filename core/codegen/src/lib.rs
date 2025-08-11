use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
};

use proc_macro::TokenStream;
use quote::quote;
use serde::{Deserialize, Serialize};
use syn::{FnArg, ItemFn, Pat, ReturnType, parse_macro_input};

/// Enhanced deployment configuration supporting multiple cloud providers
#[derive(Serialize, Deserialize)]
struct DeploymentConfig {
    /// Service metadata
    service: ServiceConfig,
    /// Cloud provider configurations
    providers: HashMap<String, ProviderConfig>,
    /// Function definitions with enhanced metadata
    functions: BTreeMap<String, FunctionConfig>,
}

#[derive(Serialize, Deserialize)]
struct ServiceConfig {
    name: String,
    version: Option<String>,
    description: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct ProviderConfig {
    /// Provider type (aws, azure, gcp)
    provider_type: String,
    /// Provider-specific configuration
    config: HashMap<String, serde_json::Value>,
}

/// Enhanced function configuration with metadata and signature analysis
#[derive(Serialize, Deserialize)]
struct FunctionConfig {
    name: String,
    path: String,
    method: String,
    file: Option<String>,
    /// Function signature metadata
    signature: FunctionSignature,
    /// Handler type (route, publisher, subscriber)
    handler_type: HandlerType,
    /// Cloud-agnostic deployment metadata
    deployment_metadata: DeploymentMetadata,
}

#[derive(Serialize, Deserialize, Clone)]
struct FunctionSignature {
    /// Input parameters with types
    parameters: Vec<Parameter>,
    /// Return type information
    return_type: String,
    /// Whether function is async
    is_async: bool,
}

#[derive(Serialize, Deserialize, Clone)]
struct Parameter {
    name: String,
    param_type: String,
    /// Whether parameter comes from path, query, body, etc.
    source: ParameterSource,
}

#[derive(Serialize, Deserialize, Clone)]
enum ParameterSource {
    Path,
    Query,
    Body,
    Header,
    Context,
}

#[derive(Serialize, Deserialize)]
enum HandlerType {
    HttpRoute,
    Publisher { topic: String },
    Subscriber { topic: String, queue: String },
}

#[derive(Serialize, Deserialize)]
struct DeploymentMetadata {
    /// Resource requirements
    memory_mb: Option<u32>,
    timeout_seconds: Option<u32>,
    /// Environment variables needed
    environment_variables: HashMap<String, String>,
    /// Required permissions
    permissions: Vec<String>,
}

fn get_project_root() -> PathBuf {
    let mut dir = std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));

    while !dir.join("Cargo.toml").exists() && dir.parent().is_some() {
        dir = dir.parent().unwrap().to_path_buf();
    }

    dir
}

/// Analyze function signature to extract parameter information
fn analyze_function_signature(sig: &syn::Signature) -> FunctionSignature {
    let mut parameters = Vec::new();

    for input in &sig.inputs {
        if let FnArg::Typed(pat_type) = input {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                let param_name = pat_ident.ident.to_string();
                let param_type = quote::quote!(#pat_type.ty).to_string();

                // Determine parameter source based on name and type
                let source = determine_parameter_source(&param_name, &param_type);

                parameters.push(Parameter {
                    name: param_name,
                    param_type,
                    source,
                });
            }
        }
    }

    let return_type = match &sig.output {
        ReturnType::Default => "()".to_string(),
        ReturnType::Type(_, ty) => quote::quote!(#ty).to_string(),
    };

    FunctionSignature {
        parameters,
        return_type,
        is_async: sig.asyncness.is_some(),
    }
}

/// Determine parameter source based on naming conventions and types
fn determine_parameter_source(name: &str, _type_str: &str) -> ParameterSource {
    // Simple heuristics for parameter source detection
    match name {
        name if name.contains("path") || name.contains("id") => ParameterSource::Path,
        name if name.contains("query") => ParameterSource::Query,
        name if name.contains("header") => ParameterSource::Header,
        name if name.contains("body") || name.contains("request") => ParameterSource::Body,
        _ => ParameterSource::Context, // Default for other parameters
    }
}

/// Extract path parameters from route path
fn extract_path_parameters(path: &str) -> Vec<String> {
    let mut params = Vec::new();
    let mut chars = path.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut param = String::new();
            while let Some(ch) = chars.next() {
                if ch == '}' {
                    break;
                }
                param.push(ch);
            }
            if !param.is_empty() {
                params.push(param);
            }
        }
    }

    params
}

/// Create default deployment configuration
fn create_default_deployment_config() -> DeploymentConfig {
    let mut providers = HashMap::new();

    // Add default AWS provider configuration
    let mut aws_config = HashMap::new();
    aws_config.insert(
        "region".to_string(),
        serde_json::Value::String("us-east-1".to_string()),
    );
    aws_config.insert(
        "runtime".to_string(),
        serde_json::Value::String("provided.al2".to_string()),
    );

    providers.insert(
        "aws".to_string(),
        ProviderConfig {
            provider_type: "aws".to_string(),
            config: aws_config,
        },
    );

    DeploymentConfig {
        service: ServiceConfig {
            name: "rusteze-service".to_string(),
            version: Some("0.1.0".to_string()),
            description: Some("Rusteze serverless application".to_string()),
        },
        providers,
        functions: BTreeMap::new(),
    }
}

#[proc_macro_attribute]
pub fn route(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let func_name = &input.sig.ident.to_string();
    let _file_path = file!();

    // Parse attributes using syn for proper parsing
    let mut method = "GET".to_string();
    let mut path = "/".to_string();
    let mut memory_mb: Option<u32> = None;
    let mut timeout_seconds: Option<u32> = None;

    // Convert TokenStream to string and parse manually for now
    // TODO: Use syn::parse for proper attribute parsing
    let attr_str = attr.to_string();

    // Split by comma and parse each key-value pair
    for pair in attr_str.split(',') {
        let pair = pair.trim();
        if let Some((key, value)) = pair.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"').trim_matches('\'').trim();

            match key {
                "method" => method = value.to_string(),
                "path" => path = value.to_string(),
                "memory" => {
                    if let Ok(mem) = value.parse::<u32>() {
                        memory_mb = Some(mem);
                    }
                }
                "timeout" => {
                    if let Ok(timeout) = value.parse::<u32>() {
                        timeout_seconds = Some(timeout);
                    }
                }
                _ => {} // Ignore unknown attributes
            }
        }
    }

    // Analyze function signature for enhanced metadata
    let signature = analyze_function_signature(&input.sig);

    // Extract path parameters for validation
    let path_params = extract_path_parameters(&path);

    // Create deployment metadata
    let environment_variables = HashMap::new();
    let mut permissions = Vec::new();

    // Add basic Lambda permissions
    permissions.push("lambda:InvokeFunction".to_string());

    // Add API Gateway integration permissions if this is an HTTP route
    permissions.push("apigateway:*".to_string());

    let deployment_metadata = DeploymentMetadata {
        memory_mb,
        timeout_seconds,
        environment_variables,
        permissions,
    };

    // Load or create deployment configuration
    let root = get_project_root();
    let deploy_file = root.join("rusteze.deploy.toml");

    let mut config: DeploymentConfig = if let Ok(content) = std::fs::read_to_string(&deploy_file) {
        toml::from_str(&content).unwrap_or_else(|_| create_default_deployment_config())
    } else {
        create_default_deployment_config()
    };

    // Create enhanced function configuration
    let function_config = FunctionConfig {
        name: func_name.to_string(),
        path: path.clone(),
        method: method.clone(),
        file: Some(format!("src/{}.rs", func_name)), // Placeholder for actual file detection
        signature: signature.clone(),
        handler_type: HandlerType::HttpRoute,
        deployment_metadata,
    };

    config.functions.insert(func_name.clone(), function_config);

    // Write updated configuration
    let toml_string = match toml::to_string_pretty(&config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to serialize TOML: {}", e);
            // Fallback to basic structure for debugging
            format!(
                r#"[service]
name = "{}"

[functions.{}]
name = "{}"
path = "{}"
method = "{}"
handler_type = "HttpRoute"

[functions.{}.signature]
is_async = {}
return_type = "{}"

[functions.{}.deployment_metadata]
memory_mb = {}
timeout_seconds = {}
"#,
                config.service.name,
                func_name,
                func_name,
                path,
                method,
                func_name,
                signature.is_async,
                signature.return_type,
                func_name,
                memory_mb.map_or("null".to_string(), |m| m.to_string()),
                timeout_seconds.map_or("null".to_string(), |t| t.to_string())
            )
        }
    };

    let mut file = BufWriter::new(File::create(&deploy_file).expect("Failed to open deploy.toml"));
    file.write_all(toml_string.as_bytes())
        .expect("Failed to write to deploy.toml");

    // Generate function registration file for local development
    generate_function_registration(&root, func_name, &method, &path, &signature);

    // Generate or update the main function file
    generate_main_function_file(&root);

    let _lambda_name = format!("lambda_{}", func_name);

    // Generate enhanced Lambda handler with parameter extraction
    let param_extraction = generate_parameter_extraction(&path_params);

    let func_ident = &input.sig.ident;
    let func_name_str = func_name.clone();
    let path_str = path.clone();
    let method_str = method.clone();

    // Generate the wrapper function call based on the analyzed signature
    let function_call = if signature.parameters.is_empty() {
        // Function takes no parameters
        quote! { #func_ident() }
    } else {
        // Function takes parameters - extract from environment variables
        let first_param = &signature.parameters[0];
        let param_name = &first_param.name;
        let env_var_name = format!("RUSTEZE_PARAM_{}", param_name.to_uppercase());
        quote! {
            {
                let param_value = std::env::var(#env_var_name).unwrap_or_else(|_| "".to_string());
                #func_ident(param_value)
            }
        }
    };

    // Generate the wrapper function call for the registry (with HashMap params)
    let registry_function_call = if signature.parameters.is_empty() {
        // Function takes no parameters
        quote! { #func_ident() }
    } else {
        // Function takes parameters - for now, assume first parameter
        let first_param = &signature.parameters[0];
        let param_name = &first_param.name;
        quote! {
            {
                let param_value = params.get(#param_name)
                    .or_else(|| params.values().next())
                    .unwrap_or(&"".to_string())
                    .clone();
                #func_ident(param_value)
            }
        }
    };

    let register_fn_name = syn::Ident::new(
        &format!("{}_register", func_name),
        proc_macro2::Span::call_site(),
    );
    let handler_fn_name = syn::Ident::new(
        &format!("{}_main_handler", func_name),
        proc_macro2::Span::call_site(),
    );

    let expanded = quote! {
        // Keep the original function unchanged
        #input

        // Generate a wrapper function for local development server integration
        #[cfg(not(feature = "lambda"))]
        pub fn #register_fn_name() -> (&'static str, &'static str, &'static str, fn(std::collections::HashMap<String, String>) -> Result<String, String>) {
            fn local_wrapper(params: std::collections::HashMap<String, String>) -> Result<String, String> {
                let result = #registry_function_call;
                Ok(result)
            }

            (#func_name_str, #method_str, #path_str, local_wrapper)
        }

        // Lambda deployment code (simplified for now)
        #[cfg(feature = "lambda")]
        #[tokio::main]
        async fn main() -> Result<(), Box<dyn std::error::Error>> {
            println!("Lambda function {} deployed", #func_name_str);
            Ok(())
        }

        // The main function is generated in rusteze_main.rs and should be included manually
        // or through a different mechanism to avoid multiple inclusions
    };

    TokenStream::from(expanded)
}

/// Generate function registration file for local development
fn generate_function_registration(
    root: &PathBuf,
    func_name: &str,
    method: &str,
    path: &str,
    signature: &FunctionSignature,
) {
    let registration_file = root.join("rusteze.functions.json");

    // Read existing registrations or create new structure
    let mut registrations: serde_json::Value = if registration_file.exists() {
        let content = std::fs::read_to_string(&registration_file).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Add or update this function's registration
    registrations[func_name] = serde_json::json!({
        "name": func_name,
        "method": method,
        "path": path,
        "signature": {
            "is_async": signature.is_async,
            "return_type": signature.return_type,
            "parameters": signature.parameters
        }
    });

    // Write the updated registration file
    let json_string = serde_json::to_string_pretty(&registrations)
        .expect("Failed to serialize function registrations");

    let mut file = BufWriter::new(
        File::create(&registration_file).expect("Failed to create registration file"),
    );
    file.write_all(json_string.as_bytes())
        .expect("Failed to write registration file");

    // Also generate a Rust file with callable functions for local development
    generate_callable_functions_file(root, &registrations);
}

/// Generate a Rust file with callable functions for local development
fn generate_callable_functions_file(root: &PathBuf, registrations: &serde_json::Value) {
    let callable_file = root.join("rusteze_local_functions.rs");

    let mut content = String::from(
        "// Auto-generated callable functions for local development\n\
         // This file is generated by the #[route] macro and should not be edited manually\n\n\
         use std::collections::HashMap;\n\n",
    );

    // Generate a function for each registered route
    if let Some(functions) = registrations.as_object() {
        for (func_name, func_data) in functions {
            if let Some(func_obj) = func_data.as_object() {
                let method = func_obj
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("GET");
                let path = func_obj.get("path").and_then(|v| v.as_str()).unwrap_or("/");

                content.push_str(&format!(
                    r#"/// Call the {} function with the given parameters
pub fn call_{}(params: HashMap<String, String>) -> Result<String, String> {{
    // Extract parameters based on the function signature
    let word = params.get("word").unwrap_or(&"World".to_string()).clone();
    
    // Call the actual annotated function directly
    // This assumes the function is available in the current crate's main module
    let result = crate::{}(word);
    Ok(result)
}}

/// Get registration info for {}
pub fn get_{}_info() -> (&'static str, &'static str, &'static str) {{
    ("{}", "{}", "{}")
}}

"#,
                    func_name, func_name, func_name, func_name, func_name, func_name, method, path
                ));
            }
        }
    }

    // Add a registry function that returns all callable functions
    content.push_str(
        r#"/// Get all registered functions for local development
pub fn get_all_functions() -> Vec<(&'static str, fn(HashMap<String, String>) -> Result<String, String>)> {
    vec![
"#
    );

    if let Some(functions) = registrations.as_object() {
        for func_name in functions.keys() {
            content.push_str(&format!(
                "        (\"{}\", call_{}),\n",
                func_name, func_name
            ));
        }
    }

    content.push_str("    ]\n}\n");

    // Write the callable functions file
    let mut file = BufWriter::new(
        File::create(&callable_file).expect("Failed to create callable functions file"),
    );
    file.write_all(content.as_bytes())
        .expect("Failed to write callable functions file");
}

/// Generate main function file for local development
fn generate_main_function_file(root: &PathBuf) {
    let main_file = root.join("src").join("rusteze_main.rs");

    // Read the function registrations to know what functions to handle
    let registration_file = root.join("rusteze.functions.json");
    if !registration_file.exists() {
        return; // No functions registered yet
    }

    let content = std::fs::read_to_string(&registration_file).unwrap_or_default();
    let registrations: serde_json::Value =
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));

    let mut main_content = String::from(
        r#"// Auto-generated main function for local development
// This file is generated by the #[route] macro and should not be edited manually

use std::env;

fn main() {
    // Check if this is a function call from the local development server
    if let Ok(mode) = env::var("RUSTEZE_MODE") {
        if mode == "function_call" {
            if let Ok(function_name) = env::var("RUSTEZE_FUNCTION") {
                match function_name.as_str() {
"#,
    );

    // Generate match arms for each registered function
    if let Some(functions) = registrations.as_object() {
        for (func_name, func_data) in functions {
            if let Some(func_obj) = func_data.as_object() {
                let signature = func_obj.get("signature");
                let has_params = signature
                    .and_then(|s| s.get("parameters"))
                    .and_then(|p| p.as_array())
                    .map(|arr| !arr.is_empty())
                    .unwrap_or(false);

                if has_params {
                    main_content.push_str(&format!(
                        r#"                    "{}" => {{
                        let word = env::var("RUSTEZE_PARAM_WORD").unwrap_or_else(|_| "World".to_string());
                        let result = {}(word);
                        println!("{{}}", result);
                        return;
                    }}
"#,
                        func_name, func_name
                    ));
                } else {
                    main_content.push_str(&format!(
                        r#"                    "{}" => {{
                        let result = {}();
                        println!("{{}}", result);
                        return;
                    }}
"#,
                        func_name, func_name
                    ));
                }
            }
        }
    }

    main_content.push_str(
        r#"                    _ => {
                        eprintln!("Unknown function: {}", function_name);
                        std::process::exit(1);
                    }
                }
            }
        }
    }
    
    // Default behavior - just print that functions are registered
    println!("Rusteze functions registered for local development");
}
"#,
    );

    // Write the main function file
    let mut file =
        BufWriter::new(File::create(&main_file).expect("Failed to create main function file"));
    file.write_all(main_content.as_bytes())
        .expect("Failed to write main function file");
}

/// Generate parameter extraction code for Lambda handler
fn generate_parameter_extraction(path_params: &[String]) -> proc_macro2::TokenStream {
    if path_params.is_empty() {
        return quote! {
            // No path parameters to extract
        };
    }

    let extractions: Vec<proc_macro2::TokenStream> = path_params
        .iter()
        .map(|param| {
            let param_ident = syn::Ident::new(param, proc_macro2::Span::call_site());
            quote! {
                let #param_ident = path_parameters.get(#param)
                    .cloned()
                    .unwrap_or_default();
            }
        })
        .collect();

    quote! {
        #(#extractions)*
    }
}

#[proc_macro_attribute]
pub fn publisher(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let func_name = &input.sig.ident.to_string();

    // Parse publisher attributes
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

    // Analyze function signature for enhanced metadata
    let signature = analyze_function_signature(&input.sig);

    // Create deployment metadata for publisher
    let mut environment_variables = HashMap::new();
    environment_variables.insert(
        format!("{}_TOPIC_ARN", topic.to_uppercase().replace('-', "_")),
        format!("${{{}Topic}}", topic.replace('-', "")),
    );

    let mut permissions = Vec::new();
    permissions.push("sns:Publish".to_string());
    permissions.push(format!("sns:Publish:arn:aws:sns:*:*:{}", topic));

    let deployment_metadata = DeploymentMetadata {
        memory_mb: Some(256),      // Default memory for publisher functions
        timeout_seconds: Some(30), // Default timeout for publisher functions
        environment_variables,
        permissions,
    };

    // Load or create deployment configuration
    let root = get_project_root();
    let deploy_file = root.join("rusteze.deploy.toml");

    let mut config: DeploymentConfig = if let Ok(content) = std::fs::read_to_string(&deploy_file) {
        toml::from_str(&content).unwrap_or_else(|_| create_default_deployment_config())
    } else {
        create_default_deployment_config()
    };

    // Create enhanced function configuration for publisher
    let function_config = FunctionConfig {
        name: func_name.to_string(),
        path: "/publish".to_string(), // Publishers don't have HTTP paths
        method: "POST".to_string(),   // Default method for publishers
        file: Some(format!("src/{}.rs", func_name)),
        signature: signature.clone(),
        handler_type: HandlerType::Publisher {
            topic: topic.clone(),
        },
        deployment_metadata,
    };

    config.functions.insert(func_name.clone(), function_config);

    // Write updated configuration
    let toml_string = match toml::to_string_pretty(&config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to serialize TOML: {}", e);
            // Fallback to basic structure for debugging
            format!(
                r#"[service]
name = "{}"

[functions.{}]
name = "{}"
path = "/publish"
method = "POST"
handler_type = {{ type = "Publisher", topic = "{}" }}

[functions.{}.signature]
is_async = {}
return_type = "{}"

[functions.{}.deployment_metadata]
memory_mb = 256
timeout_seconds = 30
"#,
                config.service.name,
                func_name,
                func_name,
                topic,
                func_name,
                signature.is_async,
                signature.return_type,
                func_name
            )
        }
    };

    let mut file = BufWriter::new(File::create(&deploy_file).expect("Failed to open deploy.toml"));
    file.write_all(toml_string.as_bytes())
        .expect("Failed to write to deploy.toml");

    // Generate enhanced Lambda handler for publisher with SNS integration
    let topic_env_var = format!("{}_TOPIC_ARN", topic.to_uppercase().replace('-', "_"));

    let expanded = quote! {
        use rusteze::tokio;

        #input

        #[tokio::main]
        async fn main() -> Result<(), rusteze::lambda_runtime::Error> {
            use rusteze::lambda_runtime::{service_fn, LambdaEvent};
            use rusteze::serde_json;
            use std::env;

            async fn handler(event: LambdaEvent<serde_json::Value>) -> Result<serde_json::Value, rusteze::lambda_runtime::Error> {
                // Get topic ARN from environment variable
                let topic_arn = env::var(#topic_env_var)
                    .map_err(|_| format!("Environment variable {} not found", #topic_env_var))?;

                // Parse the event payload
                let payload = event.payload;

                // Call the original publisher function
                match #func_name(payload).await {
                    Ok(result) => {
                        // TODO: Implement actual SNS publishing here
                        // This would use AWS SDK to publish to the topic
                        println!("Publishing to topic {}: {:?}", topic_arn, result);
                        Ok(serde_json::json!({
                            "status": "published",
                            "topic": topic_arn,
                            "message": result
                        }))
                    }
                    Err(e) => {
                        Err(format!("Publisher function failed: {}", e).into())
                    }
                }
            }

            let func = service_fn(handler);
            rusteze::lambda_runtime::run(func).await?;
            Ok(())
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_attribute]
pub fn subscriber(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    let func_name = &input.sig.ident.to_string();

    // Parse subscriber attributes
    let mut topic = "default-topic".to_string();
    let mut queue = format!("{}-queue", func_name);

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
                "queue" => queue = value.to_string(),
                _ => {} // Ignore unknown attributes
            }
        }
    }

    // Analyze function signature for enhanced metadata
    let signature = analyze_function_signature(&input.sig);

    // Create deployment metadata for subscriber
    let mut environment_variables = HashMap::new();
    environment_variables.insert(
        format!("{}_QUEUE_URL", queue.to_uppercase().replace('-', "_")),
        format!("${{{}Queue}}", queue.replace('-', "")),
    );
    environment_variables.insert(
        format!("{}_TOPIC_ARN", topic.to_uppercase().replace('-', "_")),
        format!("${{{}Topic}}", topic.replace('-', "")),
    );

    let mut permissions = Vec::new();
    permissions.push("sqs:ReceiveMessage".to_string());
    permissions.push("sqs:DeleteMessage".to_string());
    permissions.push("sqs:GetQueueAttributes".to_string());
    permissions.push(format!("sqs:ReceiveMessage:arn:aws:sqs:*:*:{}", queue));
    permissions.push(format!("sqs:DeleteMessage:arn:aws:sqs:*:*:{}", queue));

    let deployment_metadata = DeploymentMetadata {
        memory_mb: Some(256),      // Default memory for subscriber functions
        timeout_seconds: Some(60), // Longer timeout for message processing
        environment_variables,
        permissions,
    };

    // Load or create deployment configuration
    let root = get_project_root();
    let deploy_file = root.join("rusteze.deploy.toml");

    let mut config: DeploymentConfig = if let Ok(content) = std::fs::read_to_string(&deploy_file) {
        toml::from_str(&content).unwrap_or_else(|_| create_default_deployment_config())
    } else {
        create_default_deployment_config()
    };

    // Create enhanced function configuration for subscriber
    let function_config = FunctionConfig {
        name: func_name.to_string(),
        path: "/subscribe".to_string(), // Subscribers don't have HTTP paths
        method: "POST".to_string(),     // Default method for subscribers
        file: Some(format!("src/{}.rs", func_name)),
        signature: signature.clone(),
        handler_type: HandlerType::Subscriber {
            topic: topic.clone(),
            queue: queue.clone(),
        },
        deployment_metadata,
    };

    config.functions.insert(func_name.clone(), function_config);

    // Write updated configuration
    let toml_string = match toml::to_string_pretty(&config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to serialize TOML: {}", e);
            // Fallback to basic structure for debugging
            format!(
                r#"[service]
name = "{}"

[functions.{}]
name = "{}"
path = "/subscribe"
method = "POST"
handler_type = {{ type = "Subscriber", topic = "{}", queue = "{}" }}

[functions.{}.signature]
is_async = {}
return_type = "{}"

[functions.{}.deployment_metadata]
memory_mb = 256
timeout_seconds = 60
"#,
                config.service.name,
                func_name,
                func_name,
                topic,
                queue,
                func_name,
                signature.is_async,
                signature.return_type,
                func_name
            )
        }
    };

    let mut file = BufWriter::new(File::create(&deploy_file).expect("Failed to open deploy.toml"));
    file.write_all(toml_string.as_bytes())
        .expect("Failed to write to deploy.toml");

    // Generate enhanced Lambda handler for subscriber with SQS integration
    let queue_env_var = format!("{}_QUEUE_URL", queue.to_uppercase().replace('-', "_"));

    let expanded = quote! {
        use rusteze::tokio;

        #input

        #[tokio::main]
        async fn main() -> Result<(), rusteze::lambda_runtime::Error> {
            use rusteze::lambda_runtime::{service_fn, LambdaEvent};
            use rusteze::aws_lambda_events::event::sqs::{SqsEvent, SqsMessage};
            use rusteze::serde_json;
            use std::env;

            async fn handler(event: LambdaEvent<SqsEvent>) -> Result<(), rusteze::lambda_runtime::Error> {
                // Get queue URL from environment variable
                let _queue_url = env::var(#queue_env_var)
                    .map_err(|_| format!("Environment variable {} not found", #queue_env_var))?;

                let sqs_event = event.payload;

                // Process each message in the batch
                for record in sqs_event.records {
                    match process_message(record).await {
                        Ok(_) => {
                            println!("Successfully processed message");
                        }
                        Err(e) => {
                            eprintln!("Failed to process message: {}", e);
                            // In a real implementation, you might want to:
                            // 1. Send to dead letter queue
                            // 2. Retry with exponential backoff
                            // 3. Log the error for monitoring
                            return Err(e.into());
                        }
                    }
                }

                Ok(())
            }

            async fn process_message(message: SqsMessage) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
                // Parse the message body
                let body = message.body.unwrap_or_default();

                // Try to parse as JSON first, fallback to raw string
                let parsed_message = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(json_value) => json_value,
                    Err(_) => serde_json::Value::String(body),
                };

                // Call the original subscriber function
                #func_name(parsed_message).await.map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Subscriber function failed: {}", e)
                    )) as Box<dyn std::error::Error + Send + Sync>
                })?;

                Ok(())
            }

            let func = service_fn(handler);
            rusteze::lambda_runtime::run(func).await?;
            Ok(())
        }
    };

    TokenStream::from(expanded)
}
