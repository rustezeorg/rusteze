use std::env;
use std::net::SocketAddr;
use std::process;

use rusteze_cargo::commands::Commander;
use rusteze_cargo::deploy::init_deploy_command;
use rusteze_cargo::serve::init_serve_command;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    let mut command_service = Commander::new();
    println!("Created commander");

    command_service.register_command(init_serve_command());
    println!("Registered serve command");

    command_service.register_command(init_deploy_command());
    println!("Registered deploy command");

    println!("args: {:?}", &args);
    println!("Starting command processing...");

    // Parse command line arguments
    // When called as "cargo rusteze deploy", args are: [binary, "rusteze", "deploy"]
    // When called directly as "rusteze deploy", args are: [binary, "deploy"]
    let command_index = if args.len() > 2 && args[1] == "rusteze" {
        2
    } else {
        1
    };

    println!(
        "Args length: {}, command_index: {}",
        args.len(),
        command_index
    );

    if args.len() < command_index + 1 {
        println!("Not enough arguments, showing usage");
        command_service.print_usage();
        return;
    }

    let command = &args[command_index];
    println!("Looking for command: '{}'", command);

    command_service.match_command(command, &args).await;
}
