# Ledgerful Installation

If Ledgerful is not installed or not on the system `PATH`, follow these instructions.

## Linux / macOS (Bash)

```bash
curl -fsSL https://raw.githubusercontent.com/Ledgerful, LLC/Ledgerful/main/install/install.sh | sh
```

## Windows (PowerShell)

```powershell
iwr https://raw.githubusercontent.com/Ledgerful, LLC/Ledgerful/main/install/install.ps1 -UseB | iex
```

After installation, open a new terminal window or refresh your environment variables to ensure `ledgerful` is available.

## Starter config and credentials

Init template precedence is:

1. an existing path named by `LEDGERFUL_DEFAULT_CONFIG`;
2. `~/.ledgerful/default-config.toml`;
3. Ledgerful's built-in template.

Before publishing a new repo config, `ledgerful init` removes secret-bearing
assignments and structured connection URLs containing credentials. It reports
only the removed key paths. Use `GEMINI_API_KEY`, `OLLAMA_CLOUD_API_KEY`, or
the legacy `OLLAMA_API_KEY` in the process environment or an ignored repo-local
`.env`; TOML `${VAR}` interpolation is not supported.

The canonical executable is `ledgerful`; `ledgerful` remains a compatibility
alias in the same install directory. If Windows reports that the alias is
locked, close processes using `ledgerful.exe` and run:

```powershell
ledgerful update --binary
```

Ledgerful stages and verifies alias replacements and does not search for or
delete similarly named binaries elsewhere on `PATH`.
