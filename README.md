# auditlm

This is a dead-simple self-hostable code review bot. Sometimes you don't have a human to review your change for whatever reason, and sometimes you don't want to give Microsoft more training data for Copilot.

No real docs currently, but it seems to mostly work. I'd like to integrate this with forgejo/gitlab/whatever at some point.

## Usage:

```
cargo run -- --model 'model.gguf' --socket '/run/user/1000/podman/podman.sock' --base-url 'http://127.0.0.1:11800' --repo-url 'https://github.com/ellenhp/auditlm' --head 53b29c6091f5d6598a057b8af51aab8436162e0c --base 7322b9f1de4591e2ebc3277d257487ac59195906 --image 'rust:1-trixie'
```

### Required Arguments:
- `--model`: The model to use for code review
- `--socket`: Path to the Docker/Podman socket
- `--base-url`: The base URL for the OpenAI-compatible API endpoint
- `--repo-url`: URL of the Git repository to analyze
- `--base`: The base Git reference to compare against
- `--image`: Docker image to use for analysis (e.g., "rust:1-trixie", "ubuntu:22.04")

### Optional Arguments:
- `--api-key`: API key for authentication (optional, uses empty string for local models)
- `--head`: Specific commit hash or branch to checkout after cloning

# Example output

<details>
<summary>`git diff` output</summary>

```diff
diff --git a/src/main.rs b/src/main.rs
index 72c71f4..a095c67 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,20 +10,34 @@ use rig::{agent::PromptRequest, client::CompletionClient};
 
 use crate::{container::ContainerManager, tools::execute::ExecuteCommandTool};
 
+/// Command-line arguments for the auditlm tool
 #[derive(Debug, Parser)]
 struct Cli {
+    /// The OpenAI model to use for code review (e.g., "gpt-4", "gpt-3.5-turbo")
     #[arg(long)]
     model: String,
+
+    /// The base URL for the OpenAI-compatible API endpoint
     #[arg(long)]
     base_url: String,
+
+    /// Optional API key for authentication to a cloud provider. If not provided, an empty string will be used for e.g. a local llama.cpp service.
     #[arg(long)]
-    api_key: String,
+    api_key: Option<String>,
+
+    /// Path to the Docker socket for container management
     #[arg(long)]
     socket: String,
+
+    /// URL of the Git repository to clone and analyze
     #[arg(long)]
     repo_url: String,
+
+    /// Optional specific commit hash or branch to checkout after cloning
     #[arg(long)]
     head: Option<String>,
+
+    /// The base Git reference to compare against when generating the diff (e.g., branch name, commit hash)
     #[arg(long)]
     base: String,
 }
@@ -63,7 +77,7 @@ async fn main() -> anyhow::Result<()> {
 
     let container_manager = Arc::new(container_manager);
 
-    let openai_client = openai::Client::builder(&args.api_key)
+    let openai_client = openai::Client::builder(&args.api_key.unwrap_or(String::new()))
         .base_url(&args.base_url)
         .build()?;
 
```
</details>

## Code Review

This change makes the `api_key` field in the CLI struct optional, which is a good improvement for flexibility. Here's my analysis:

### Changes Made:
1. **API Key Optional**: Changed `api_key: String` to `api_key: Option<String>`
2. **Documentation Added**: Added comprehensive doc comments to all CLI fields
3. **Client Builder Update**: Updated `openai::Client::builder()` call to unwrap the optional API key with a fallback to empty string

### Positive Aspects:
- **Flexibility**: Users can now run the tool without providing an API key, which is essential for self-hosted models like llama.cpp
- **Better UX**: Clear documentation helps users understand each parameter
- **Backward Compatibility**: The change maintains functionality for users who do provide an API key

### Minor Suggestion:
Consider adding a comment explaining why `String::new()` is used instead of a more descriptive default when no API key is provided, as this affects different providers differently.

### Overall Assessment:
This is a well-considered improvement that enhances the tool's usability for different deployment scenarios while maintaining robustness. The documentation addition significantly improves the developer experience.

