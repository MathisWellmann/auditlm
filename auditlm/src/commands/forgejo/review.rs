//! Pull request review processing
//!
//! This module provides functionality for processing pull request reviews,
//! including extracting repository information and generating review prompts.

use crate::commands::forgejo::client::ForgejoClient;
use crate::commands::forgejo::error::ForgejoError;
use crate::commands::forgejo::server::process_pr_review;
use crate::commands::forgejo::types::ForgejoArgs;
use crate::commands::forgejo::utils::ForgejoResourceManager;

pub struct PrReviewContext {
    pub owner: String,
    pub repo: String,
    pub pr_index: u64,
}

impl PrReviewContext {
    /// Create a new PR review context
    pub fn new(owner: String, repo: String, pr_index: u64) -> Self {
        Self {
            owner,
            repo,
            pr_index,
        }
    }
}

/// Process a pull request review with resource management
pub async fn process_pr_review_with_resources(
    args: &ForgejoArgs,
    forgejo_client: &ForgejoClient,
    context: &PrReviewContext,
    resource_manager: &mut ForgejoResourceManager,
) -> Result<(), ForgejoError> {
    // Clone the repository
    resource_manager
        .clone_repository(&context.owner, &context.repo)
        .await?;

    // Get pull request information
    let pr_info = forgejo_client
        .get_pull_request(&context.owner, &context.repo, context.pr_index)
        .await?;

    // Set up OpenAPI server
    let openapi_server = resource_manager.setup_openapi_server().await?;

    // Start MCP server
    let (server_handle, server_addr) =
        crate::commands::forgejo::server::start_local_mcp_server_with_openapi(openapi_server)
            .await?;

    // Give the server a moment to start up
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Set up MCP client
    let client = crate::commands::forgejo::server::setup_mcp_client(&server_addr).await?;

    // Get container manager
    let container_manager = resource_manager.get_container_manager().await?;

    // Process the review
    let review_response = process_pr_review(args, &pr_info, container_manager, client).await?;

    // Create the review on Forgejo
    forgejo_client
        .create_pull_review(
            &context.owner,
            &context.repo,
            context.pr_index,
            review_response,
        )
        .await?;

    // Clean up the server
    server_handle.abort();
    let _ = server_handle.await;

    Ok(())
}

/// Extract repository information from an issue
pub fn extract_repo_info(issue: &forgejo_api::structs::Issue) -> Option<(String, String)> {
    let repo_meta = issue.repository.as_ref()?;
    let owner = repo_meta.owner.as_ref()?.clone();
    let repository = repo_meta.name.as_ref()?.clone();
    Some((owner, repository))
}

/// Check if an issue is a pull request
pub fn is_pull_request(issue: &forgejo_api::structs::Issue) -> bool {
    issue.pull_request.is_some()
}
