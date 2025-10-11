mod container;
mod tools;

use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use rig::providers::openai;
use rig::{agent::PromptRequest, client::CompletionClient};

use crate::{container::ContainerManager, tools::execute::ExecuteCommandTool};

#[derive(Debug, Parser)]
struct Cli {
    #[arg(long)]
    model: String,
    #[arg(long)]
    base_url: String,
    #[arg(long)]
    api_key: String,
    #[arg(long)]
    socket: String,
    #[arg(long)]
    repo_url: String,
    #[arg(long)]
    head: Option<String>,
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

    let openai_client = openai::Client::builder(&args.api_key)
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
