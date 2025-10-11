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
    commit: Option<String>,
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
    if let Some(commit) = &args.commit {
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

    let agent = openai_client
        .completion_model(&args.model)
        .completions_api()
        .into_agent_builder()
        .tool(ExecuteCommandTool::with_container(container_manager))
        .preamble("You are a code review assistant. The git repository has already been cloned to /workspace. You can explore the codebase and use git commands to checkout different commits or branches if needed. Feel free to run unit tests or available static analysis tools. Your job is to audit the state of the codebase.")
        .build();

    let repo_info = if let Some(commit) = &args.commit {
        format!(
            "Repository {} has been cloned and checked out to commit {}",
            args.repo_url, commit
        )
    } else {
        format!(
            "Repository {} has been cloned to the latest commit",
            args.repo_url
        )
    };

    let request = PromptRequest::new(&agent, &repo_info).multi_turn(99);
    // Prompt the model and print its response
    let response = request.await?;

    println!("agent: {response}");

    Ok(())
}
