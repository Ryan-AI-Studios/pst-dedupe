# Ledgerful MCP Server

Ledgerful provides a Model Context Protocol (MCP) server that exposes its intelligence as read-only tools for AI coding agents.

## Registration

### Claude Code
Run this command in your terminal:
```bash
mcp add ledgerful cargo run --manifest-path C:/dev/ledgerful/Cargo.toml -- mcp
```

### Cursor
Add to `.cursor/mcp.json` or Global Settings:
```json
{
 "mcpServers": {
  "ledgerful": {
   "command": "cargo",
   "args": ["run", "--manifest-path", "C:/dev/ledgerful/Cargo.toml", "--features", "mcp", "--", "mcp"]
  }
 }
}
```

### Windsurf
Add to your `mcp.json`:
```json
{
 "mcpServers": {
  "ledgerful": {
   "command": "cargo",
   "args": ["run", "--manifest-path", "C:/dev/ledgerful/Cargo.toml", "--features", "mcp", "--", "mcp"]
  }
 }
}
```

### Cline
Add to `.cline/mcp.json`:
```json
{
 "mcpServers": {
  "ledgerful": {
   "command": "cargo",
   "args": ["run", "--manifest-path", "C:/dev/ledgerful/Cargo.toml", "--features", "mcp", "--", "mcp"]
  }
 }
}
```

### Continue
Add to your config:
```json
{
 "mcpServers": {
  "ledgerful": {
   "command": "cargo",
   "args": ["run", "--manifest-path", "C:/dev/ledgerful/Cargo.toml", "--features", "mcp", "--", "mcp"]
  }
 }
}
```

### Aider
Run aider with:
```bash
aider --mcp-server "cargo run --manifest-path C:/dev/ledgerful/Cargo.toml -- mcp"
```

## Tools

1. `scan`: Run impact scan on current repo.
2. `search`: BM25/regex code search.
3. `ask`: Semantic Q&A with context assembly.
4. `ledger_status`: Current pending/unaudited state.
5. `ledger_search`: Full-text search transactions.
6. `hotspots`: Current hotspot rankings.
7. `endpoints_changed`: API endpoints affected by current diff.
8. `security_boundaries`: Security policy graph summary.
9. `dead_code`: Confidence-ranked dead code candidates in the repo.
10. `verify_plan`: Predicted test list for the current diff, without running tests.

## Known Limitations
- No streaming.
- No mutations (read-only v1).
- No streaming or mutations.

## Troubleshooting
- **agent can't find ledgerful on PATH**: Install with `cargo install ledgerful --locked` or use `cargo run -- mcp`.
