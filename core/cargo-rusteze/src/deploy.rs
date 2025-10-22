use std::path::Path;
use std::process;

use crate::commands::Command;
use crate::read_config;

#[cfg(feature = "aws")]
use crate::aws::lambda::deploy_to_aws;

const HELP_TEXT: &str = "
OPTIONS:
    -d, --dry-run   Performs a dry run without actually deploying.
    -h, --help      Show this help message
";

pub fn init_deploy_command() -> Command {
    Command {
        name: "deploy".to_string(),
        help_text: "deploy          Deploy your program.".to_string(),
        operation: Box::new(|args| Box::pin(deploy_command(args.clone()))),
    }
}

pub async fn deploy_command(args: Vec<String>) {
    println!("Running deploy command with args {:?}", args);

    if args.len() >= 3 {
        let subcommand = &args[2];

        match subcommand.as_str() {
            "help" | "--help" | "-h" => {
                println!("{}", HELP_TEXT);
                return;
            }
            _ => {
                println!("Unknown command: {}", subcommand);
                process::exit(1);
            }
        };
    }

    // Check if .rusteze directory exists
    if !Path::new(".rusteze").exists() {
        println!("Error: .rusteze directory not found. Please run 'cargo rusteze codegen' first.");
        process::exit(1);
    }

    // Read configuration
    let config = read_config();
    println!("Deploying service: {}", config.service_name);

    match config.deployment.provider.as_str() {
        "aws" => {
            #[cfg(feature = "aws")]
            {
                if let Err(e) = deploy_to_aws(&config).await {
                    println!("Deployment failed: {}", e);
                    process::exit(1);
                }
            }
            #[cfg(not(feature = "aws"))]
            {
                println!("AWS deployment not supported. Please compile with --features aws");
                process::exit(1);
            }
        }
        _ => {
            println!("Unsupported provider: {}", config.deployment.provider);
            process::exit(1);
        }
    }
}
