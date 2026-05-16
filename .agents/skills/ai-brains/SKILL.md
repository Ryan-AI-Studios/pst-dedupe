---
name: ai-brains
description: Persistent memory and project context vault. Use this skill whenever the user asks 'what did we decide', mentions past sessions, or when starting work on a repo cold. Trigger when you hear 'remember this', 'don't forget', 'check the vault', or 'what did we decide about'. ALSO trigger on frustration signals like 'I told you last time' or 'we already tried that'. Use even if memory isn't explicitly mentioned if the task involves project history. DO NOT use for generic coding questions, library documentation, or formatting help.
---

# AI-Brains Memory Protocol

This skill provides access to the long-term memory vault. Use it to avoid repeating work and to stay aligned with established architectural decisions.

## When NOT to use this skill
- **Generic Knowledge**: Do not use for general "How to" questions (e.g., "How do I use unwrap in Rust?").
- **Trivial Edits**: Do not use for one-off formatting or simple syntax fixes.
- **Immediate Context**: Do not use if the answer is already visible in the current session's immediate conversation history.

## Availability & Fallback
This skill requires the `ai-brains` CLI tool.
1. **Check**: Run `ai-brains --help`. If it prints usage info, the CLI is available.
2. **Fallback**: If the CLI is not found, inform the user that ai-brains needs to be installed. Proceed with manual context gathering (README, Cargo.toml, entry points) and do not attempt further vault commands.

## Workflow Phases

### Phase 1: Orient (What do I already know?)
Trigger when starting a new session or entering a new repository.
1. **Sync Safety**: Run `ai-brains safety sync`.
   - **Goal**: Ingest recent ChangeGuard hotspots to identify brittle files.
2. **Get Orientation**: Run `ai-brains preflight --max-words 1000`.
   - **Goal**: Identify the most recent project state and safety constraints.
- **Heuristic**: Keep any additional manual research notes under ~150 words to ensure the memory index remains dominant in your context.
- **Missing Bearings**: If a core bearing (README, CI config) is missing, record it as a constraint in Phase 3.

### Phase 2: Recall (Search before acting)
Trigger before starting a development track, architectural change, or when an unfamiliar constant/path is encountered.
Run: `ai-brains recall "<topic>"`
- **Goal**: Find project-specific constraints or rejected approaches.
- **Context**: This command traverses a relational graph. Look for 'CONFLICTS_WITH' or 'INVARIANT' tags in the results.

### Phase 3: Record (Persist after deciding)
Trigger immediately after a major decision, discovery of a critical constraint, or user correction.
Run: `ai-brains pin "DECISION: <content>"`
- **Goal**: Pin "Dense" knowledge (decisions, invariants, constraints).
- **Format**: Use the format `DECISION: ...`, `CONSTRAINT: ...`, or `INVARIANT: ...`.
- **Role Selection**: Use the default (assistant) when recording your own reasoning. Use `--role user` when recording a direct correction or instruction from the user.

## Maintenance
For batch reconciliation across sessions and to update the relational graph, run:
`ai-brains nightly`
**Important**: `nightly` is a heavy batch operation that summarizes sessions and rebuilds the relational graph. Do NOT run it as a substitute for other commands. Only use it when the vault feels stale or a significant number of sessions have accumulated.

## Command Summary

| Action | Command |
|---|---|
| Initialize Context | `ai-brains context` |
| Sync Safety Signals | `ai-brains safety sync` |
| Get Orientation | `ai-brains preflight` |
| Deep Search | `ai-brains recall` |
| Pinned Record | `ai-brains pin` |
| Ingest Turn | `ai-brains ingest` (reads JSON from stdin — not for interactive use) |
| Nightly Audit | `ai-brains nightly` (heavy batch op — do NOT substitute for other commands) |
| Forget Memory | `ai-brains forget` |
| Backup Vault | `ai-brains backup` |
