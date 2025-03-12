use std::process;

use crate::commands::Command;

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
                print!("Unknown command: {}", subcommand);
                process::exit(0)
            }
        };
    }
}
