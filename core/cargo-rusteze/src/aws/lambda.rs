use std::{fs::File, path::Path, io::Write};

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_lambda::Client as LambdaClient;
use aws_sdk_apigatewayv2::Client as ApiGatewayClient;
use serde::{Deserialize, Serialize};
use std::process::Command;
use crate::RustezeConfig;
use tokio::runtime::Runtime;
use std::fs;


#[derive(Serialize, Deserialize)]
struct RouteConfig {
    functions: Vec<FunctionConfig>,
}

#[derive(Serialize, Deserialize)]
struct FunctionConfig {
    name: String,
    path: String,
    method: String,
}

const FUNCTION_NAME: &str = "my_rust_lambda";
const ZIP_FILE: &str = "lambda.zip";

fn build_lambda() {
    println!("Building Lambda function...");
    let status = Command::new("cargo")
        .args(&["lambda", "build", "--release"])
        .status()
        .expect("Failed to compile Lambda function");

    if status.success() {
        println!("Lambda built successfully!");
    }
}

fn zip_lambda() {
    println!("Zipping Lambda binary...");
    let binary_path = Path::new("target/x86_64-unknown-linux-musl/release/my_lambda");

    if binary_path.exists() {
        let mut zip_file = File::create(ZIP_FILE).expect("Failed to create ZIP file");
        let binary_data = std::fs::read(binary_path).expect("Failed to read binary file");
        zip_file.write_all(&binary_data).expect("Failed to write ZIP file");
        println!("Lambda ZIP file created.");
    } else {
        panic!("Lambda binary not found!");
    }
}

pub async fn deploy_lambda(config: &RustezeConfig) -> Result<(), Box<dyn std::error::Error>> {
    build_lambda();
    zip_lambda();

    println!("Deploying Lambda function...");

    let region_provider = RegionProviderChain::default_provider();
    let shared_config = aws_config::from_env().region(region_provider).load().await;
    let lambda_client = LambdaClient::new(&shared_config);
    let api_client = ApiGatewayClient::new(&shared_config);

    // @todo - Should move this to the lib to be shared between the codegen and the deploy command.
    let toml_str = fs::read_to_string("rusteze.deploy.toml")?;
    let route_config: RouteConfig = toml::from_str(&toml_str)?;

    for function in &route_config.functions {
        let binary_path = format!("target/release/{}", function.name);
        let zip_path = format!("{}.zip", binary_path);

        Command::new("cargo").arg("build").arg("--release").output()?;
        Command::new("zip").arg("-j").arg(&zip_path).arg(&binary_path).output()?;

        let file_zip = match fs::read(&zip_path) {
            Ok(f) => f,
            Err(err) => panic!("Unable to find zipped file: {:?}", err)
        };

        // Deploy Lambda Function
        let result = lambda_client.create_function()
            .function_name(&function.name)
            .runtime(aws_sdk_lambda::types::Runtime::Providedal2)
            .handler(&function.name)
            .code(aws_sdk_lambda::types::FunctionCode::builder().zip_file(file_zip.into()).build())
            .role("arn:aws:iam::your-account-id:role/lambda-execution-role")
            .send()
            .await?;

        // We should now write back to the deploy.toml with the arn to keep track of it.

        let arn = result.function_arn;
        println!("Deployed Lambda: {} arn: {:?}", function.name, &arn);
    }

    // Step 3: Set Up API Gateway
    let apis = api_client.get_apis().send().await?;
    let api_name = &config.service_name;
    let api = apis.items().iter()
        .find(|a| a.name() == Some(api_name));
 
    let api_id = match api {
         Some(a) => a.api_id().unwrap().to_string(),
         None => {
             let api = api_client.create_api()
                 .name(api_name.clone())
                 .protocol_type(aws_sdk_apigatewayv2::types::ProtocolType::Http)
                 .send()
                 .await?;
             api.api_id().unwrap().to_string()
         }
    };
    
    // Should use a hashmap after the lambda's have been updated.
    for function in &route_config.functions {
        let integration = api_client.create_integration()
            .api_id(&api_id)
            .integration_type(aws_sdk_apigatewayv2::types::IntegrationType::AwsProxy)
            .integration_uri(format!(
                "arn:aws:apigateway:{}:lambda:path/2015-03-31/functions/arn:aws:lambda:{}:function:{}:invoke",
                shared_config.region().unwrap().as_ref(),
                shared_config.region().unwrap().as_ref(),
                function.name
            ))
            .send()
            .await?;

        api_client.create_route()
            .api_id(&api_id)
            .route_key(format!("{} {}", function.method, function.path))
            .target(format!("integrations/{}", integration.integration_id().unwrap()))
            .send()
            .await?;

        println!("Created API route: {} {}", function.method, function.path);
    }
 
    println!("Deployment complete! API Gateway ID: {:?}", &api_id);
    Ok(())
}