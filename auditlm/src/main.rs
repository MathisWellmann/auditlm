mod commands;
mod container;
mod tools;

use clap::Parser;

use commands::{forgejo::forgejo_daemon, git::handle_git_command};

/// Command-line arguments for the auditlm tool
#[derive(Debug, Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Parser)]
enum Commands {
    /// Analyze a Git repository with AI
    Git(commands::GitArgs),
    /// Analyze a Forgejo pull request with AI
    Forgejo(commands::ForgejoArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Git(args) => {
            handle_git_command(args).await?;
        }
        Commands::Forgejo(args) => {
            forgejo_daemon(args).await?;
        }
    }

    Ok(())
}
