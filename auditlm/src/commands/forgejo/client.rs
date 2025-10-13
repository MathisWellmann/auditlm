//! Forgejo API client implementation
//!
//! This module provides a high-level client for interacting with Forgejo instances,
//! including methods for searching issues, retrieving pull requests, and creating reviews.

use anyhow::Context;
use forgejo_api::Forgejo;
use forgejo_api::structs::{
    CreatePullReviewOptions, IssueGetCommentsAndTimelineQuery, IssueSearchIssuesQuery,
};
use http::HeaderMap;
use http::header::AUTHORIZATION;
use rmcp_openapi::Server;
use std::env;
use url::Url;

use crate::commands::forgejo::error::ForgejoError;
use crate::commands::forgejo::types::PrInfo;
use crate::container::ContainerManager;

/// Client for interacting with Forgejo API
///
/// This struct encapsulates the Forgejo API client and authenticated user information,
/// providing a high-level interface for common operations.
pub struct ForgejoClient {
    forgejo: Forgejo,
    authenticated_user: forgejo_api::structs::User,
}

impl ForgejoClient {
    /// Create a new Forgejo client
    ///
    /// # Arguments
    ///
    /// * `forgejo_url` - Base URL of the Forgejo instance
    /// * `token` - Authentication token for API access
    ///
    /// # Returns
    ///
    /// Returns a new `ForgejoClient` instance or an error if authentication fails.
    pub async fn new(forgejo_url: &str, token: &str) -> Result<Self, ForgejoError> {
        let forgejo = Forgejo::new(
            forgejo_api::Auth::Token(token),
            Url::parse(forgejo_url)
                .context("Failed to parse Forgejo URL")
                .map_err(|e| ForgejoError::Configuration(e.to_string()))?,
        )
        .map_err(|e| ForgejoError::Api(e))?;

        let authenticated_user = forgejo
            .user_get_current()
            .await
            .map_err(|e| ForgejoError::Api(e))?;

        Ok(Self {
            forgejo,
            authenticated_user,
        })
    }

    /// Search for mentioned issues
    ///
    /// Searches for issues that mention the authenticated user within the last 24 hours.
    ///
    /// # Returns
    ///
    /// Returns a vector of issues that mention the authenticated user, or an error
    /// if the search fails.
    pub async fn search_mentioned_issues(
        &self,
    ) -> Result<Vec<forgejo_api::structs::Issue>, ForgejoError> {
        let issues = self
            .forgejo
            .issue_search_issues(IssueSearchIssuesQuery {
                since: Some(
                    time::OffsetDateTime::now_utc().saturating_sub(time::Duration::hours(24)),
                ),
                mentioned: Some(true),
                ..Default::default()
            })
            .await
            .map_err(|e| ForgejoError::Api(e))?;

        Ok(issues.1)
    }

    /// Get issue comments and timeline
    pub async fn get_issue_timeline(
        &self,
        owner: &str,
        repo: &str,
        issue_index: u64,
    ) -> Result<(HeaderMap, Vec<serde_json::Value>), ForgejoError> {
        let (_headers, issue_comments) = self
            .forgejo
            .issue_get_comments_and_timeline(
                owner,
                repo,
                issue_index,
                IssueGetCommentsAndTimelineQuery::default(),
            )
            .await
            .map_err(|e| ForgejoError::Api(e))?;

        let timeline_values: Vec<serde_json::Value> = issue_comments
            .into_iter()
            .map(|comment| {
                serde_json::to_value(comment).unwrap_or_else(|_| serde_json::Value::Null)
            })
            .collect();

        // Create a new HeaderMap (simplified approach)
        let header_map = HeaderMap::new();

        Ok((header_map, timeline_values))
    }

    /// Get pull request information
    pub async fn get_pull_request(
        &self,
        owner: &str,
        repo: &str,
        pr_index: u64,
    ) -> Result<PrInfo, ForgejoError> {
        let (_headers, timeline) = self.get_issue_timeline(owner, repo, pr_index).await?;

        let pull_meta = self
            .forgejo
            .repo_get_pull_request(owner, repo, pr_index)
            .await
            .map_err(|e| ForgejoError::Api(e))?;

        let diff = if let Some(diff_url) = pull_meta.diff_url {
            reqwest::get(diff_url)
                .await
                .map_err(|e| ForgejoError::Repository(format!("Failed to fetch diff URL: {}", e)))?
                .bytes()
                .await
                .map_err(|e| ForgejoError::Repository(format!("Failed to read diff bytes: {}", e)))
                .map(|bytes| String::from_utf8_lossy(&bytes).to_string())?
        } else {
            "[failed to fetch diff]".to_string()
        };

        Ok(PrInfo {
            owner: owner.to_string(),
            repo: repo.to_string(),
            index: pr_index,
            diff,
            timeline: timeline
                .into_iter()
                .map(|t| serde_json::to_value(t).unwrap())
                .collect(),
        })
    }

