use crate::RustezeConfig;
use aws_config::retry::ProvideErrorKind;
use aws_sdk_lambda::Client as LambdaClient;
use aws_sdk_lambda::types::LastUpdateStatus;
use aws_sdk_sts::error::ProvideErrorMetadata;
use core::fmt;
use std::fmt::Display;
use std::time::Duration;
use std::{fs, path::Path, process::Command};

pub fn build_lambda_functions() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
    // @todo - should configure which architecture to use.
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

#[derive(Debug)]
pub enum DeployFunctionError {
    DeploymentError(String),
}

impl Display for DeployFunctionError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DeployFunctionError::DeploymentError(message) => {
                write!(f, "Unable to create function: {}", message)
            }
        }
    }
}

impl std::error::Error for DeployFunctionError {}

async fn is_function_ready(
    lambda_client: &LambdaClient,
    function_name: String,
) -> Result<bool, DeployFunctionError> {
    // Get the function.
    println!("Checking {} function is ready!", &function_name);
    let func = match lambda_client
        .get_function()
        .function_name(function_name)
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            return Err(DeployFunctionError::DeploymentError(
                "Unable to check Lambda function status".to_string(),
            ));
        }
    };

    if let Some(config) = func.configuration() {
        match config.last_update_status() {
            Some(lambda_status) => match lambda_status {
                LastUpdateStatus::Successful => return Ok(true),
                LastUpdateStatus::InProgress => return Ok(false),
                _ => {
                    println!("LambdaUpdateStatus unknown");
                    return Err(DeployFunctionError::DeploymentError(
                        "Lambda update status is unknown".to_string(),
                    ));
                }
            },
            None => {
                return Err(DeployFunctionError::DeploymentError(
                    "Function not ready to update".to_string(),
                ));
            }
        }
    }

    Ok(false)
}

pub async fn deploy_function(
    config: &RustezeConfig,
    lambda_client: &LambdaClient,
    function_name: &str,
    role_arn: &str,
    binary: &str,
) -> Result<String, DeployFunctionError> {
    // List all current functions to see if we need to update rather than remove.
    let all_functions = match lambda_client.list_functions().send().await {
        Ok(f) => f,
        Err(e) => return Err(DeployFunctionError::DeploymentError(e.to_string())),
    };

    // Would need to determine whats been deployed for the routes so that we can then delete the other ones.
    let existing_function = all_functions
        .functions()
        .iter()
        .find(|f| f.function_name() == Some(&function_name));

    let zip_data = match get_lambda_zip(&binary) {
        Ok(r) => r,
        Err(e) => return Err(DeployFunctionError::DeploymentError(e.to_string())),
    };

    // @todo - Update the architecture to pull from config.
    let arn = match existing_function {
        Some(f) => {
            println!("Function exists, updating: {}", function_name);

            let update_result = lambda_client
                .update_function_code()
                .function_name(&*function_name)
                .zip_file(zip_data.into())
                .architectures(aws_sdk_lambda::types::Architecture::Arm64)
                .send()
                .await;

            let updated_function_arn = match update_result {
                Ok(res) => {
                    println!("✓ Updated Lambda function: {}", function_name);

                    while !is_function_ready(&lambda_client, function_name.to_string())
                        .await
                        .unwrap()
                    {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }

                    match lambda_client
                        .update_function_configuration()
                        .function_name(&*function_name)
                        .runtime(aws_sdk_lambda::types::Runtime::Providedal2023)
                        .memory_size(
                            config
                                .deployment
                                .lambda
                                .map(|l| l.memory_size as i32)
                                .unwrap_or(256),
                        )
                        .send()
                        .await
                    {
                        Ok(r) => {}
                        Err(e) => {
                            let service_error = e.as_service_error().unwrap();
                            return Err(DeployFunctionError::DeploymentError(
                                format!(
                                    "Failed to update function config {}: {} (Code: {:?})",
                                    function_name,
                                    service_error,
                                    ProvideErrorKind::code(service_error)
                                )
                                .into(),
                            ));
                        }
                    };

                    res.function_arn().unwrap().to_string()
                }
                Err(e) => {
                    let service_error = e.as_service_error().unwrap();
                    return Err(DeployFunctionError::DeploymentError(
                        format!(
                            "Failed to update function {}: {} (Code: {:?})",
                            function_name,
                            service_error,
                            ProvideErrorKind::code(service_error)
                        )
                        .into(),
                    ));
                }
            };

            updated_function_arn
        }
        None => {
            let result = lambda_client
                .create_function()
                .function_name(&*function_name)
                .runtime(aws_sdk_lambda::types::Runtime::Providedal2023)
                .handler("bootstrap") // For custom runtime
                .architectures(aws_sdk_lambda::types::Architecture::Arm64)
                .code(
                    aws_sdk_lambda::types::FunctionCode::builder()
                        .zip_file(zip_data.into())
                        .build(),
                )
                .role(&*role_arn)
                .memory_size(
                    config
                        .deployment
                        .lambda
                        .map(|l| l.memory_size as i32)
                        .unwrap_or(256),
                )
                .tags("application", config.service_name.clone())
                .timeout(30)
                .send()
                .await;

            let function_arn = match result {
                Ok(response) => {
                    println!("✓ Created new Lambda function: {}", function_name);
                    response.function_arn().unwrap().to_string()
                }
                Err(e) => {
                    let service_error = e.as_service_error().unwrap();
                    return Err(DeployFunctionError::DeploymentError(
                        format!(
                            "Failed to create function {}: {} (Code: {:?})",
                            function_name,
                            service_error,
                            ProvideErrorKind::code(service_error)
                        )
                        .into(),
                    ));
                }
            };

            function_arn
        }
    };

    Ok(arn)
}
