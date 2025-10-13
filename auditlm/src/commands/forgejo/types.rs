use clap::Parser;

/// Command-line arguments for the forgejo subcommand
#[derive(Debug, Parser, Clone)]
pub struct ForgejoArgs {
    /// The OpenAI model to use for code review (e.g., "gpt-4", "gpt-3.5-turbo")
    #[arg(long)]
    pub model: String,

    /// The base URL for the OpenAI-compatible API endpoint
    #[arg(long)]
    pub base_url: String,

    /// Optional API key for authentication to a cloud provider. If not provided, an empty string will be used for e.g. a local llama.cpp service.
    #[arg(long)]
    pub api_key: Option<String>,

    /// Path to the Docker socket for container management
    #[arg(long)]
    pub socket: String,

    /// URL of the Forgejo instance to connect to
    #[arg(long)]
    pub forgejo_url: String,

    /// Docker image to use for analysis (e.g., "rust:1-trixie", "ubuntu:22.04")
    #[arg(long)]
    pub image: String,

    /// How often to check for new @ mentions.
    #[arg(long, default_value = "30")]
    pub interval: u64,
}

/// Information about a pull request
#[derive(Debug, Clone)]
pub struct PrInfo {
    pub owner: String,
    pub repo: String,
    pub index: u64,
    pub diff: String,
    pub timeline: Vec<serde_json::Value>,
}
