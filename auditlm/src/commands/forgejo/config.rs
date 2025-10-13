use crate::commands::forgejo::error::ForgejoError;
use crate::commands::forgejo::utils::{validate_docker_socket, validate_forgejo_url};
use std::env;

/// Configuration for Forgejo operations
#[derive(Debug, Clone)]
pub struct ForgejoConfig {
    pub forgejo_token: String,
    pub forgejo_url: String,
    pub model: String,
    pub base_url: String,
    pub socket: String,
    pub image: String,
    pub interval: u64,
}

impl ForgejoConfig {
    /// Create a new configuration from command-line arguments
    pub fn from_args(
        args: &crate::commands::forgejo::types::ForgejoArgs,
    ) -> Result<Self, ForgejoError> {
        let forgejo_token = env::var("FORGEJO_TOKEN").map_err(|_| {
            ForgejoError::Configuration("FORGEJO_TOKEN environment variable not set".to_string())
        })?;

        Ok(Self {
            forgejo_token,
            forgejo_url: args.forgejo_url.clone(),
            model: args.model.clone(),
            base_url: args.base_url.clone(),
            socket: args.socket.clone(),
            image: args.image.clone(),
            interval: args.interval,
        })
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), ForgejoError> {
        // Validate Forgejo URL
        validate_forgejo_url(&self.forgejo_url)?;

        // Validate model name
        if self.model.is_empty() {
            return Err(ForgejoError::Configuration(
                "Model name cannot be empty".to_string(),
            ));
        }

        // Validate base URL
        if self.base_url.is_empty() {
            return Err(ForgejoError::Configuration(
                "Base URL cannot be empty".to_string(),
            ));
        }

        if !self.base_url.starts_with("http://") && !self.base_url.starts_with("https://") {
            return Err(ForgejoError::Configuration(
                "Base URL must start with http:// or https://".to_string(),
            ));
        }

        // Validate Docker socket
        validate_docker_socket(&self.socket)?;

        // Validate Docker image
        if self.image.is_empty() {
            return Err(ForgejoError::Configuration(
                "Docker image cannot be empty".to_string(),
            ));
        }

        // Validate interval
        if self.interval == 0 {
            return Err(ForgejoError::Configuration(
                "Interval must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }
}
