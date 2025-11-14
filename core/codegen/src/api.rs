use quote::quote;
use rusteze_config::ParameterConfig;
use syn::ItemFn;

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

pub fn extract_function_params(func: &ItemFn, route_path: &str) -> Vec<(String, ParameterConfig)> {
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

pub fn extract_params(path: &str) -> Vec<String> {
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

pub fn generate_generic_proxy_code(func: &ItemFn, route_path: &str) -> (String, String) {
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
                            "    // Extract {} from URL path parameter\n    let {} = event.path_parameters().first(\"{}\").unwrap_or_default().to_string();\n",
                            param_name, param_name, param_name
                        ));
                    } else if type_str.contains("i32")
                        || type_str.contains("u32")
                        || type_str.contains("i64")
                        || type_str.contains("u64")
                    {
                        param_setup.push_str(&format!(
                            "    // Extract {} from URL path parameter\n    let {} = event.path_parameters().first(\"{}\").and_then(|s| s.parse().ok()).unwrap_or(0);\n",
                            param_name, param_name, param_name
                        ));
                    } else if type_str.contains("bool") {
                        param_setup.push_str(&format!(
                            "    // Extract {} from URL path parameter\n    let {} = event.path_parameters().first(\"{}\").map(|s| s == \"true\" || s == \"1\" || s == \"yes\").unwrap_or(false);\n",
                            param_name, param_name, param_name
                        ));
                    }
                } else {
                    // This is likely a structured input type - parse from JSON
                    param_setup.push_str(&format!(
                        "    // Parse structured input from request body\n    let body_str = match std::str::from_utf8(event.body()) {{\n        Ok(s) => s,\n        Err(_) => panic!(\"Invalid UTF-8 in request body\"),\n    }};\n    let {} = if !body_str.is_empty() {{\n        serde_json::from_str(body_str).unwrap_or_else(|e| {{\n            eprintln!(\"Failed to parse JSON input: {{}}\", e);\n            panic!(\"Invalid JSON body\");\n        }})\n    }} else {{\n        panic!(\"No request body provided\");\n    }};\n",
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

pub fn generate_local_mode_proxy_code(func: &ItemFn, route_path: &str) -> (String, String) {
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
                    // Handle path parameter from command line args
                    if type_str.contains("String") {
                        param_setup.push_str(&format!(
                            "        // Extract {} from command line args\n        let args: Vec<String> = std::env::args().collect();\n        let {} = if args.len() > 1 {{\n            args[1].clone()\n        }} else {{\n            String::new()\n        }};\n",
                            param_name, param_name
                        ));
                    } else if type_str.contains("i32")
                        || type_str.contains("u32")
                        || type_str.contains("i64")
                        || type_str.contains("u64")
                    {
                        param_setup.push_str(&format!(
                            "        // Extract {} from command line args\n        let args: Vec<String> = std::env::args().collect();\n        let {} = if args.len() > 1 {{\n            args[1].parse().unwrap_or(0)\n        }} else {{\n            0\n        }};\n",
                            param_name, param_name
                        ));
                    } else if type_str.contains("bool") {
                        param_setup.push_str(&format!(
                            "        // Extract {} from command line args\n        let args: Vec<String> = std::env::args().collect();\n        let {} = if args.len() > 1 {{\n            args[1] == \"true\" || args[1] == \"1\" || args[1] == \"yes\"\n        }} else {{\n            false\n        }};\n",
                            param_name, param_name
                        ));
                    }
                } else {
                    // This is likely a structured input type - parse from JSON command line arg
                    param_setup.push_str(&format!(
                        "        // Parse structured input from command line args\n        let args: Vec<String> = std::env::args().collect();\n        let {} = if args.len() > 1 {{\n            serde_json::from_str(&args[1]).unwrap_or_else(|e| {{\n                eprintln!(\"Failed to parse JSON input: {{}}\", e);\n                panic!(\"Invalid JSON argument\");\n            }})\n        }} else {{\n            panic!(\"No JSON argument provided\");\n        }};\n",
                        param_name
                    ));
                }

                function_args.push(param_name.to_string());
            }
        }
    } else {
        // Handle multiple parameters from command line args
        for (i, input) in non_self_inputs.iter().enumerate() {
            if let syn::FnArg::Typed(pat_type) = input {
                if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                    let param_name = &pat_ident.ident;
                    let param_type = &pat_type.ty;
                    let type_str = quote! { #param_type }.to_string();

                    if type_str.contains("String") {
                        param_setup.push_str(&format!(
                            "        // Extract {} from command line args\n        let args: Vec<String> = std::env::args().collect();\n        let {} = if args.len() > {} {{\n            args[{}].clone()\n        }} else {{\n            String::new()\n        }};\n",
                            param_name, param_name, i + 1, i + 1
                        ));
                    } else if type_str.contains("i32")
                        || type_str.contains("u32")
                        || type_str.contains("i64")
                        || type_str.contains("u64")
                    {
                        param_setup.push_str(&format!(
                            "        // Extract {} from command line args\n        let args: Vec<String> = std::env::args().collect();\n        let {} = if args.len() > {} {{\n            args[{}].parse().unwrap_or(0)\n        }} else {{\n            0\n        }};\n",
                            param_name, param_name, i + 1, i + 1
                        ));
                    } else if type_str.contains("bool") {
                        param_setup.push_str(&format!(
                            "        // Extract {} from command line args\n        let args: Vec<String> = std::env::args().collect();\n        let {} = if args.len() > {} {{\n            args[{}] == \"true\" || args[{}] == \"1\" || args[{}] == \"yes\"\n        }} else {{\n            false\n        }};\n",
                            param_name, param_name, i + 1, i + 1, i + 1, i + 1
                        ));
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
