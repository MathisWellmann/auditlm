use anyhow::Context;
use rig::providers::openai;
use rig::{agent::PromptRequest, client::CompletionClient};
use rmcp::Peer;
use rmcp::model::InitializeRequestParam;
use rmcp::service::RunningService;
use rmcp::{
    model::{ClientCapabilities, ClientInfo},
    service::ServiceExt,
};
use std::sync::Arc;
use std::time::Instant;
use tracing::info;

use crate::commands::forgejo::common::{client_info, defaults, tools};
use crate::commands::forgejo::error::ForgejoError;
use crate::commands::forgejo::types::ForgejoArgs;
use crate::container::ContainerManager;
use crate::tools::execute::ExecuteCommandTool;
use crate::tools::todo::TodoListTool;

/// Set up MCP client connection
///
/// Establishes a connection to an MCP server using TCP transport.
///
/// # Arguments
///
/// * `server_addr` - Address of the MCP server to connect to
///
/// # Returns
///
/// Returns a running MCP client service or an error if connection fails.
pub async fn setup_mcp_client(
    server_addr: &str,
) -> Result<RunningService<rmcp::RoleClient, InitializeRequestParam>, ForgejoError> {
    let start_time = Instant::now();
    info!("Setting up MCP client connection to: {}", server_addr);

    // Connect to the MCP server as a client using TCP
    println!("Connecting to MCP server at: {}", server_addr);
    let stream = tokio::net::TcpStream::connect(server_addr)
        .await
        .context("Failed to connect to MCP server")
        .map_err(|e| ForgejoError::McpConnection(e.to_string()))?;
    let transport = stream;

    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: rmcp::model::Implementation {
            name: client_info::NAME.to_string(),
            version: client_info::VERSION.to_string(),
            title: Some(client_info::TITLE.to_string()),
            website_url: Some(client_info::WEBSITE_URL.to_string()),
            icons: None,
        },
    };

    println!("Creating client");
    let client = client_info
        .serve(transport)
        .await
        .context("Failed to connect to MCP server")
        .map_err(|e| ForgejoError::McpConnection(e.to_string()))?;

    // Check if the transport is still open
    if client.is_transport_closed() {
        return Err(ForgejoError::Transport(
            "Transport is closed after client creation".to_string(),
        ));
    }

    // Get server info to verify connection
    println!("Getting server info");
    let server_info = client
        .peer_info()
        .ok_or_else(|| ForgejoError::McpConnection("No server info available".to_string()))?;
    println!("Connected to server: {:?}", server_info);

    let duration = start_time.elapsed();
    info!("MCP client setup completed in {:?}", duration);

    Ok(client)
}

/// Create and configure the OpenAI agent with MCP tools
///
/// Creates an AI agent configured with OpenAI completion model and a set of tools
/// for interacting with Forgejo repositories.
///
/// # Arguments
///
/// * `args` - Command-line arguments containing configuration
/// * `container_manager` - Container manager for executing commands
/// * `tools` - List of MCP tools to provide to the agent
/// * `client` - MCP client for tool communication
///
/// # Returns
///
/// Returns a configured AI agent or an error if creation fails.
pub async fn create_agent_with_tools(
    args: &ForgejoArgs,
    container_manager: Arc<ContainerManager>,
    tools: Vec<rmcp::model::Tool>,
    client: Peer<rmcp::RoleClient>,
) -> Result<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>, ForgejoError> {
    let start_time = Instant::now();
    info!(
        "Creating agent with {} tools for model: {}",
        tools.len(),
        args.model
    );

    // Create OpenAI client
    let openai_client = openai::Client::builder(&args.api_key.clone().unwrap_or(String::new()))
        .base_url(&args.base_url)
        .build()
        .map_err(|e| ForgejoError::Agent(format!("Failed to create OpenAI client: {}", e)))?;

    // Create agent with MCP tools
    let agent = openai_client
        .completion_model(&args.model)
        .completions_api()
        .into_agent_builder()
        .preamble(include_str!("../../../prompts/forgejo_prompt.txt"))
        .max_tokens(defaults::MAX_TOKENS)
        .temperature(defaults::TEMPERATURE)
        .tool(ExecuteCommandTool::with_container(
            container_manager.clone(),
        ))
        .tool(TodoListTool::new());

    let tool_allowlist = vec![
        tools::GET_PULL_REQUEST.to_string(),
        tools::GET_PULL_REQUEST_COMMITS.to_string(),
    ];

    // Add MCP tools to the agent
    let agent = tools
        .into_iter()
        .filter(|tool| tool_allowlist.contains(&tool.name.to_string()))
        .fold(agent, |agent, tool| {
            println!("Providing {:?} to agent.", tool);
            agent.rmcp_tool(tool, client.clone())
        })
        .build();

    let duration = start_time.elapsed();
    info!("Agent creation completed in {:?}", duration);

    Ok(agent)
}

