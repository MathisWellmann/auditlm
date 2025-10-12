use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use rig::providers::openai;
use rig::{agent::PromptRequest, client::CompletionClient};
use rmcp::{
    model::{ClientCapabilities, ClientInfo},
    service::ServiceExt,
};
use rmcp_openapi::Server;
use url::Url;

use crate::container::ContainerManager;
use crate::tools::execute::ExecuteCommandTool;

/// Command-line arguments for the forgejo subcommand
#[derive(Debug, Parser)]
pub struct ForgejoArgs {
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

    /// URL of the Forgejo instance to connect to
    #[arg(long)]
    forgejo_url: String,

    /// Docker image to use for analysis (e.g., "rust:1-trixie", "ubuntu:22.04")
    #[arg(long)]
    image: String,

    /// Repository name in the format "owner/repo"
    repository: String,

    /// The index of the pull request under review.
    pull_request_index: u64,
}

// Helper functions for git operations
async fn clone_repository(
    forgejo_url: &str,
    repository: &str,
    container_manager: &ContainerManager,
) -> anyhow::Result<()> {
    let repo_url = format!("{}/{}.git", forgejo_url, repository);
    println!("Cloning repository: {}", repo_url);

    let clone_command = vec![
        "git".to_string(),
        "clone".to_string(),
        repo_url,
        ".".to_string(),
    ];

    let clone_result = container_manager
        .execute_command(&clone_command)
        .await
        .context("Failed to clone repository")?;

    println!("Clone result: {}", clone_result);
    Ok(())
}

async fn get_git_diff(base: &str, container_manager: &ContainerManager) -> anyhow::Result<String> {
    println!("Getting git diff from base: {}", base);
    let diff_command = vec![
        "git".to_string(),
        "--no-pager".to_string(),
        "diff".to_string(),
        base.to_string(),
    ];

    let diff_output = container_manager
        .execute_command(&diff_command)
        .await
        .context("Failed to get git diff")?;

    Ok(diff_output)
}