    /// Create a pull request review
    pub async fn create_pull_review(
        &self,
        owner: &str,
        repo: &str,
        pr_index: u64,
        review_body: String,
    ) -> Result<(), ForgejoError> {
        self.forgejo
            .repo_create_pull_review(
                owner,
                repo,
                pr_index,
                CreatePullReviewOptions {
                    body: Some(review_body),
                    comments: Some(vec![]),
                    commit_id: None,
                    event: Some("COMMENT".to_string()),
                },
            )
            .await
            .map_err(|e| ForgejoError::Api(e))?;

        Ok(())
    }

    /// Check if we should comment on an issue based on recent comments
    pub async fn should_comment_on_issue(
        &self,
        owner: &str,
        repository: &str,
        issue_index: u64,
    ) -> Result<bool, ForgejoError> {
        let (_headers, issue_comments) = self
            .get_issue_timeline(owner, repository, issue_index)
            .await?;

        let mut most_recent_mention: Option<usize> = None;
        let mut most_recent_own_comment: Option<usize> = None;

        for (index, comment) in issue_comments.iter().enumerate() {
            // Parse the comment as a timeline item to extract user and body
            if let Some(user) = comment
                .get("user")
                .and_then(|u| u.get("login"))
                .and_then(|l| l.as_str())
            {
                if Some(user.to_string()) == self.authenticated_user.login.clone() {
                    most_recent_own_comment = Some(index);
                }
            }

            if let Some(body) = comment.get("body").and_then(|b| b.as_str()) {
                if body.contains(&format!(
                    "@{}",
                    self.authenticated_user.login.clone().unwrap_or_default()
                )) {
                    most_recent_mention = Some(index);
                }
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
}

/// Clone a repository into a container
pub async fn clone_repository(
    forgejo_url: &str,
    owner: &str,
    repository: &str,
    container_manager: &ContainerManager,
) -> Result<(), ForgejoError> {
    let repo_url = format!("{}/{}/{}.git", forgejo_url, owner, repository);
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
        .map_err(|e| {
            ForgejoError::Repository(e.context("Failed to clone repository").to_string())
        })?;

    println!("Clone result: {}", clone_result);
    Ok(())
}

/// Initialize container manager and create analysis container
pub async fn initialize_container_manager(
    socket: &str,
    image: &str,
) -> Result<ContainerManager, ForgejoError> {
    let mut container_manager = ContainerManager::new(socket)
        .await
        .map_err(|e| ForgejoError::Container(e))?;

    container_manager
        .create_analysis_container(image)
        .await
        .map_err(|e| ForgejoError::Container(e))?;

    Ok(container_manager)
}

/// Set up OpenAPI server with Forgejo spec
pub async fn setup_openapi_server(forgejo_url: &str) -> Result<rmcp_openapi::Server, ForgejoError> {
    // Download Forgejo OpenAPI spec
    let openapi_spec = serde_json::from_str(include_str!("../../../api/forgejo.v1.json"))
        .map_err(|e| ForgejoError::OpenApiServer(format!("Failed to parse OpenAPI spec: {}", e)))?;

    // Create OpenAPI server with Forgejo spec
    let forgejo_base_url = Url::parse(forgejo_url)
        .context("Failed to parse Forgejo URL")
        .map_err(|e| ForgejoError::OpenApiServer(e.to_string()))?;

    let forgejo_token = env::var("FORGEJO_TOKEN").map_err(|_| {
        ForgejoError::Configuration("FORGEJO_TOKEN environment variable not set".to_string())
    })?;

    // Create authorization header
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        format!("token {}", forgejo_token)
            .parse()
            .context("Failed to parse authorization header")
            .map_err(|e| ForgejoError::OpenApiServer(e.to_string()))?,
    );

    let mut openapi_server = Server::builder()
        .openapi_spec(openapi_spec)
        .base_url(forgejo_base_url)
        .default_headers(headers)
        .build();

    // Load tools from the OpenAPI spec
    openapi_server
        .load_openapi_spec()
        .context("Failed to load OpenAPI tools")
        .map_err(|e| ForgejoError::OpenApiServer(e.to_string()))?;

    println!("Loaded {} OpenAPI tools", openapi_server.tool_count());

    // Extract tool names from the OpenAPI server
    let openapi_tool_names = openapi_server.get_tool_names();
    println!("OpenAPI tool names: {:?}", openapi_tool_names);

    Ok(openapi_server)
}
