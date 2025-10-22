use crate::RustezeConfig;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_apigatewayv2::Client as ApiGatewayClient;
use aws_sdk_lambda::Client as LambdaClient;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{fs, path::Path, process::Command};

#[derive(Serialize, Deserialize, Debug)]
struct Manifest {
    project: ProjectInfo,
    deployment: DeploymentInfo,
    route: HashMap<String, RouteInfo>,
    topic: HashMap<String, TopicInfo>,
    auth: HashMap<String, AuthInfo>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ProjectInfo {
    name: String,
    version: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct DeploymentInfo {
    runtime: String,
    memory: String,
    timeout: String,
    environment: String,
    arn: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct RouteInfo {
    method: String,
    path: String,
    binary: String,
    description: Option<String>,
    parameters: Option<HashMap<String, ParameterInfo>>,
    deployment: DeploymentInfo,
    auth: Option<AuthRequirement>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ParameterInfo {
    #[serde(rename = "type")]
    param_type: String,
    source: String,
    required: bool,
    description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct AuthRequirement {
    handler: String,
    required: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct TopicInfo {
    name: String,
    subscribers: HashMap<String, SubscriberInfo>,
    description: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct SubscriberInfo {
    binary: String,
    description: String,
    deployment: DeploymentInfo,
}

#[derive(Serialize, Deserialize, Debug)]
struct AuthInfo {
    binary: String,
    description: String,
    deployment: DeploymentInfo,
}

fn build_lambda_functions() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("Building Lambda functions...");

    // Change to .rusteze directory
    std::env::set_current_dir(".rusteze")?;

    let status = Command::new("cargo")
        .args(&["build", "--release"])
        .status()?;

    // Change back to original directory
    std::env::set_current_dir("..")?;

    if !status.success() {
        return Err("Failed to build Lambda functions".into());
    }

    println!("Lambda functions built successfully!");
    Ok(())
}

fn create_lambda_zip(
    binary_name: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let binary_path = format!(".rusteze/target/release/{}", binary_name);

    if !Path::new(&binary_path).exists() {
        return Err(format!("Binary not found: {}", binary_path).into());
    }

    // Create a simple zip file with just the binary
    let binary_data = fs::read(&binary_path)?;

    // For now, we'll use a simple approach - just read the binary
    // In production, you'd want to create a proper ZIP file
    Ok(binary_data)
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
    let mut manifest: Manifest = serde_json::from_str(&manifest_content)?;

    // Build all Lambda functions
    build_lambda_functions()?;

    // Set up AWS clients
    let region_provider = RegionProviderChain::default_provider()
        .or_else(aws_config::Region::new(config.deployment.region.clone()));
    let shared_config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(region_provider)
        .load()
        .await;

    let lambda_client = LambdaClient::new(&shared_config);
    let api_client = ApiGatewayClient::new(&shared_config);

    // Deploy Lambda functions for routes
    let mut deployed_functions = HashMap::new();

    for (route_name, route_info) in &manifest.route {
        println!(
            "Deploying Lambda function for route: {} {} {}",
            route_info.method, route_info.path, route_name
        );

        let function_name = format!("{}-{}", config.service_name, route_info.binary);
        let zip_data = create_lambda_zip(&route_info.binary)?;

        // Try to create the function, or update if it exists
        let result = lambda_client
            .create_function()
            .function_name(&function_name)
            .runtime(aws_sdk_lambda::types::Runtime::Providedal2)
            .handler("bootstrap") // For custom runtime
            .code(
                aws_sdk_lambda::types::FunctionCode::builder()
                    .zip_file(zip_data.into())
                    .build(),
            )
            .role(&format!(
                "arn:aws:iam::{}:role/lambda-execution-role",
                get_account_id().await?
            ))
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

        let function_arn = match result {
            Ok(response) => response.function_arn().unwrap().to_string(),
            Err(e) => {
                // If function exists, try to update it
                if e.to_string().contains("ResourceConflictException") {
                    println!("Function exists, updating: {}", function_name);
                    let update_result = lambda_client
                        .update_function_code()
                        .function_name(&function_name)
                        .zip_file(create_lambda_zip(&route_info.binary)?.into())
                        .send()
                        .await?;
                    update_result.function_arn().unwrap().to_string()
                } else {
                    return Err(format!(
                        "Failed to create/update function {}: {}",
                        function_name, e
                    )
                    .into());
                }
            }
        };

        deployed_functions.insert(route_name.clone(), (function_name.clone(), function_arn));
        println!("✓ Deployed Lambda function: {}", function_name);
    }

    // Deploy topic subscriber functions
    let mut deployed_subscriber_functions = HashMap::new();

    for (topic_name, topic_info) in &manifest.topic {
        for (subscriber_name, subscriber_info) in &topic_info.subscribers {
            println!(
                "Deploying subscriber function: {} for topic: {}",
                subscriber_name, topic_name
            );

            let function_name = format!("{}-{}", config.service_name, subscriber_info.binary);
            let zip_data = create_lambda_zip(&subscriber_info.binary)?;

            let result = lambda_client
                .create_function()
                .function_name(&function_name)
                .runtime(aws_sdk_lambda::types::Runtime::Providedal2)
                .handler("bootstrap")
                .code(
                    aws_sdk_lambda::types::FunctionCode::builder()
                        .zip_file(zip_data.into())
                        .build(),
                )
                .role(&format!(
                    "arn:aws:iam::{}:role/lambda-execution-role",
                    get_account_id().await?
                ))
                .memory_size(128)
                .timeout(30)
                .send()
                .await;

            let function_arn = match result {
                Ok(response) => {
                    println!("✓ Deployed subscriber function: {}", function_name);
                    response.function_arn().unwrap().to_string()
                }
                Err(e) if e.to_string().contains("ResourceConflictException") => {
                    println!("Subscriber function exists, updating: {}", function_name);
                    let update_result = lambda_client
                        .update_function_code()
                        .function_name(&function_name)
                        .zip_file(create_lambda_zip(&subscriber_info.binary)?.into())
                        .send()
                        .await?;
                    println!("✓ Updated subscriber function: {}", function_name);
                    update_result.function_arn().unwrap().to_string()
                }
                Err(e) => {
                    return Err(format!(
                        "Failed to deploy subscriber function {}: {}",
                        function_name, e
                    )
                    .into());
                }
            };

            deployed_subscriber_functions.insert(
                format!("{}:{}", topic_name, subscriber_name),
                (function_name, function_arn),
            );
        }
    }

    // Deploy auth functions
    let mut deployed_auth_functions = HashMap::new();

    for (auth_name, auth_info) in &manifest.auth {
        println!("Deploying auth function: {}", auth_name);

        let function_name = format!("{}-{}", config.service_name, auth_info.binary);
        let zip_data = create_lambda_zip(&auth_info.binary)?;

        let result = lambda_client
            .create_function()
            .function_name(&function_name)
            .runtime(aws_sdk_lambda::types::Runtime::Providedal2)
            .handler("bootstrap")
            .code(
                aws_sdk_lambda::types::FunctionCode::builder()
                    .zip_file(zip_data.into())
                    .build(),
            )
            .role(&format!(
                "arn:aws:iam::{}:role/lambda-execution-role",
                get_account_id().await?
            ))
            .memory_size(128)
            .timeout(10)
            .send()
            .await;

        let function_arn = match result {
            Ok(response) => {
                println!("✓ Deployed auth function: {}", function_name);
                response.function_arn().unwrap().to_string()
            }
            Err(e) if e.to_string().contains("ResourceConflictException") => {
                println!("Auth function exists, updating: {}", function_name);
                let update_result = lambda_client
                    .update_function_code()
                    .function_name(&function_name)
                    .zip_file(create_lambda_zip(&auth_info.binary)?.into())
                    .send()
                    .await?;
                println!("✓ Updated auth function: {}", function_name);
                update_result.function_arn().unwrap().to_string()
            }
            Err(e) => {
                return Err(
                    format!("Failed to deploy auth function {}: {}", function_name, e).into(),
                );
            }
        };

        deployed_auth_functions.insert(auth_name.clone(), (function_name, function_arn));
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
fn update_manifest_with_arns(
    manifest: &mut Manifest,
    deployed_functions: &HashMap<String, (String, String)>,
    deployed_auth_functions: &HashMap<String, (String, String)>,
    deployed_subscriber_functions: &HashMap<String, (String, String)>,
    api_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Update route ARNs
    for (route_name, route_info) in &mut manifest.route {
        if let Some((_, function_arn)) = deployed_functions.get(route_name) {
            route_info.deployment.arn = Some(function_arn.clone());
        }
    }

    // Update auth function ARNs
    for (auth_name, auth_info) in &mut manifest.auth {
        if let Some((_, function_arn)) = deployed_auth_functions.get(auth_name) {
            auth_info.deployment.arn = Some(function_arn.clone());
        }
    }

    // Update subscriber function ARNs
    for (topic_name, topic_info) in &mut manifest.topic {
        for (subscriber_name, subscriber_info) in &mut topic_info.subscribers {
            let key = format!("{}:{}", topic_name, subscriber_name);
            if let Some((_, function_arn)) = deployed_subscriber_functions.get(&key) {
                subscriber_info.deployment.arn = Some(function_arn.clone());
            }
        }
    }

    // Update project deployment info
    manifest.deployment.arn = Some(format!("api-gateway:{}", api_id));

    // Write updated manifest back to file
    let updated_manifest = serde_json::to_string_pretty(manifest)?;
    fs::write(".rusteze/manifest.json", updated_manifest)?;

    println!("✓ Updated manifest.json with deployment ARNs");
    Ok(())
}
