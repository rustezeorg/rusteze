
use std::env;
use std::fs::{self, File};

#[cfg(feature = "aws")]
use rusteze_cargo_deploy::aws::lambda::deploy_lambda;
use rusteze_cargo_metadata::read_config;
use tokio::runtime::Runtime;

/// Reads `deploy.toml`
fn main() {
    let args: Vec<String> = env::args().collect();

    println!("Args! {:?}", args);

    if args.len() < 2 || args[2] != "deploy" {
        eprintln!("Usage: cargo deploy");
        return;
    }

    let config = read_config();
    
    let rt = Runtime::new().unwrap();
    rt.block_on(async {
        let c = config.clone();
        let deployment_type = &config.deployment.deployment_type.unwrap_or("lambda".to_string());
        
        match deployment_type.as_str() {
            #[cfg(feature = "aws")]
           "lambda" => deploy_lambda(&c).await,
            _ => panic!("Unsupported deployment type!"),
        }
    });
}

