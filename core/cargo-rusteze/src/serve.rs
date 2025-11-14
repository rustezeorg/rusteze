use std::process;

use crate::commands::Command;
use rusteze::local_server::start_dev_server;

const HELP_TEXT: &str = "
serve           Start the local development server

OPTIONS:
    -p, --port <PORT>       Port to run the server on [default: 3000]
    --hot-reload, --watch   Enable hot reload (experimental)
    -h, --help              Show this help message
";

pub fn init_serve_command() -> Command {
    Command {
        name: "serve".to_string(),
        help_text: "serve           Start the local development server".to_string(),
        operation: Box::new(|args| Box::pin(serve_command(args.clone()))),
    }
}

pub async fn serve_command(args: Vec<String>) {
    println!("Running serve command with args {:?}", args);

    // Check for help flags in any position
    if args
        .iter()
        .any(|arg| arg == "help" || arg == "--help" || arg == "-h")
    {
        println!("{}", HELP_TEXT);
        return;
    }

    let port = parse_port(&args).unwrap_or(3000);
    let hot_reload = parse_hot_reload(&args);

    match start_dev_server(port, hot_reload).await {
        Ok(_) => println!("Dev server completed successfully"),
        Err(e) => println!("Dev server error: {:?}", e),
    }
}

fn parse_port(args: &[String]) -> Option<u16> {
    for i in 0..args.len() {
        if (args[i] == "--port" || args[i] == "-p") && i + 1 < args.len() {
            return args[i + 1].parse().ok();
        }
    }
    None
}

fn parse_hot_reload(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--hot-reload" || arg == "--watch")
}
