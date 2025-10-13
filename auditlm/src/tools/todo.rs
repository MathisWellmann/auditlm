use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub(crate) struct TodoListArgs {
    tasks: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum TodoListToolError {}

pub(crate) struct TodoListTool {}

impl TodoListTool {
    pub(crate) fn new() -> TodoListTool {
        TodoListTool {}
    }
}

impl Tool for TodoListTool {
    const NAME: &'static str = "Update todo list";
    type Error = TodoListToolError;
    type Args = TodoListArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: "Update and track the todo list for code review tasks".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "tasks": {
                        "type": "string",
                        "description": "The current todo list or task updates to track progress"
                    },
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Simply echo the input to keep it in context
        // This helps maintain task state between prompts since rig-core keeps tool output in context
        println!("Todo list updated: {}", args.tasks);
        Ok(format!("Todo list: {}", args.tasks))
    }

    fn name(&self) -> String {
        Self::NAME.to_string()
    }
}
