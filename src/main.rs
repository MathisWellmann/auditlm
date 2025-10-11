mod container;
mod tools;

use std::sync::Arc;

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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Cli::parse();

    let mut container_manager = ContainerManager::new(&args.socket).await?;
    container_manager.create_analysis_container().await?;
    let container_manager = Arc::new(container_manager);

    let openai_client = openai::Client::builder(&args.api_key)
        .base_url(&args.base_url)
        .build()?;

    let agent = openai_client
        .completion_model(&args.model)
        .completions_api()
        .into_agent_builder()
        .tool(ExecuteCommandTool::with_container(container_manager))
        .preamble("You are a code review assistant. Your job is to explore a git repository and audit the state of the codebase.")
        .build();

    let request = PromptRequest::new(&agent, "This is a react-based chat app.").multi_turn(99);
    // Prompt the model and print its response
    let response = request.await?;

    println!("agent: {response}");

    Ok(())
}