/// Start a local MCP server with OpenAPI
pub async fn start_local_mcp_server_with_openapi(
    server: rmcp_openapi::Server,
) -> Result<(tokio::task::JoinHandle<()>, String), ForgejoError> {
    // Bind to a random available port on localhost
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .context("Failed to bind to local port")
        .map_err(|e| ForgejoError::McpConnection(e.to_string()))?;

    let addr = listener
        .local_addr()
        .context("Failed to get local address")
        .map_err(|e| ForgejoError::McpConnection(e.to_string()))?;

    let server_addr = addr.to_string();

    println!("OpenAPI MCP server listening on: {}", server_addr);

    let handle = tokio::spawn(async move {
        match listener.accept().await {
            Ok((stream, addr)) => {
                println!("Client connected to OpenAPI MCP server from: {}", addr);
                if let Ok(socket) = stream.peer_addr() {
                    println!("Client socket address: {}", socket);
                }

                if let Ok(service) = server.serve(stream).await {
                    match service.waiting().await {
                        Ok(reason) => println!("Shut down MCP server with reason: {:?}", reason),
                        Err(e) => eprintln!("Join error: {:?}", e),
                    }
                }
                println!("OpenAPI MCP server connection closed");
            }
            Err(e) => {
                eprintln!("Failed to accept connection: {}", e);
            }
        }
    });

    Ok((handle, server_addr))
}

/// Process a pull request review
pub async fn process_pr_review(
    args: &ForgejoArgs,
    pr_info: &crate::commands::forgejo::types::PrInfo,
    container_manager: Arc<ContainerManager>,
    client: RunningService<rmcp::RoleClient, InitializeRequestParam>,
) -> Result<String, ForgejoError> {
    let start_time = Instant::now();
    info!(
        "Starting PR review processing for {}/{}#{}",
        pr_info.owner, pr_info.repo, pr_info.index
    );

    // Check transport before listing tools
    if client.is_transport_closed() {
        return Err(ForgejoError::Transport(
            "Transport is closed before listing tools".to_string(),
        ));
    }

    // List available tools
    let tools_start = Instant::now();
    println!("Getting available tools");
    let tools_result = client
        .list_tools(Default::default())
        .await
        .map_err(|e| ForgejoError::McpConnection(format!("Failed to list tools: {}", e)))?;
    let tools = tools_result.tools;
    let tools_duration = tools_start.elapsed();
    info!(
        "Tool listing completed in {:?}, found {} tools",
        tools_duration,
        tools.len()
    );
    println!("Available tools: {:?}", tools);

    // Create agent with tools
    let agent = create_agent_with_tools(args, container_manager, tools, client.clone()).await?;

    // Create prompt with PR timeline.
    let prompt_start = Instant::now();
    let prompt = format!(
        "The repository owner is `{}` and the repository name is `{}` and the index of the pull request under review is {}. The repository has been checked out to its default branch. Use the `repoGetPullRequest` tool to determine which branch to check out for review. The full timeline of the PR review follows: {:?}\n\nThe diff of the PR follows: {}",
        pr_info.owner, pr_info.repo, pr_info.index, pr_info.timeline, pr_info.diff
    );
    info!(
        "Prompt created in {:?}, length: {} characters",
        prompt_start.elapsed(),
        prompt.len()
    );

    // Send prompt to agent
    let agent_start = Instant::now();
    let request = PromptRequest::new(&agent, &prompt).multi_turn(999);
    let response = request
        .await
        .map_err(|e| ForgejoError::Agent(format!("Agent request failed: {}", e)))?;
    let agent_duration = agent_start.elapsed();
    info!(
        "Agent processing completed in {:?}, response length: {} characters",
        agent_duration,
        response.len()
    );

    let total_duration = start_time.elapsed();
    info!("PR review processing completed in {:?}", total_duration);

    println!("Agent response: {}", response);
    Ok(response)
}
