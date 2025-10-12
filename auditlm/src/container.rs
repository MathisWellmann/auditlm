use anyhow::{Context, Result, bail};
use bollard::{
    API_DEFAULT_VERSION, Docker,
    container::{Config, CreateContainerOptions, LogOutput, StartContainerOptions},
    image::CreateImageOptions,
    models::ContainerCreateResponse,
};
use futures::StreamExt;
use tempfile::TempDir;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Container manager for setting up and managing analysis environments
pub struct ContainerManager {
    docker: Docker,
    container_id: Option<String>,
    temp_dir: Option<TempDir>,
}

impl ContainerManager {
    /// Create a new container manager
    pub async fn new(socket: &str) -> Result<Self> {
        let docker = Docker::connect_with_socket(socket, 1000, API_DEFAULT_VERSION)
            .context("Failed to connect to Docker daemon")?;

        info!("Connected to Docker daemon");

        Ok(Self {
            docker,
            container_id: None,
            temp_dir: None,
        })
    }

    /// Create a new analysis container with comprehensive tools
    pub async fn create_analysis_container(&mut self, image: &str) -> Result<()> {
        // Create a temporary directory for mounting
        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path();

        // Generate a unique container name
        let container_name = format!("auditlm-analysis-{}", Uuid::new_v4());

        info!("Creating analysis container: {}", container_name);

        // Pull the base image if it doesn't exist
        self.pull_image(image).await?;

        // Create container configuration
        let create_options = Some(CreateContainerOptions {
            name: container_name.clone(),
            ..Default::default()
        });

        let mut binds = Vec::new();
        if let Some(host_path) = temp_path.to_str() {
            binds.push(format!("{}:/workspace:z", host_path));
        }

        let config = Config {
            image: Some(image),
            host_config: Some(bollard::models::HostConfig {
                binds: Some(binds),
                auto_remove: Some(true),
                ..Default::default()
            }),
            working_dir: Some("/workspace"),
            cmd: Some(vec!["sleep", "infinity"]),
            tty: Some(true),
            ..Default::default()
        };

        // Create the container
        let container: ContainerCreateResponse = self
            .docker
            .create_container(create_options, config)
            .await
            .context("Failed to create container")?;

        let container_id = container.id;

        // Start the container
        let start_options: Option<StartContainerOptions<String>> = None;
        self.docker
            .start_container(&container_id, start_options)
            .await
            .context("Failed to start container")?;

        // Store container info
        self.container_id = Some(container_id.clone());
        self.temp_dir = Some(temp_dir);

        info!("Analysis container created and started: {}", container_id);

        Ok(())
    }

    /// Pull an image if it doesn't exist
    async fn pull_image(&self, image_name: &str) -> Result<()> {
        info!("Pulling image: {}", image_name);

        let create_options = Some(CreateImageOptions {
            from_image: image_name,
            ..Default::default()
        });

        let mut output = self.docker.create_image(create_options, None, None);

        while let Some(result) = output.next().await {
            match result {
                Ok(info) => {
                    if let Some(id) = info.id {
                        debug!("Image layer: {}", id);
                    }
                }
                Err(e) => {
                    error!("Error pulling image: {}", e);
                    return Err(e).context("Failed to pull image");
                }
            }
        }

        info!("Image pulled successfully: {}", image_name);
        Ok(())
    }

    /// Execute a command in the container
    pub async fn execute_command(&self, command: &[String]) -> Result<String> {
        let container_id = if let Some(container) = self.container_id() {
            container
        } else {
            bail!("Container not started");
        };
        debug!(
            "Executing command in container {}: {:?}",
            container_id, command
        );

        let exec_config = bollard::exec::CreateExecOptions {
            cmd: Some(command.iter().map(|s| s.to_string()).collect()),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };

        let exec = self
            .docker
            .create_exec(container_id, exec_config)
            .await
            .context("Failed to create exec")?;

        let exec_id = exec.id;

        // Start the exec
        let start_exec_options: Option<bollard::exec::StartExecOptions> = None;
        let output = self.docker.start_exec(&exec_id, start_exec_options);

        let mut combined = Vec::new();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        match output.await {
            Ok(bollard::exec::StartExecResults::Attached { mut output, .. }) => {
                while let Some(Ok(result)) = output.next().await {
                    match result {
                        LogOutput::StdOut { message } => {
                            combined.extend_from_slice(&message);
                            stdout.extend_from_slice(&message);
                        }
                        LogOutput::StdErr { message } => {
                            combined.extend_from_slice(&message);
                            stderr.extend_from_slice(&message);
                        }
                        _ => {}
                    }
                }
            }
            Ok(bollard::exec::StartExecResults::Detached) => {
                debug!("Command detached");
            }
            Err(e) => {
                error!("Error executing command: {}", e);
                return Err(e).context("Failed to execute command");
            }
        }

        let output = String::from_utf8_lossy(&combined);
        let error_output = String::from_utf8_lossy(&stderr);

        if !error_output.is_empty() {
            debug!("Command stderr: {}", error_output);
        }

        Ok(output.to_string())
    }

    /// Get the container ID
    pub fn container_id(&self) -> Option<&str> {
        self.container_id.as_deref()
    }
}
