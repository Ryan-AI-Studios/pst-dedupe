repo{
  name:"pst-dedupe"
  os:"Windows"
  goal:"scoped edits; verified behavior; clean provenance"
}

onboarding{
  startup:"read .agents/skills/onboarding/SKILL.md at session start"
}

ledgerful{
  before[3]:
    "ledgerful ledger status --compact"
    "ledgerful scan --impact for meaningful code/config/policy edits"
    "read .ledgerful/reports/latest-impact.json if present"
  edit[3]:
    "do not edit .ledgerful state files"
    "inspect hotspots"
    "inspect temporal couplings >70%"
  after[3]:
    "ledgerful verify; if aliases fail, use verify.commands"
    "cargo install --path . after Ledgerful source edits"
    "report risk, verification, pending tx, drift"
  skip[5]:
    "format-only"
    "scratch files"
    "binary/media-only"
    "lockfile-only dependency churn"
    "explicit user bypass"
  fail{
    unavailable:"continue with native checks; report missing signals"
    drift:"reconcile/adopt before continuing unless user says otherwise"
    verify:"report exact failed command and continue with justified fallback"
  }
}

ledger{
  start:"ledgerful ledger start <entity> --category <CATEGORY> --message <intent>"
  commit:"ledgerful ledger commit <tx-id> --summary <what> --reason <why>"
  hooks_install:"powershell -File scripts/install-hooks.ps1 (after clone; requires ledgerful on PATH)"
  hooks[4]:
    "pre-commit: ledgerful ledger status --compact --exit-code --verify-signatures; then scripts/pre-commit.ps1 (fmt/clippy/test)"
    "pre-push: ledgerful ledger status --compact --exit-code --verify-signatures; then ledgerful verify --scope fast"
    "commit-msg: ledgerful internal hook-commit-msg"
    "post-commit: ledgerful internal hook-post-commit"
}

verify{
  scope:"targeted during work; full commands before commit"
  commands[3]:
    "cargo fmt --all --check"
    "cargo clippy --workspace --all-targets -- -D warnings"
    "cargo test --workspace"
  hygiene[2]:
    "no secrets or .env commits"
    "temporary output belongs in output/ and should be removed before finish"
}

rust{
  forbid[2]:".unwrap()","expect() in production"
  errors:"use miette + Result"
  boundaries[9]:
    "pst-reader owns PST parsing: header, NDB, LTP, messaging extraction"
    "dedup-engine owns dedup hashing, index, CSV report, EML serialization"
    "pst-dedup-cli owns the CLI surface: inspect, scan, dups, JSON/CSV output"
    "pst-dedup-gui owns the egui app and background scan worker"
    "pst-writer owns experimental PST writing and fixture/EML import helpers"
    "matter-core owns matter layout, SQLite metadata, CAS, audit chain, jobs/checkpoints"
    "ingest-purview owns package detect, safe ZIP expand, leaf checkpoints; call from blocking worker only"
    "matter-entity owns offline entity/PII packs, entity_scan job logic, mask/hash hits"
    "matter-people owns people-comms graph: normalize_participant, people_graph two-pass job, edges/timeline"
    "matter-sentiment owns offline VADER-class sentiment (vader_lexicon_v1), unit-extreme aggregate, sentiment job"
    "matter-semantic owns local semantic search: Embedder/MockEmbedder, chunk+overlap, model-namespaced store, pre-filter cosine, semantic_index job"
  invariants[2]:
    "features work offline with local model"
    "preserve Windows paths; prefer camino for UTF-8 paths"
}

powershell{
  forbid[7]:"&&","[[","]]","then","fi","done","echo -e"
  prefer[6]:"Get-ChildItem","Get-Content","Test-Path","Join-Path","Copy-Item","Remove-Item"
  rules[3]:
    "use $_ and object properties for pipelines"
    "use backslashes for shell-level Windows paths"
    "avoid Bash shims for complex logic"
}
