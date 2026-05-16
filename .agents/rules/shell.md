@ENV: POWERSHELL_WIN32
@FORBID: &&
@ENFORCE: ;
@SCOPE: run_shell_command

# Shell Rules

- Use PowerShell-native commands on Windows.
- Prefer `rg` / `rg --files` for search.
- Prefer `cargo` commands for Rust verification.
- Do not delete build artifacts, `.changeguard`, `.agents`, `.git`, or fixture data unless explicitly requested.
- Do not pass secrets or `.env` contents to review tools.
