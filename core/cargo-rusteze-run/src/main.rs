use std::env;
use std::net::SocketAddr;
use std::process;

use rusteze::{config::RustezeConfig, local_server::LocalServer};

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    // Parse command line arguments
    if args.len() < 2 {
        print_usage();
        return;
    }

    let command = &args[1];

    match command.as_str() {
        "rusteze-run" => {
            // This is called as `cargo rusteze-run`
            if args.len() < 3 {
                print_usage();
                return;
            }

            let subcommand = &args[2];
            match subcommand.as_str() {
                "dev" | "serve" => {
                    let port = parse_port(&args).unwrap_or(3000);
                    let hot_reload = parse_hot_reload(&args);
                    start_dev_server(port, hot_reload).await;
                }
                "help" | "--help" | "-h" => {
                    print_usage();
                }
                _ => {
                    eprintln!("Unknown subcommand: {}", subcommand);
                    print_usage();
                    process::exit(1);
                }
            }
        }
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}

async fn start_dev_server(port: u16, hot_reload: bool) {
    println!("🚀 Starting Rusteze development server...");

    // Load configuration
    let config = match RustezeConfig::load() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("❌ Failed to load rusteze.toml: {}", e);
            eprintln!("💡 Make sure you have a rusteze.toml file in your project root");
            process::exit(1);
        }
    };

    // Create and start the local server
    let mut server = LocalServer::new(config);
    let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

    let result = if hot_reload {
        println!("🔥 Hot reload enabled");
        server.start_with_hot_reload(addr).await
    } else {
        server.start_with_dev_logging(addr).await
    };

    if let Err(e) = result {
        eprintln!("❌ Failed to start development server: {}", e);
        process::exit(1);
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

fn print_usage() {
    println!("Rusteze Development Server");
    println!();
    println!("USAGE:");
    println!("    cargo rusteze-run dev [OPTIONS]");
    println!("    cargo rusteze-run serve [OPTIONS]");
    println!();
    println!("SUBCOMMANDS:");
    println!("    dev, serve    Start the local development server");
    println!("    help          Show this help message");
    println!();
    println!("OPTIONS:");
    println!("    -p, --port <PORT>       Port to run the server on [default: 3000]");
    println!("    --hot-reload, --watch   Enable hot reload (experimental)");
    println!("    -h, --help              Show this help message");
    println!();
    println!("EXAMPLES:");
    println!("    cargo rusteze-run dev                    # Start server on port 3000");
    println!("    cargo rusteze-run dev --port 8080        # Start server on port 8080");
    println!("    cargo rusteze-run dev --hot-reload       # Start with hot reload");
    println!("    cargo rusteze-run serve -p 5000 --watch  # Start on port 5000 with hot reload");
}
