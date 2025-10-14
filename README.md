# auditlm

A dead-simple, self-hostable code review bot with Forgejo integration. Sometimes you don't have a human to review your changes, and sometimes you don't want to give Microsoft more training data for Copilot. You can think of auditlm as an open-source and self-hosted CodeRabbit. I've using it to assist with its own development, so you can see it in action on some of the [recent PRs](https://git.lunacy.casa/hypha/auditlm/pulls/13).

## What it does

auditlm is an AI-powered code review assistant that:
- Monitors your Forgejo instance for pull requests that mention your bot
- Sets up isolated container environments for safe code analysis
- Uses local LLM models (like llama.cpp with GLM-4.5-Air) to review your code
- Posts detailed reviews directly to your pull requests

## Features

- **Self-hosted**: Run everything on your own infrastructure if you want!
- **Privacy-focused**: No data sent to third-party services unless you set it up with a cloud provider
- **Flexible**: Works with any OpenAI-compatible LLM endpoint
- **Isolated**: Each review runs in a clean container environment
- **Language-agnostic**: Analyze code in any language by providing a custom docker image

## Quick Start

### Prerequisites

- Docker or Podman for container management
- A self-hosted Forgejo instance
- An LLM endpoint (local llama.cpp setup recommended)
- Rust toolchain for building from source

### Setting up Forgejo

First, set up a Forgejo instance using Docker Compose. You can do this with [our example](examples/docker-compose.yaml) but I'd personally recommend deploying it with kubernetes instead if you plan on using it for more than just testing.

```bash
DOCKER_HOST="unix:///run/user/$(id -u)/podman/podman.sock" docker-compose up -d
```

### Configure your bot

1. Create a new user in Forgejo called `clanker` (or whatever you prefer)
2. Generate an API token for this user
3. Export the token as an environment variable so that the auditlm binary can masquerade as your bot user:

```bash
export FORGEJO_TOKEN="your-api-token-here"
```

### Set up your LLM

Configure llama.cpp with your preferred model. GLM-4.5-Air works well after applying [this patch](https://github.com/ggml-org/llama.cpp/pull/15186) to llama.cpp and using the jinja template in that PR minus the trailing `%`. Note the base URL of your LLM, I gave mine a custom port to keep 8080 free, so for me it's `http://127.0.0.1:11800`.

### Run auditlm

Clone this repository, navigate to the `auditlm` directory, and tweak the following command to your liking by modifying the docker socket and analysis image if necessary, then run it:

```bash
cargo run -- forgejo \
  --model 'GLM-4.5-Air-Q4_K_M-00001-of-00002.gguf' \
  --socket '/run/user/1000/podman/podman.sock' \
  --base-url 'http://127.0.0.1:11800' \
  --forgejo-url 'https://git.lunacy.casa' \
  --image 'rust:1-trixie'
```

auditlm will then listen for @mentions of the clanker bot in pull requests and provide a review in each case. Follow-up reviews will address changes since the last review.

## Configuration

### Docker Images

The example above uses `rust:1-trixie` for analysis environments, which won't work for all projects. Adjust the `--image` parameter based on your project's requirements.

### Network Considerations

This is a toy example. While you can keep your forgejo instance running on localhost, you should consider making it public, because if the agent tries to connect to `localhost` inside the analysis container via e.g. `curl`, that request will not be routed to your Forgejo and the agent may get confused. Most of the agent's tools will still work, however.

## Development

### Building from Source

```bash
git clone https://github.com/your-repo/auditlm.git
cd auditlm
cargo build --release
```

### Project Structure

- `auditlm/src/commands/` - Command implementations (forgejo, git)
- `auditlm/src/tools/` - AI agent tools
- `auditlm/src/container.rs` - Container management
- `auditlm/prompts/` - AI prompts for different scenarios

## License

This project is licensed under the AGPLv3 - see the [LICENSE.md](LICENSE.md) file for details.
