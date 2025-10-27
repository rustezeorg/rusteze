use std::path::Path;
use std::process;

use crate::commands::Command;
use crate::read_config;

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

    // Find the index of "deploy" command and check for subcommands after it
    let deploy_index = args.iter().position(|arg| arg == "deploy").unwrap_or(0);
    let subcommand_index = deploy_index + 1;

    if args.len() > subcommand_index {
        let subcommand = &args[subcommand_index];

        match subcommand.as_str() {
            "help" | "--help" | "-h" => {
                println!("{}", HELP_TEXT);
                return;
            }
            _ => {
                // If it starts with -, it's a flag, otherwise it's an unknown command
                if subcommand.starts_with('-') {
                    println!("Unknown flag: {}", subcommand);
                } else {
                    println!("Unknown subcommand: {}", subcommand);
                }
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
            if let Err(e) = deploy_to_aws(&config).await {
                println!("Deployment failed: {}", e);
                process::exit(1);
            }
        }
        _ => {
            println!("Unsupported provider: {}", config.deployment.provider);
            process::exit(1);
        }
    }
}
