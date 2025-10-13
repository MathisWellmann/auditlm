use thiserror::Error;

/// Errors encountered in Forgejo operations
#[derive(Error, Debug)]
pub enum ForgejoError {
    #[error("Forgejo API error: {0}")]
    Api(#[from] forgejo_api::ForgejoError),
    #[error("Container operation failed: {0}")]
    Container(#[from] anyhow::Error),
    #[error("MCP connection error: {0}")]
    McpConnection(String),
    #[error("OpenAPI server error: {0}")]
    OpenApiServer(String),
    #[error("Agent operation failed: {0}")]
    Agent(String),
    #[error("Repository operation failed: {0}")]
    Repository(String),
    #[error("Configuration error: {0}")]
    Configuration(String),
    #[error("Transport error: {0}")]
    Transport(String),
}

impl ForgejoError {
    /// Check if this is a non-fatal error that can be retried
    pub fn is_non_fatal(&self) -> bool {
        match self {
            ForgejoError::Api(_) | ForgejoError::Container(_) => true,
            _ => false,
        }
    }
}
