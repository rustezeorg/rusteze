use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

type AsyncHandler<R> = Pin<Box<dyn Future<Output = R> + Send>>;

pub struct Command {
    pub name: String,
    pub help_text: String,
    pub operation: Box<dyn Fn(&Vec<String>) -> AsyncHandler<()> + Send + Sync>,
}

pub struct Commander {
    pub commands: HashMap<String, Command>,
}

const LOGO: &str = "██████╗ ██╗   ██╗███████╗████████╗███████╗███████╗███████╗
██╔══██╗██║   ██║██╔════╝╚══██╔══╝██╔════╝╚══███╔╝██╔════╝
██████╔╝██║   ██║███████╗   ██║   █████╗    ███╔╝ █████╗  
██╔══██╗██║   ██║╚════██║   ██║   ██╔══╝   ███╔╝  ██╔══╝  
██║  ██║╚██████╔╝███████║   ██║   ███████╗███████╗███████╗
╚═╝  ╚═╝ ╚═════╝ ╚══════╝   ╚═╝   ╚══════╝╚══════╝╚══════╝";

impl Commander {
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    pub fn register_command(&mut self, command: Command) {
        let _ = &self.commands.insert(command.name.to_string(), command);
    }

    pub async fn match_command(&self, command: &str, args: &Vec<String>) {
        match self.commands.get(command) {
            Some(c) => {
                Commander::print_header();
                let future = (c.operation)(args);
                future.await;
            }
            None => {
                self.print_usage();
                return;
            }
        }
    }

    fn print_header() {
        println!();
        println!();
        println!("{}", LOGO);
        println!();
        println!();
    }

    pub fn print_usage(&self) {
        Commander::print_header();

        if self.commands.len() > 0 {
            println!("USAGE:");
        }

        self.commands
            .values()
            .for_each(|c| println!("{}", c.help_text));
    }
}

pub enum CommandsError {}
