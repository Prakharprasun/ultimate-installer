use clap::Parser;

/// Ultimate Installer — natural language shell compiler.
///
/// Translates user intent into safe, executable shell scripts using
/// a multi-model LLM pipeline and community-verified script cache.
#[derive(Parser, Debug)]
#[command(name = "ulti", version, about, long_about = None)]
struct Cli {
    /// The natural-language command to execute, e.g. "install neofetch"
    #[arg()]
    command: Option<String>,

    /// Run the first-time configuration wizard
    #[arg(long)]
    config: bool,
}

fn main() {
    let cli = Cli::parse();

    if cli.config {
        println!("🔧 ulti config — not yet implemented");
        return;
    }

    match cli.command {
        Some(cmd) => {
            println!("🚀 ulti received: \"{cmd}\"");
            println!("   (pipeline not yet implemented)");
        }
        None => {
            // Cold start check: no command given, show help or wizard
            eprintln!("⚠ No command provided.\n");
            eprintln!("Usage:  ulti \"install neofetch\"");
            eprintln!("        ulti --config\n");
            eprintln!("Run `ulti --help` for more options.");
        }
    }
}
