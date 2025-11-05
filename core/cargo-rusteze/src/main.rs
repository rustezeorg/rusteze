use std::env;

use rusteze_cargo::commands::Commander;
use rusteze_cargo::deploy::init_deploy_command;
use rusteze_cargo::serve::init_serve_command;
use tracing::{debug, error};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    let mut command_service = Commander::new();
    command_service.register_command(init_serve_command());
    command_service.register_command(init_deploy_command());

    // Parse command line arguments
    // When called as "cargo rusteze deploy", args are: [binary, "rusteze", "deploy"]
    // When called directly as "rusteze deploy", args are: [binary, "deploy"]
    let command_index = if args.len() > 2 && args[1] == "rusteze" {
        2
    } else {
        1
    };

    debug!(
        "Args length: {}, command_index: {}",
        args.len(),
        command_index
    );

    if args.len() < command_index + 1 {
        error!("Not enough arguments, showing usage");
        command_service.print_usage();
        return;
    }

    let command = &args[command_index];
    debug!("Looking for command: '{}'", command);

    command_service.match_command(command, &args).await;
}
