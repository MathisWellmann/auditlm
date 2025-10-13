use anyhow::Context;
use clap::Parser;
use forgejo_api::Forgejo;
use forgejo_api::structs::{IssueGetCommentsAndTimelineQuery, IssueSearchIssuesQuery};
use http::HeaderMap;
use http::header::AUTHORIZATION;
use rig::providers::openai;
use rig::{agent::PromptRequest, client::CompletionClient};
use rmcp::Peer;
use rmcp::model::InitializeRequestParam;
use rmcp::service::RunningService;
use rmcp::{
    model::{ClientCapabilities, ClientInfo},
    service::ServiceExt,
};
use rmcp_openapi::Server;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use time::OffsetDateTime;
use url::Url;

use crate::container::ContainerManager;
use crate::tools::execute::ExecuteCommandTool;

/// Errors encountered in the task processing loop.
#[derive(Error, Debug)]
pub enum ForgejoTaskError {
    #[error("Forgejo API error: {0}")]
    ForgejoApi(#[from] forgejo_api::ForgejoError),
    #[error("Non-fatal error: {0}")]
    NonFatal(#[from] anyhow::Error),
}

/// Command-line arguments for the forgejo subcommand
#[derive(Debug, Parser, Clone)]
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

    /// How often to check for new @ mentions.
    #[arg(long, default_value = "30")]
    interval: u64,
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

// Initialize container manager and create analysis container
async fn initialize_container_manager(
    socket: &str,
    image: &str,
) -> anyhow::Result<ContainerManager> {
    let mut container_manager = ContainerManager::new(socket).await?;
    container_manager.create_analysis_container(image).await?;
    Ok(container_manager)
}

// Set up OpenAPI server with Forgejo spec
async fn setup_openapi_server(forgejo_url: &str) -> anyhow::Result<rmcp_openapi::Server> {
    // Download Forgejo OpenAPI spec
    let openapi_spec = serde_json::from_str(include_str!("../../api/forgejo.v1.json"))?;

    // Create OpenAPI server with Forgejo spec
    let forgejo_base_url = Url::parse(forgejo_url).context("Failed to parse Forgejo URL")?;

    let forgejo_token =
        env::var("FORGEJO_TOKEN").expect("FORGEJO_TOKEN environment variable not set");

    // Create authorization header
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("token {}", forgejo_token)
            .parse()
            .context("Failed to parse authorization header")?,
    );

    let mut openapi_server = Server::builder()
        .openapi_spec(openapi_spec)
        .base_url(forgejo_base_url)
        .default_headers(headers)
        .build();

    // Load tools from the OpenAPI spec
    openapi_server
        .load_openapi_spec()
        .context("Failed to load OpenAPI tools")?;

    println!("Loaded {} OpenAPI tools", openapi_server.tool_count());

    // Extract tool names from the OpenAPI server
    let openapi_tool_names = openapi_server.get_tool_names();
    println!("OpenAPI tool names: {:?}", openapi_tool_names);

    Ok(openapi_server)
}

// Set up MCP client connection
async fn setup_mcp_client(
    server_addr: &str,
) -> anyhow::Result<RunningService<rmcp::RoleClient, InitializeRequestParam>> {
    // Connect to the MCP server as a client using TCP
    println!("Connecting to MCP server at: {}", server_addr);
    let stream = tokio::net::TcpStream::connect(server_addr)
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

    Ok(client)
}

// Create and configure the OpenAI agent with MCP tools
async fn create_agent_with_tools(
    args: &ForgejoArgs,
    container_manager: Arc<ContainerManager>,
    tools: Vec<rmcp::model::Tool>,
    client: Peer<rmcp::RoleClient>,
) -> anyhow::Result<rig::agent::Agent<rig::providers::openai::completion::CompletionModel>> {
    // Create OpenAI client
    let openai_client = openai::Client::builder(&args.api_key.clone().unwrap_or(String::new()))
        .base_url(&args.base_url)
        .build()?;

    // Create agent with MCP tools
    let agent = openai_client
        .completion_model(&args.model)
        .completions_api()
        .into_agent_builder()
        .preamble(include_str!("../../prompts/forgejo_prompt.txt"))
        .tool(ExecuteCommandTool::with_container(container_manager));

    let tool_allowlist = vec![
        "repoGetPullRequest".to_string(),
        "repoGetPullRequestCommits".to_string(),
        "repoGetPullRequestFiles".to_string(),
        "repoCreatePullReview".to_string(),
        "repoCreatePullReviewComment".to_string(),
        "repoListPullReview".to_string(),
        "repoGetPullReview".to_string(),
        "repoGetPullReviewComments".to_string(),
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

    Ok(agent)
}

// Process mentioned issues and queue appropriate tasks
async fn process_mentioned_issues(
    args: &ForgejoArgs,
    forgejo: &Forgejo,
    authenticated_user: &forgejo_api::structs::User,
) -> Result<(), ForgejoTaskError> {
    let issues = forgejo
        .issue_search_issues(IssueSearchIssuesQuery {
            since: Some(OffsetDateTime::now_utc().saturating_sub(time::Duration::hours(24))),
            mentioned: Some(true),
            ..Default::default()
        })
        .await?;

    for issue in &issues.1 {
        let id = match issue.id {
            Some(id) => id,
            None => {
                tracing::error!("Found issue without ID: {:?}", issue);
                continue;
            }
        };

        let repo_meta = match &issue.repository {
            Some(repo) => repo,
            None => {
                // Not much we can do without a respository.
                tracing::error!("Found issue without repository: {:?}", issue);
                continue;
            }
        };

        // Extract owner and repository name from repo_meta
        let owner = match &repo_meta.owner {
            Some(owner) => owner.clone(),
            None => {
                tracing::error!("Found repository without owner: {:?}", repo_meta);
                continue;
            }
        };

        let repository = match &repo_meta.name {
            Some(name) => name.clone(),
            None => {
                tracing::error!("Found repository without name: {:?}", repo_meta);
                continue;
            }
        };

        // Check to see if we've weighed in since the mention (avoid double-comments)
        if !should_comment_on_issue(forgejo, &owner, &repository, id as u64, authenticated_user)
            .await?
        {
            continue;
        }

        if issue.pull_request.is_some() {
            if let Err(e) = review_forgejo_pr(&args, forgejo, &owner, &repository, id as u64).await
            {
                tracing::error!("Error while reviewing PR {e}");
            }
        } else {
            tracing::info!(
                "Got a task to look into issue {owner}/{repository}#{id} (unimplemented)"
            )
        }
    }

    Ok(())
}

// Check if we should comment on an issue based on recent comments
async fn should_comment_on_issue(
    forgejo: &Forgejo,
    owner: &str,
    repository: &str,
    issue_index: u64,
    authenticated_user: &forgejo_api::structs::User,
) -> anyhow::Result<bool> {
    let (_headers, issue_comments) = forgejo
        .issue_get_comments_and_timeline(
            owner,
            repository,
            issue_index,
            IssueGetCommentsAndTimelineQuery::default(),
        )
        .await?;

    let mut most_recent_mention: Option<usize> = None;
    let mut most_recent_own_comment: Option<usize> = None;

    for (index, comment) in issue_comments.iter().enumerate() {
        if comment.user.as_ref().map(|u| u.login.clone()).flatten()
            == authenticated_user.login.clone()
        {
            most_recent_own_comment = Some(index);
        }
        if comment.body.as_ref().map(|body| {
            body.contains(&format!(
                "@{}",
                authenticated_user.login.clone().unwrap_or_default()
            ))
        }) == Some(true)
        {
            most_recent_mention = Some(index);
        }
    }

    let (most_recent_mention, most_recent_own_comment) = match (
        most_recent_mention,
        most_recent_own_comment,
    ) {
        (None, _) => {
            tracing::info!(
                "It appears we were never mentioned in {}/{}#{} This indicates a logic error or deleted comment.",
                owner,
                repository,
                issue_index
            );
            return Ok(false);
        }
        (Some(_), None) => {
            tracing::info!(
                "We were mentioned in {}/{}#{} and have never commented in it.",
                owner,
                repository,
                issue_index
            );
            return Ok(true);
        }
        (Some(mention), Some(comment)) => (mention, comment),
    };

    if most_recent_mention > most_recent_own_comment {
        tracing::info!(
            "We were mentioned more recently in {}/{}#{} than our last comment.",
            owner,
            repository,
            issue_index
        );
        Ok(true)
    } else {
        tracing::info!(
            "We've commented in {}/{}#{} since our last mention.",
            owner,
            repository,
            issue_index
        );
        Ok(false)
    }
}

pub async fn forgejo_dameon(args: ForgejoArgs) -> anyhow::Result<()> {
    let forgejo_token =
        env::var("FORGEJO_TOKEN").expect("FORGEJO_TOKEN environment variable not set");

    let forgejo = Forgejo::new(
        forgejo_api::Auth::Token(&forgejo_token),
        Url::parse(&args.forgejo_url)?,
    )?;

    let authenticated_user = forgejo.user_get_current().await?;

    // Loop forever finding and queueing assigned tasks.
    loop {
        match process_mentioned_issues(&args, &forgejo, &authenticated_user).await {
            Ok(()) => {
                // Successfully processed issues
            }
            Err(ForgejoTaskError::NonFatal(e)) => {
                tracing::error!("Non-fatal error processing mentioned issues: {}", e);
            }
            Err(ForgejoTaskError::ForgejoApi(e)) => {
                tracing::error!("Forgejo API error processing mentioned issues: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_secs(args.interval)).await
    }
}

pub async fn review_forgejo_pr(
    args: &ForgejoArgs,
    forgejo: &Forgejo,
    owner: &str,
    repo: &str,
    pr_index: u64,
) -> anyhow::Result<()> {
    // Initialize container manager
    let container_manager = initialize_container_manager(&args.socket, &args.image).await?;

    // Set up OpenAPI server with Forgejo spec
    let openapi_server = setup_openapi_server(&args.forgejo_url).await?;

    // Start a local MCP server with the OpenAPI server
    let (server_handle, server_addr) = start_local_mcp_server_with_openapi(openapi_server).await?;

    // Give the server a moment to start up
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Set up MCP client connection
    let client = setup_mcp_client(&server_addr).await?;

    println!("Cloning repository");
    // Clone repository and get diff
    clone_repository(&args.forgejo_url, repo, &container_manager).await?;

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

    // Create agent with tools
    let agent =
        create_agent_with_tools(&args, Arc::new(container_manager), tools, client.clone()).await?;

    let (_headers, timeline) = forgejo
        .issue_get_comments_and_timeline(
            owner,
            repo,
            pr_index,
            IssueGetCommentsAndTimelineQuery::default(),
        )
        .await?;

    // Create prompt with PR timeline.
    let prompt = format!(
        "The repository owner is `{}` and the repository name is `{}` and the index of the pull request under review is {}. The repository has been checked out to its default branch. Use the `repoGetPullRequest` tool to determine which branch to check out for review. The full timeline of the PR review follows: {:?}",
        owner, repo, pr_index, timeline
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
