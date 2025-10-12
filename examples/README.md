# Forgejo example

```bash
DOCKER_HOST="unix:///run/user/$(id -u)/podman/podman.sock" docker-compose up -d
```

Create a new user called `clanker` or whatever and give it an API key, export it into the environment as `FOREGEJO_TOKEN`.

```bash
RUST_LOG=debug cargo run -- forgejo --model 'model.gguf' --socket '/run/user/1000/podman/podman.sock' --base-url 'http://127.0.0.1:11800' --forgejo-url 'http://localhost:3000/' --image 'rust:1-trixie' instanceadmin/auditlm 1
```

This is just a toy example. You should consider making your forgejo public, because if the agent tries to connect to `localhost:3000` inside the container via e.g. `curl`, that request will not be routed to your forgejo and the agent may get confused. The agent's tools will still work, however.