pub async fn handle_forgejo_command(args: ForgejoArgs) -> anyhow::Result<()> {
    // Initialize container manager
    let mut container_manager = ContainerManager::new(&args.socket).await?;
    container_manager
        .create_analysis_container(&args.image)
        .await?;

    // Download Forgejo OpenAPI spec
    let openapi_spec = serde_json::from_str(include_str!("../../api/forgejo.v1.json"))?;

    // Create OpenAPI server with Forgejo spec
    let forgejo_base_url = Url::parse(&args.forgejo_url).context("Failed to parse Forgejo URL")?;

    let openapi_server = Server::builder()
        .openapi_spec(openapi_spec)
        .base_url(forgejo_base_url)
        .build();

    // Load tools from the OpenAPI spec
    let mut openapi_server = openapi_server;
    openapi_server
        .load_openapi_spec()
        .context("Failed to load OpenAPI tools")?;

    println!("Loaded {} OpenAPI tools", openapi_server.tool_count());

    // Extract tool names from the OpenAPI server
    let openapi_tool_names = openapi_server.get_tool_names();
    println!("OpenAPI tool names: {:?}", openapi_tool_names);

    // No need to create a Forgejo server since we're using the OpenAPI server

    // Start a local MCP server with the OpenAPI server
    let (server_handle, server_addr) = start_local_mcp_server_with_openapi(openapi_server).await?;

    // Give the server a moment to start up
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Connect to the MCP server as a client using TCP
    println!("Connecting to MCP server at: {}", server_addr);
    let stream = tokio::net::TcpStream::connect(&server_addr)
        .await
        .context("Failed to connect to MCP server")?;
    let transport = stream;

    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: rmcp::model::Implementation {
            name: "auditlm-forgejo-client".to_string(),
            version: "0.1.0".to_string(),
            title: Some("AuditLM Forgejo Client".to_string()),
            website_url: Some("https://github.com/auditlm/auditlm".to_string()),
            icons: None,
        },
    };

    println!("Creating client");
    let client = client_info
        .serve(transport)
        .await
        .context("Failed to connect to MCP server")?;

    // Check if the transport is still open
    if client.is_transport_closed() {
        return Err(anyhow::anyhow!("Transport is closed after client creation"));
    }

    // Get server info to verify connection
    println!("Getting server info");
    let server_info = client
        .peer_info()
        .ok_or_else(|| anyhow::anyhow!("No server info available"))?;
    println!("Connected to server: {:?}", server_info);

    println!("Cloning repository");
    // Clone repository and get diff
    clone_repository(&args.forgejo_url, &args.repository, &container_manager).await?;

    // Check transport again before listing tools
    if client.is_transport_closed() {
        return Err(anyhow::anyhow!("Transport is closed before listing tools"));
    }

    // List available tools
    println!("Getting available tools");
    let tools_result = client
        .list_tools(Default::default())
        .await
        .context("Failed to list tools")?;
    let tools = tools_result.tools;
    println!("Available tools: {:?}", tools);

    // Create OpenAI client
    let openai_client = openai::Client::builder(&args.api_key.unwrap_or(String::new()))
        .base_url(&args.base_url)
        .build()?;

    // Create agent with MCP tools
    let agent = openai_client
        .completion_model(&args.model)
        .completions_api()
        .into_agent_builder()
        .preamble("You are a code review assistant with access to Forgejo repository tools. To access the diff for the pull request under review, you should call the repoGetPullRequest tool. The git repository under review has been cloned to /workspace. You can explore the codebase to get context on on the change. You can run arbitrary commands like `grep`, `cargo doc` or similar in the repository. Please write a succinct review and send it to the forgejo server using your repoCreatePullReview tool.")
        .tool(ExecuteCommandTool::with_container(Arc::new(container_manager)));

    let tool_allowlist = vec![
        "repoGetPullRequest".to_string(),
        "repoGetPullRequestCommits".to_string(),
        "repoGetPullRequestFiles".to_string(),
        "repoCreatePullReview".to_string(),
        "repoCreatePullReviewComment".to_string(),
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

    // Create prompt with diff
    let prompt = format!(
        "The repository owner and name is `{}` and the index of the pull request under review is {}",
        args.repository, args.pull_request_index
    );

    // Send prompt to agent
    let request = PromptRequest::new(&agent, &prompt).multi_turn(999);
    let response = request.await?;

    println!("Agent response: {}", response);

    // Keep the server running until we're done
    println!("Shutting down MCP server");
    server_handle.abort();
    let _ = server_handle.await;

    Ok(())
}

async fn start_local_mcp_server_with_openapi(
    server: rmcp_openapi::Server,
) -> anyhow::Result<(tokio::task::JoinHandle<()>, String)> {
    // Bind to a random available port on localhost
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let server_addr = addr.to_string();

    println!("OpenAPI MCP server listening on: {}", server_addr);

    let handle = tokio::spawn(async move {
        // Accept only one connection for now
        match listener.accept().await {
            Ok((stream, addr)) => {
                println!("Client connected to OpenAPI MCP server from: {}", addr);
                let server_clone = server.clone();

                // Set TCP keepalive to prevent connection drops
                if let Ok(socket) = stream.peer_addr() {
                    println!("Client socket address: {}", socket);
                }

                // Use the serve_with_ct method from ServiceExt trait
                let ct = tokio_util::sync::CancellationToken::new();
                match server_clone.serve_with_ct(stream, ct.clone()).await {
                    Ok(running_service) => {
                        println!("OpenAPI MCP server successfully started");
                        // Keep the service running
                        if let Err(e) = running_service.waiting().await {
                            println!("OpenAPI MCP server finished with error: {}", e);
                        } else {
                            println!("OpenAPI MCP server finished normally");
                        }
                    }
                    Err(e) => {
                        eprintln!("OpenAPI MCP server error: {}", e);
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
