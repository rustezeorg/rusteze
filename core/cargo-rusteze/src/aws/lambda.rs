use crate::RustezeConfig;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_apigatewayv2::Client as ApiGatewayClient;
use aws_sdk_iam::Client as IamClient;
use aws_sdk_lambda::Client as LambdaClient;
use aws_sdk_sts::error::ProvideErrorMetadata;
use rusteze_config::RoutesConfig as Manifest;

use std::collections::HashMap;
use std::{fs, path::Path, process::Command};

fn build_lambda_functions() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Building Lambda functions with cargo lambda...");

    // Check if cargo lambda is installed
    let check_status = Command::new("cargo")
        .args(&["lambda", "--version"])
        .output();

    match check_status {
        Ok(output) if output.status.success() => {
            println!(
                "✓ cargo-lambda found: {}",
                String::from_utf8_lossy(&output.stdout).trim()
            );
        }
        _ => {
            return Err(
                "cargo-lambda not found. Please install it with: cargo install cargo-lambda".into(),
            );
        }
    }

    // Change to .rusteze directory
    std::env::set_current_dir(".rusteze")?;

    // Build with cargo lambda for proper Lambda packaging
    let status = Command::new("cargo")
        .args(&[
            "lambda",
            "build",
            "--release",
            "--arm64",
            "--output-format",
            "zip",
        ])
        .status()?;

    // Change back to original directory
    std::env::set_current_dir("..")?;

    if !status.success() {
        return Err("Failed to build Lambda functions with cargo lambda".into());
    }

    println!("✓ Lambda functions built successfully with cargo lambda!");
    Ok(())
}

fn get_lambda_zip(binary_name: &str) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    // cargo lambda creates zip files in target/lambda/{binary_name}/
    let zip_path = format!(".rusteze/target/lambda/{}/bootstrap.zip", binary_name);

    // Alternative path that cargo lambda might use
    let alt_zip_path = format!(".rusteze/target/lambda/{}.zip", binary_name);

    let final_path = if Path::new(&zip_path).exists() {
        zip_path
    } else if Path::new(&alt_zip_path).exists() {
        alt_zip_path
    } else {
        // List available files for debugging
        let lambda_dir = Path::new(".rusteze/target/lambda");
        if lambda_dir.exists() {
            println!("Available files in .rusteze/target/lambda/:");
            if let Ok(entries) = fs::read_dir(lambda_dir) {
                for entry in entries.flatten() {
                    println!("  {}", entry.path().display());
                }
            }
        }

        return Err(format!(
            "Lambda zip file not found. Tried:\n  - {}\n  - {}\nMake sure 'cargo lambda build --release --output-format zip' completed successfully.", 
            zip_path, alt_zip_path
        ).into());
    };

    // Read the zip file created by cargo lambda
    let zip_data = fs::read(&final_path)?;
    println!(
        "✓ Loaded Lambda zip for {}: {} bytes",
        binary_name,
        zip_data.len()
    );

    Ok(zip_data)
}

