# Fixture Shapes — confirmed 2026-06-18

## Hook fixtures (`hook_*.json`)

Source: `~/.claude-events/claude-events.db` table `hook_events`, columns reconstructed
into `HookRelayMessage` camelCase format (the relay publishes this shape to `hook.<event>`).

Key JSON paths:
- Session ID: `.sessionId` (top-level string, UUID)
- Workspace: `.cwd` (top-level string)
- Event type: `.hookEvent` (e.g. `"SessionStart"`, `"PreToolUse"`, `"Stop"`)
- Tool name: `.toolName` (top-level string, null for non-tool events)
- Timestamp: `.createdAt` (epoch milliseconds integer)
- Agent ID: `.agentId` (top-level string, null for non-agent events)
- Agent type: `.agentType` (top-level string, null for non-agent events)

Redacted fields (keys kept, values blanked): `lastAssistantMessage`, `rawPayload`, `message`.
Model is NOT present in hook events; it lives only in transcript lines.

## Transcript fixture (`transcript_assistant.jsonl`)

Source: on-disk JSONL from `~/.claude/projects/*/<session>.jsonl`
(a real `transcript.message` RabbitMQ body would have the same shape — the pipeline
publishes each JSONL line verbatim; wire shape still needs confirmation at live run).

Key JSON paths:
- Session ID: `.sessionId` (top-level string, UUID)
- Model: `.message.model` (nested under `message` object, e.g. `"claude-sonnet-4-6"`)
- Role confirmation: `.message.role == "assistant"` and `.type == "assistant"`

Redacted fields: `.message.content[*].thinking`, `.message.content[*].signature`
(content text blanked to `"REDACTED"`; structural keys kept).
