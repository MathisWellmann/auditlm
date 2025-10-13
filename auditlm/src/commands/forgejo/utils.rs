use std::sync::Arc;

use crate::commands::forgejo::client::{
    clone_repository, initialize_container_manager, setup_openapi_server,
};
use crate::commands::forgejo::config::ForgejoConfig;
use crate::commands::forgejo::error::ForgejoError;
use crate::container::ContainerManager;

/// Resource manager for Forgejo operations
pub struct ForgejoResourceManager {
    config: ForgejoConfig,
    container_manager: Option<Arc<ContainerManager>>,
}

impl ForgejoResourceManager {
    /// Create a new resource manager
    pub fn new(config: ForgejoConfig) -> Self {
        Self {
            config,
            container_manager: None,
        }
    }

    /// Get or create the container manager
    pub async fn get_container_manager(&mut self) -> Result<Arc<ContainerManager>, ForgejoError> {
        if self.container_manager.is_none() {
            let manager =
                initialize_container_manager(&self.config.socket, &self.config.image).await?;
            self.container_manager = Some(Arc::new(manager));
        }
        Ok(self.container_manager.as_ref().unwrap().clone())
    }

    /// Clone a repository using the container manager
    pub async fn clone_repository(
        &mut self,
        owner: &str,
        repository: &str,
    ) -> Result<(), ForgejoError> {
        let container_manager = self.get_container_manager().await?;
        clone_repository(
            &self.config.forgejo_url,
            owner,
            repository,
            &container_manager,
        )
        .await
    }

    /// Set up the OpenAPI server
    pub async fn setup_openapi_server(&self) -> Result<rmcp_openapi::Server, ForgejoError> {
        setup_openapi_server(&self.config.forgejo_url).await
    }
}

/// Validate Forgejo URL format
pub fn validate_forgejo_url(url: &str) -> Result<(), ForgejoError> {
    if url.is_empty() {
        return Err(ForgejoError::Configuration(
            "Forgejo URL cannot be empty".to_string(),
        ));
    }

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ForgejoError::Configuration(
            "Forgejo URL must start with http:// or https://".to_string(),
        ));
    }

    Ok(())
}

/// Validate Docker socket path
pub fn validate_docker_socket(socket: &str) -> Result<(), ForgejoError> {
    if socket.is_empty() {
        return Err(ForgejoError::Configuration(
            "Docker socket path cannot be empty".to_string(),
        ));
    }

    // Check if the socket exists and is a socket file
    if !std::path::Path::new(socket).exists() {
        return Err(ForgejoError::Configuration(format!(
            "Docker socket does not exist: {}",
            socket
        )));
    }

    Ok(())
}