pub async fn deploy_to_aws(
    config: &RustezeConfig,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!(
        "Starting AWS deployment for service: {}",
        config.service_name
    );

    // Read the manifest file
    let manifest_content = fs::read_to_string(".rusteze/manifest.json")?;
    println!("Loading manifest: {}", &manifest_content);
    let mut manifest: Manifest = serde_json::from_str(&manifest_content)?;
    println!("manifest loaded! Building functions");

    // Build all Lambda functions
    build_lambda_functions()?;

    // Set up AWS clients
    println!(
        "Setting up AWS clients for region: {}",
        config.deployment.region
    );
    let region_provider = RegionProviderChain::default_provider()
        .or_else(aws_config::Region::new(config.deployment.region.clone()));
    let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(region_provider)
        .load()
        .await;

    println!("AWS region: {:?}", shared_config.region());

    let lambda_client = LambdaClient::new(&shared_config);
    let api_client = ApiGatewayClient::new(&shared_config);
    let iam_client = IamClient::new(&shared_config);

    // Get account ID first to verify credentials work
    println!("Verifying AWS credentials...");
    let account_id = match get_account_id().await {
        Ok(id) => {
            println!("✓ AWS Account ID: {}", id);
            id
        }
        Err(e) => {
            return Err(format!(
                "Failed to get AWS account ID. Please check your AWS credentials: {}",
                e
            )
            .into());
        }
    };

    // Ensure Lambda execution role exists
    println!("Setting up IAM role for Lambda execution...");
    let role_arn = match ensure_lambda_execution_role(&iam_client, &account_id).await {
        Ok(arn) => arn,
        Err(e) => {
            eprintln!("Failed to set up Lambda execution role: {}", e);
            eprintln!("\nTroubleshooting tips:");
            eprintln!("1. Ensure your AWS credentials have IAM permissions:");
            eprintln!("   - iam:GetRole");
            eprintln!("   - iam:CreateRole");
            eprintln!("   - iam:AttachRolePolicy");
            eprintln!(
                "2. Check if you're using the correct AWS region: {}",
                config.deployment.region
            );
            eprintln!("3. Verify your AWS credentials are configured correctly:");
            eprintln!("   aws sts get-caller-identity");
            return Err(e);
        }
    };

    // Deploy Lambda functions for routes
    let mut deployed_functions = HashMap::new();

    for (route_name, route_info) in &manifest.route {
        println!(
            "Deploying Lambda function for route: {} {} {}",
            route_info.method, route_info.path, route_name
        );

        let function_name = format!("{}-{}", config.service_name, route_info.binary);
        let zip_data = get_lambda_zip(&route_info.binary)?;

        println!("Role being used: {}", &role_arn);

        // Try to create the function, or update if it exists
        let result = lambda_client
            .create_function()
            .function_name(&function_name)
            .runtime(aws_sdk_lambda::types::Runtime::Providedal2023)
            .handler("bootstrap") // For custom runtime
            .architectures(aws_sdk_lambda::types::Architecture::Arm64)
            .code(
                aws_sdk_lambda::types::FunctionCode::builder()
                    .zip_file(zip_data.into())
                    .build(),
            )
            .role(&role_arn)
            .memory_size(
                config
                    .deployment
                    .lambda
                    .map(|l| l.memory_size as i32)
                    .unwrap_or(256),
            )
            .timeout(30)
            .send()
            .await;

        println!("Finished deploying lambda: {:?}", &result);

        let function_arn = match result {
            Ok(response) => {
                println!("✓ Created new Lambda function: {}", function_name);
                response.function_arn().unwrap().to_string()
            }
            Err(e) => {
                // Check for specific AWS Lambda error types
                if let Some(service_error) = e.as_service_error() {
                    if service_error.is_resource_conflict_exception() {
                        println!("Function exists, updating: {}", function_name);
                        let update_result = lambda_client
                            .update_function_code()
                            .function_name(&function_name)
                            .zip_file(get_lambda_zip(&route_info.binary)?.into())
                            .send()
                            .await?;
                        println!("✓ Updated Lambda function: {}", function_name);
                        update_result.function_arn().unwrap().to_string()
                    } else {
                        return Err(format!(
                            "Failed to create function {}: {} (Code: {:?})",
                            function_name,
                            service_error,
                            service_error.code()
                        )
                        .into());
                    }
                } else {
                    // Fallback to string checking for other error types
                    let error_string = e.to_string();
                    if error_string.contains("ResourceConflictException")
                        || error_string.contains("Function already exist")
                        || error_string.contains("already exists")
                    {
                        println!(
                            "Function exists (detected from error message), updating: {}",
                            function_name
                        );

                        let update_result = lambda_client
                            .update_function_code()
                            .function_name(&function_name)
                            .zip_file(get_lambda_zip(&route_info.binary)?.into())
                            .send()
                            .await?;
                        println!("✓ Updated Lambda function: {}", function_name);
                        update_result.function_arn().unwrap().to_string()
                    } else {
                        return Err(format!(
                            "Failed to create/update function {}: {}",
                            function_name, error_string
                        )
                        .into());
                    }
                }
            }
        };

        deployed_functions.insert(route_name.clone(), (function_name.clone(), function_arn));
        println!("✓ Deployed Lambda function: {}", function_name);
    }

    // Deploy topic subscriber functions
    let mut deployed_subscriber_functions = HashMap::new();

    if let Some(ref topic_map) = manifest.topic {
        for (topic_name, topic_info) in topic_map {
            for (subscriber_name, subscriber_info) in &topic_info.subscribers {
                println!(
                    "Deploying subscriber function: {} for topic: {}",
                    subscriber_name, topic_name
                );

                let function_name = format!("{}-{}", config.service_name, subscriber_info.binary);
                let zip_data = get_lambda_zip(&subscriber_info.binary)?;

                // @todo - move this to a helper - ISS-123
                let result = lambda_client
                    .create_function()
                    .function_name(&function_name)
                    .runtime(aws_sdk_lambda::types::Runtime::Providedal2023)
                    .handler("bootstrap")
                    .architectures(aws_sdk_lambda::types::Architecture::Arm64)
                    .code(
                        aws_sdk_lambda::types::FunctionCode::builder()
                            .zip_file(zip_data.into())
                            .build(),
                    )
                    .role(&role_arn)
                    .memory_size(128)
                    .timeout(30)
                    .send()
                    .await;

                let function_arn = match result {
                    Ok(response) => {
                        println!("✓ Created new subscriber function: {}", function_name);
                        response.function_arn().unwrap().to_string()
                    }
                    Err(e) => {
                        // Check for specific AWS Lambda error types
                        if let Some(service_error) = e.as_service_error() {
                            if service_error.is_resource_conflict_exception() {
                                println!("Subscriber function exists, updating: {}", function_name);
                                let update_result = lambda_client
                                    .update_function_code()
                                    .function_name(&function_name)
                                    .zip_file(get_lambda_zip(&subscriber_info.binary)?.into())
                                    .send()
                                    .await?;
                                println!("✓ Updated subscriber function: {}", function_name);
                                update_result.function_arn().unwrap().to_string()
                            } else {
                                return Err(format!(
                                    "Failed to create subscriber function {}: {} (Code: {:?})",
                                    function_name,
                                    service_error,
                                    service_error.code()
                                )
                                .into());
                            }
                        } else {
                            // Fallback to string checking
                            let error_string = e.to_string();
                            if error_string.contains("ResourceConflictException")
                                || error_string.contains("Function already exist")
                                || error_string.contains("already exists")
                            {
                                println!(
                                    "Subscriber function exists (detected from error message), updating: {}",
                                    function_name
                                );

                                let update_result = lambda_client
                                    .update_function_code()
                                    .function_name(&function_name)
                                    .zip_file(get_lambda_zip(&subscriber_info.binary)?.into())
                                    .send()
                                    .await?;
                                println!("✓ Updated subscriber function: {}", function_name);
                                update_result.function_arn().unwrap().to_string()
                            } else {
                                return Err(format!(
                                    "Failed to deploy subscriber function {}: {}",
                                    function_name, error_string
                                )
                                .into());
                            }
                        }
                    }
                };

                deployed_subscriber_functions.insert(
                    format!("{}:{}", topic_name, subscriber_name),
                    (function_name, function_arn),
                );
            }
        }
    }

    // Deploy auth functions
    let mut deployed_auth_functions = HashMap::new();

    if let Some(ref auth_map) = manifest.auth {
        for (auth_name, auth_info) in auth_map {
            println!("Deploying auth function: {}", auth_name);

            let function_name = format!("{}-{}", config.service_name, auth_info.binary);
            let zip_data = get_lambda_zip(&auth_info.binary)?;

            //@todo - move this to a helper - ISS-123
            let result = lambda_client
                .create_function()
                .function_name(&function_name)
                .runtime(aws_sdk_lambda::types::Runtime::Providedal2023)
                .handler("bootstrap")
                .architectures(aws_sdk_lambda::types::Architecture::Arm64)
                .code(
                    aws_sdk_lambda::types::FunctionCode::builder()
                        .zip_file(zip_data.into())
                        .build(),
                )
                .role(&role_arn)
                .memory_size(128)
                .timeout(10)
                .send()
                .await;

            let function_arn = match result {
                Ok(response) => {
                    println!("✓ Created new auth function: {}", function_name);
                    response.function_arn().unwrap().to_string()
                }
                Err(e) => {
                    // Check for specific AWS Lambda error types
                    if let Some(service_error) = e.as_service_error() {
                        if service_error.is_resource_conflict_exception() {
                            println!("Auth function exists, updating: {}", function_name);

                            let update_result = lambda_client
                                .update_function_code()
                                .function_name(&function_name)
                                .zip_file(get_lambda_zip(&auth_info.binary)?.into())
                                .send()
                                .await?;
                            println!("✓ Updated auth function: {}", function_name);
                            update_result.function_arn().unwrap().to_string()
                        } else {
                            return Err(format!(
                                "Failed to create auth function {}: {} (Code: {:?})",
                                function_name,
                                service_error,
                                service_error.code()
                            )
                            .into());
                        }
                    } else {
                        // Fallback to string checking
                        let error_string = e.to_string();
                        if error_string.contains("ResourceConflictException")
                            || error_string.contains("Function already exist")
                            || error_string.contains("already exists")
                        {
                            println!(
                                "Auth function exists (detected from error message), updating: {}",
                                function_name
                            );

                            let update_result = lambda_client
                                .update_function_code()
                                .function_name(&function_name)
                                .zip_file(get_lambda_zip(&auth_info.binary)?.into())
                                .send()
                                .await?;
                            println!("✓ Updated auth function: {}", function_name);
                            update_result.function_arn().unwrap().to_string()
                        } else {
                            return Err(format!(
                                "Failed to deploy auth function {}: {}",
                                function_name, error_string
                            )
                            .into());
                        }
                    }
                }
            };

            deployed_auth_functions.insert(auth_name.clone(), (function_name, function_arn));
        }
    }

    // Set up API Gateway
    println!("Setting up API Gateway...");

    let api_name = &config.service_name;
    let apis = api_client.get_apis().send().await?;
    let existing_api = apis.items().iter().find(|a| a.name() == Some(api_name));

    let api_id = match existing_api {
        Some(api) => {
            println!("Using existing API Gateway: {}", api_name);
            api.api_id().unwrap().to_string()
        }
        None => {
            println!("Creating new API Gateway: {}", api_name);
            let api = api_client
                .create_api()
                .name(api_name)
                .protocol_type(aws_sdk_apigatewayv2::types::ProtocolType::Http)
                .send()
                .await?;
            api.api_id().unwrap().to_string()
        }
    };

    // Create routes and integrations
    for (route_name, route_info) in &manifest.route {
        if let Some((function_name, function_arn)) = deployed_functions.get(route_name) {
            println!(
                "Creating API route: {} {}",
                route_info.method, route_info.path
            );

            // Create integration
            let integration = api_client
                .create_integration()
                .api_id(&api_id)
                .integration_type(aws_sdk_apigatewayv2::types::IntegrationType::AwsProxy)
                .integration_uri(format!(
                    "arn:aws:apigateway:{}:lambda:path/2015-03-31/functions/{}/invocations",
                    config.deployment.region, function_arn
                ))
                .payload_format_version("2.0")
                .send()
                .await?;

            // Create route
            let route_key = format!("{} {}", route_info.method, route_info.path);
            api_client
                .create_route()
                .api_id(&api_id)
                .route_key(&route_key)
                .target(format!(
                    "integrations/{}",
                    integration.integration_id().unwrap()
                ))
                .send()
                .await?;

            // Add Lambda permission for API Gateway to invoke the function
            let statement_id = format!("apigateway-{}-{}", api_id, route_name);
            let _ = lambda_client
                .add_permission()
                .function_name(function_name)
                .statement_id(&statement_id)
                .action("lambda:InvokeFunction")
                .principal("apigateway.amazonaws.com")
                .source_arn(format!(
                    "arn:aws:execute-api:{}:{}:{}/*/*",
                    config.deployment.region,
                    get_account_id().await?,
                    api_id
                ))
                .send()
                .await; // Ignore errors as permission might already exist

            println!("✓ Created route: {}", route_key);
        }
    }

    // Create a deployment stage
    let stage_name = "prod";
    let _ = api_client
        .create_stage()
        .api_id(&api_id)
        .stage_name(stage_name)
        .auto_deploy(true)
        .send()
        .await; // Ignore errors as stage might already exist

    let api_url = format!(
        "https://{}.execute-api.{}.amazonaws.com/{}",
        api_id, config.deployment.region, stage_name
    );

    // Update manifest with deployed ARNs
    update_manifest_with_arns(
        &mut manifest,
        &deployed_functions,
        &deployed_auth_functions,
        &deployed_subscriber_functions,
        &api_id,
    )?;

    println!("\n🚀 Deployment complete!");
    println!("API Gateway ID: {}", api_id);
    println!("API URL: {}", api_url);
    println!("\nAvailable endpoints:");

    for (_, route_info) in &manifest.route {
        println!(
            "  {} {} -> {}{}",
            route_info.method, route_info.path, api_url, route_info.path
        );
    }

    Ok(())
}

