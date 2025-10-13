use std::time::Duration;

use crate::commands::forgejo::client::ForgejoClient;
use crate::commands::forgejo::config::ForgejoConfig;
use crate::commands::forgejo::error::ForgejoError;
use crate::commands::forgejo::review::process_pr_review_with_resources;
use crate::commands::forgejo::types::ForgejoArgs;
use crate::commands::forgejo::utils::ForgejoResourceManager;

/// Main daemon function for Forgejo integration
pub async fn forgejo_daemon(args: ForgejoArgs) -> anyhow::Result<()> {
    // Create and validate configuration
    let config = ForgejoConfig::from_args(&args)
        .map_err(|e| anyhow::anyhow!("Configuration error: {}", e))?;
    config
        .validate()
        .map_err(|e| anyhow::anyhow!("Configuration validation failed: {}", e))?;

    let forgejo_client = ForgejoClient::new(&config.forgejo_url, &config.forgejo_token)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create Forgejo client: {}", e))?;

    // Loop forever finding and processing mentioned issues
    loop {
        match process_mentioned_issues(&args, &forgejo_client).await {
            Ok(()) => {
                // Successfully processed issues
            }
            Err(e) if e.is_non_fatal() => {
                tracing::error!("Non-fatal error processing mentioned issues: {}", e);
            }
            Err(e) => {
                tracing::error!("Error processing mentioned issues: {}", e);
            }
        }

        tokio::time::sleep(Duration::from_secs(config.interval)).await
    }
}

/// Process mentioned issues and queue appropriate tasks
///
/// This function searches for issues that mention the authenticated user
/// and processes them accordingly. For pull requests, it triggers a review
/// process. For regular issues, it logs a message (unimplemented).
///
/// # Arguments
///
/// * `args` - Command-line arguments containing configuration
/// * `forgejo_client` - Authenticated Forgejo client
///
/// # Returns
///
/// Returns `Ok(())` if processing completes successfully, or an error
/// if any step fails.
async fn process_mentioned_issues(
    args: &ForgejoArgs,
    forgejo_client: &ForgejoClient,
) -> Result<(), ForgejoError> {
    let issues = forgejo_client.search_mentioned_issues().await?;

    for issue in issues {
        let id = match issue.id {
            Some(id) => id,
            None => {
                tracing::error!("Found issue without ID: {:?}", issue);
                continue;
            }
        };

        // Extract repository information using helper function
        let (owner, repository) = match crate::commands::forgejo::extract_repo_info(&issue) {
            Some(info) => info,
            None => {
                tracing::error!("Failed to extract repository info from issue: {:?}", issue);
                continue;
            }
        };

        // Check if we should comment on this issue
        if !forgejo_client
            .should_comment_on_issue(&owner, &repository, id as u64)
            .await?
        {
            continue;
        }

        // Process based on issue type
        if crate::commands::forgejo::is_pull_request(&issue) {
            if let Err(e) =
                review_forgejo_pr(&args, forgejo_client, &owner, &repository, id as u64).await
            {
                tracing::error!("Error while reviewing PR: {}", e);
            }
        } else {
            tracing::info!(
                "Got a task to look into issue {owner}/{repository}#{id} (unimplemented)"
            )
        }
    }

    Ok(())
}

/// Review a Forgejo pull request
///
/// This function performs a comprehensive review of a pull request by:
/// 1. Setting up a containerized environment
/// 2. Cloning the repository
/// 3. Fetching the PR diff and timeline
/// 4. Using an AI agent to analyze the changes
/// 5. Posting a review comment to the PR
///
/// # Arguments
///
/// * `args` - Command-line arguments containing configuration
/// * `forgejo_client` - Authenticated Forgejo client
/// * `owner` - Repository owner
/// * `repo` - Repository name
/// * `pr_index` - Pull request index/number
///
/// # Returns
///
/// Returns `Ok(())` if the review is completed successfully, or an error
/// if any step fails.
pub async fn review_forgejo_pr(
    args: &ForgejoArgs,
    forgejo_client: &ForgejoClient,
    owner: &str,
    repo: &str,
    pr_index: u64,
) -> Result<(), ForgejoError> {
    // Create a review context
    let context = crate::commands::forgejo::PrReviewContext::new(
        owner.to_string(),
        repo.to_string(),
        pr_index,
    );
    let config = ForgejoConfig::from_args(args)?;

    let mut resource_manager = ForgejoResourceManager::new(config);

    process_pr_review_with_resources(args, forgejo_client, &context, &mut resource_manager).await?;

    Ok(())
}
