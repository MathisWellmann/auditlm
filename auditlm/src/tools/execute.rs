use std::sync::Arc;

use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::json;

use crate::container::ContainerManager;

#[derive(Deserialize)]
pub(crate) struct ExecuteCommandArgs {
    command: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum ExecuteCommandToolError {
    #[error("Command Failed")]
    CommandFailed { stderr: String },
}

pub(crate) struct ExecuteCommandTool {
    container: Arc<ContainerManager>,
}
impl ExecuteCommandTool {
    pub(crate) fn with_container(container: Arc<ContainerManager>) -> ExecuteCommandTool {
        ExecuteCommandTool { container }
    }
}

impl Tool for ExecuteCommandTool {
    const NAME: &'static str = "Execute command";
    type Error = ExecuteCommandToolError;
    type Args = ExecuteCommandArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Execute a command in the code review sandbox".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The command to run, along with any arguments" },
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let command = vec!["sh".to_string(), "-c".to_string(), args.command];
        let result = self
            .container
            .execute_command(&command)
            .await
            .map_err(|err| ExecuteCommandToolError::CommandFailed {
                stderr: err.to_string(),
            })?;
        println!("Called {:?} and got:\n{}", command, result);
        Ok(result)
    }

    fn name(&self) -> String {
        Self::NAME.to_string()
    }
}