async fn get_account_id() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    use aws_sdk_sts::Client as StsClient;

    let region_provider = RegionProviderChain::default_provider();
    let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(region_provider)
        .load()
        .await;

    let sts_client = StsClient::new(&shared_config);
    let identity = sts_client.get_caller_identity().send().await?;
    Ok(identity.account().unwrap().to_string())
}

async fn ensure_lambda_execution_role(
    iam_client: &IamClient,
    account_id: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let role_name = "lambda-execution-role";
    let role_arn = format!("arn:aws:iam::{}:role/{}", account_id, role_name);

    println!("Checking for Lambda execution role: {}", role_name);

    // Check if role exists
    match iam_client.get_role().role_name(role_name).send().await {
        Ok(role_response) => {
            println!("✓ Lambda execution role already exists: {}", role_name);

            // Verify the role has the correct trust policy
            if let Some(role) = role_response.role() {
                if let Some(trust_policy) = role.assume_role_policy_document() {
                    if !trust_policy.contains("lambda.amazonaws.com") {
                        println!(
                            "⚠️  Warning: Existing role may not have correct trust policy for Lambda"
                        );
                        println!(
                            "Trust policy should contain 'lambda.amazonaws.com' service principal"
                        );
                    } else {
                        println!("✓ Role has correct Lambda trust policy");
                    }
                }
            }

            // Check if the basic execution policy is attached
            let policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole";
            match iam_client
                .list_attached_role_policies()
                .role_name(role_name)
                .send()
                .await
            {
                Ok(policies) => {
                    let has_basic_policy = policies
                        .attached_policies()
                        .iter()
                        .any(|p| p.policy_arn() == Some(policy_arn));

                    if !has_basic_policy {
                        println!("⚠️  Attaching missing AWSLambdaBasicExecutionRole policy...");
                        iam_client
                            .attach_role_policy()
                            .role_name(role_name)
                            .policy_arn(policy_arn)
                            .send()
                            .await?;
                        println!("✓ Attached AWSLambdaBasicExecutionRole policy");
                    } else {
                        println!("✓ Role has required execution policy");
                    }
                }
                Err(e) => {
                    println!("⚠️  Could not check attached policies: {}", e);
                }
            }

            return Ok(role_arn);
        }
        Err(e) => {
            // Check for specific error types
            if let Some(service_error) = e.as_service_error() {
                println!("IAM service error: {:?}", service_error);
                if service_error.is_no_such_entity_exception() {
                    println!("Role doesn't exist, will create it");
                } else {
                    return Err(format!(
                        "Error checking for role '{}': {} (Code: {:?})",
                        role_name,
                        service_error,
                        service_error.code()
                    )
                    .into());
                }
            } else {
                return Err(format!(
                    "Error checking for role '{}': {} (Raw error: {:?})",
                    role_name,
                    e,
                    e.raw_response()
                )
                .into());
            }
        }
    }

    println!("Creating Lambda execution role: {}", role_name);

    // Trust policy document for Lambda - ensure proper JSON formatting
    let trust_policy = r#"{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "Service": "lambda.amazonaws.com"
      },
      "Action": "sts:AssumeRole"
    }
  ]
}"#;

    // Create the role
    match iam_client
        .create_role()
        .role_name(role_name)
        .assume_role_policy_document(trust_policy)
        .description("Execution role for Lambda functions created by Rusteze")
        .send()
        .await
    {
        Ok(_) => println!("✓ Successfully created role: {}", role_name),
        Err(e) => {
            if let Some(service_error) = e.as_service_error() {
                return Err(format!(
                    "Failed to create role '{}': {} (Code: {:?})",
                    role_name,
                    service_error,
                    service_error.code()
                )
                .into());
            } else {
                return Err(format!(
                    "Failed to create role '{}': {} (Raw error: {:?})",
                    role_name,
                    e,
                    e.raw_response()
                )
                .into());
            }
        }
    }

    // Attach the basic execution policy
    let policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole";
    match iam_client
        .attach_role_policy()
        .role_name(role_name)
        .policy_arn(policy_arn)
        .send()
        .await
    {
        Ok(_) => println!("✓ Successfully attached policy: {}", policy_arn),
        Err(e) => {
            if let Some(service_error) = e.as_service_error() {
                return Err(format!(
                    "Failed to attach policy to role '{}': {} (Code: {:?})",
                    role_name,
                    service_error,
                    service_error.code()
                )
                .into());
            } else {
                return Err(format!(
                    "Failed to attach policy to role '{}': {} (Raw error: {:?})",
                    role_name,
                    e,
                    e.raw_response()
                )
                .into());
            }
        }
    }

    println!("✓ Created Lambda execution role: {}", role_name);

    // Wait a moment for the role to propagate in AWS
    println!("Waiting for role to propagate...");
    tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;

    // Verify the role was created and can be retrieved
    match iam_client.get_role().role_name(role_name).send().await {
        Ok(_) => println!("✓ Role propagation verified"),
        Err(e) => {
            return Err(format!(
                "Role was created but cannot be retrieved (propagation issue): {}",
                e
            )
            .into());
        }
    }

    Ok(role_arn)
}
fn update_manifest_with_arns(
    manifest: &mut Manifest,
    deployed_functions: &HashMap<String, (String, String)>,
    deployed_auth_functions: &HashMap<String, (String, String)>,
    deployed_subscriber_functions: &HashMap<String, (String, String)>,
    api_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Update route ARNs
    for (route_name, route_info) in manifest.route.iter_mut() {
        if let Some((_, function_arn)) = deployed_functions.get(route_name) {
            if let Some(ref mut deployment) = route_info.deployment {
                deployment.arn = Some(function_arn.clone());
            }
        }
    }

    // Update auth function ARNs
    if let Some(ref mut auth_map) = manifest.auth {
        for (auth_name, auth_info) in auth_map.iter_mut() {
            if let Some((_, function_arn)) = deployed_auth_functions.get(auth_name) {
                if let Some(ref mut deployment) = auth_info.deployment {
                    deployment.arn = Some(function_arn.clone());
                }
            }
        }
    }

    // Update subscriber function ARNs
    if let Some(ref mut topic_map) = manifest.topic {
        for (topic_name, topic_info) in topic_map.iter_mut() {
            for (subscriber_name, subscriber_info) in topic_info.subscribers.iter_mut() {
                let key = format!("{}:{}", topic_name, subscriber_name);
                if let Some((_, function_arn)) = deployed_subscriber_functions.get(&key) {
                    if let Some(ref mut deployment) = subscriber_info.deployment {
                        deployment.arn = Some(function_arn.clone());
                    }
                }
            }
        }
    }

    // Update project deployment info
    if let Some(ref mut deployment) = manifest.deployment {
        deployment.arn = Some(format!("api-gateway:{}", api_id));
    }

    // Write updated manifest back to file
    let updated_manifest = serde_json::to_string_pretty(manifest)?;
    fs::write(".rusteze/manifest.json", updated_manifest)?;

    println!("✓ Updated manifest.json with deployment ARNs");
    Ok(())
}
