mod container;
mod tools;

use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use rig::providers::openai;
use rig::{agent::PromptRequest, client::CompletionClient};

use crate::{container::ContainerManager, tools::execute::ExecuteCommandTool};

/// Command-line arguments for the auditlm tool
#[derive(Debug, Parser)]
struct Cli {
    /// The OpenAI model to use for code review (e.g., "gpt-4", "gpt-3.5-turbo")
    #[arg(long)]
    model: String,

    /// The base URL for the OpenAI-compatible API endpoint
    #[arg(long)]
    base_url: String,

    /// Optional API key for authentication to a cloud provider. If not provided, an empty string will be used for e.g. a local llama.cpp service.
    #[arg(long)]
    api_key: Option<String>,

    /// Path to the Docker socket for container management
    #[arg(long)]
    socket: String,

    /// URL of the Git repository to clone and analyze
    #[arg(long)]
    repo_url: String,

    /// Optional specific commit hash or branch to checkout after cloning
    #[arg(long)]
    head: Option<String>,

    /// The base Git reference to compare against when generating the diff (e.g., branch name, commit hash)
    #[arg(long)]
    base: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Cli::parse();

    let mut container_manager = ContainerManager::new(&args.socket).await?;
    container_manager.create_analysis_container().await?;

    // Clone the repository into the container
    println!("Cloning repository: {}", args.repo_url);
    let clone_command = vec![
        "git".to_string(),
        "clone".to_string(),
        args.repo_url.clone(),
        "/workspace".to_string(),
    ];
    let clone_result = container_manager
        .execute_command(&clone_command)
        .await
        .context("Failed to clone repository")?;
    println!("Clone result: {}", clone_result);

    // Checkout specific commit if provided
    if let Some(commit) = &args.head {
        println!("Checking out commit: {}", commit);
        let checkout_command = vec!["git".to_string(), "checkout".to_string(), commit.clone()];
        let checkout_result = container_manager
            .execute_command(&checkout_command)
            .await
            .context("Failed to checkout commit")?;
        println!("Checkout result: {}", checkout_result);
    }

    let container_manager = Arc::new(container_manager);

    let openai_client = openai::Client::builder(&args.api_key.unwrap_or(String::new()))
        .base_url(&args.base_url)
        .build()?;

    println!("Getting git diff from base: {}", args.base);
    let diff_command = vec![
        "git".to_string(),
        "--no-pager".to_string(),
        "diff".to_string(),
        args.base.clone(),
    ];
    let diff_output = container_manager
        .execute_command(&diff_command)
        .await
        .context("Failed to get git diff")?;

    let agent = openai_client
        .completion_model(&args.model)
        .completions_api()
        .into_agent_builder()
        .tool(ExecuteCommandTool::with_container(container_manager))
        .preamble("You are a code review assistant. The git repository under review has been cloned to /workspace. You can explore the codebase to get context on the provided changes. You can run arbitrary commands like `grep`, `cargo doc` or similar. Please provide a succinct review.")
        .build();

    // Get the git diff output

    let prompt = format!(
        "Please review the following git diff output:\n\n{}",
        diff_output
    );

    let request = PromptRequest::new(&agent, &prompt).multi_turn(999);
    // Prompt the model and print its response
    let response = request.await?;

    println!("agent: {response}");

    Ok(())
}
