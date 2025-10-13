//! Common utilities and constants for Forgejo integration

/// Default values for Forgejo configuration
pub mod defaults {
    /// Default maximum tokens for AI agent
    pub const MAX_TOKENS: u64 = 1024;

    /// Default temperature for AI agent
    pub const TEMPERATURE: f64 = 0.6;
}

/// Tool names for MCP integration
pub mod tools {
    /// Tool for getting pull request information
    pub const GET_PULL_REQUEST: &str = "repoGetPullRequest";

    /// Tool for getting pull request commits
    pub const GET_PULL_REQUEST_COMMITS: &str = "repoGetPullRequestCommits";
}

/// Client information for MCP connections
pub mod client_info {
    /// Client name
    pub const NAME: &str = "auditlm-forgejo-client";

    /// Client version
    pub const VERSION: &str = "0.1.0";

    /// Client title
    pub const TITLE: &str = "AuditLM Forgejo Client";

    /// Client website URL
    pub const WEBSITE_URL: &str = "https://github.com/auditlm/auditlm";
}
