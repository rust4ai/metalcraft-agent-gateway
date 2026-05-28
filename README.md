# metalcraft-agent-gateway

Platform-agnostic messaging gateway for [metalcraft-agent](https://github.com/rust4ai/metalcraft-agent).

Exposes a single HTTP API that translates into platform-specific calls (Discord, Slack, etc.).
metalcraft-agent talks to the gateway — it never needs to know which platform is behind it.

## Protocol

```
Base: $AGENT_GATEWAY_URL/api/v1
Auth: Authorization: Bearer $AGENT_GATEWAY_API_KEY

POST   /messages                           {channel_id, content, message_reference_id?, platform?}
PATCH  /messages/{message_id}              {channel_id, content, platform?}
PUT    /messages/{message_id}/reactions     {channel_id, emoji, platform?}
GET    /channels/{channel_id}/messages?limit=N&platform=X
GET    /channels/{channel_id}?platform=X
```

## Multi-platform support

The gateway can run one platform, or both simultaneously.

- **Set bot tokens** for each platform you want: `DISCORD_BOT_TOKEN`, `SLACK_BOT_TOKEN`, or both.
- **`PLATFORM` env var** sets the default used when requests omit the `platform` field.
- If only one token is configured, that platform is used automatically as the default.
- If both are configured with no `PLATFORM` set, every request must include a `platform` field.

| Platform | Env var needed | `platform` value |
|----------|---------------|-----------------|
| Discord  | `DISCORD_BOT_TOKEN` | `"discord"` |
| Slack    | `SLACK_BOT_TOKEN` | `"slack"` |

### Examples

Single platform (Discord only):
```bash
DISCORD_BOT_TOKEN=xxx  # no PLATFORM needed, auto-detected
```

Both platforms with Discord as default:
```bash
DISCORD_BOT_TOKEN=xxx
SLACK_BOT_TOKEN=xoxb-xxx
PLATFORM=discord
```

Then target Slack explicitly per-request:
```json
{"channel_id": "C12345", "content": "hello", "platform": "slack"}
```

## Running locally

```bash
cp .env.example .env   # fill in your tokens
cargo run
```

## Docker

```bash
docker build -t metalcraft-agent-gateway .
docker run --env-file .env -p 3000:3000 metalcraft-agent-gateway
```

## Deploy to Railway

1. Push this repo to GitHub
2. Create a new Railway project -> "Deploy from GitHub repo"
3. Add environment variables (`DISCORD_BOT_TOKEN` and/or `SLACK_BOT_TOKEN`, `AGENT_GATEWAY_API_KEY`, optionally `PLATFORM`)
4. Railway auto-detects the Dockerfile via `railway.toml`

## License

MIT
