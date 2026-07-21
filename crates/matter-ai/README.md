# matter-ai

Opt-in **AI provider trait** + **first-pass code suggestions** for Dedupe Desk (track **0051**).

## Architecture locks

| Rule | Detail |
|---|---|
| AI off by default | Core product works without AI; jobs fail closed when disabled |
| Trait + Mock + OpenAI-compatible | One HTTP shape for local (Ollama / LM Studio) and cloud |
| Remote requires `allow_remote` | No silent cloud fallback |
| Suggestions ≠ final codes | Job writes `item_ai_suggestions` only; human accept → `item_codes` |
| Keys not in SQLite | OS keyring (Desk) + env `PST_DEDUPE_AI_API_KEY` (headless) |
| Full catalog guidance | Prompt embeds operator definitions — no name-only inventing |
| Middle-drop truncation | Head + tail kept when text exceeds cap |
| Skip withheld | Privilege withhold items never sent to the model |
| Mock-only CI | Default tests never hit the network |

## Provider kinds

| Kind | Use |
|---|---|
| `none` | Disabled (default) |
| `mock` | Deterministic JSON for tests / offline demos |
| `openai_compatible` | `POST {base}/v1/chat/completions` |

Loopback hosts (`127.0.0.1`, `localhost`, `::1`) are **local**. Any other host is **remote** and requires `ai_allow_remote = 1`.

**Redirects are not followed** on OpenAI-compatible HTTP calls (`reqwest` redirect policy `none`). A 3xx response is treated as an error so a loopback base URL cannot silently hop to a remote host when `allow_remote` is false.

### Local servers (operator-installed)

| Server | Typical base URL |
|---|---|
| Ollama | `http://127.0.0.1:11434/v1` |
| LM Studio | `http://127.0.0.1:1234/v1` |

Desk does **not** ship or auto-start these daemons.

## Secrets

Resolution order:

1. Env **`PST_DEDUPE_AI_API_KEY`** (headless CLI, CI, services)
2. OS keyring service `dedupe-desk` / user `ai_api_key` (interactive Desk)

Never store API keys in `matter.db` or audit content. Keyring failures fail closed with a clear error (no hang).

## Job `ai_suggest_codes`

```json
{
  "scope": "in_review",
  "max_items": 100,
  "max_text_bytes": 8000,
  "reset": false,
  "temperature": 0.0
}
```

```powershell
# Enable AI (mock) on a matter, then:
.\target\release\pst-dedup.exe job run --path $m --kind ai_suggest_codes --json
```

- Fail if AI disabled
- Skip missing text; skip withheld
- Middle-drop text; full catalog guidance in prompt
- Robust JSON extract (fence / prose / bare array)
- Write **only** suggestions (`pending`); never `item_codes` from the job
- Fingerprint skip when `reset=false` (text + model + template + catalog hash)
- Audit: kind / model / is_remote / template / counts — **no** keys or full bodies

## Human path

- **Accept** → `Matter::accept_ai_suggestion` → `apply_codes` + audit `source=ai_suggestion`
- **Reject** → status `rejected`
- Rich citations / chat → track **0052**

## Honesty

- AI is optional; core review works without it
- Local models require an operator-installed OpenAI-compatible server
- Cloud may transmit matter text — privilege and confidentiality risk
- Suggestions hallucinate — human review mandatory; definitions come from **your catalog**
- Middle-drop truncation may omit mid-document body
- JSON extract is best-effort against chatty models
- Mock is not a real model
- Not a substitute for privilege review

## License

MIT OR Apache-2.0. Runtime deps include `reqwest` (rustls), `keyring`, `matter-core`.
