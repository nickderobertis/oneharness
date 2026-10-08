# AGENTS (live-schema)

Rules for the `live-schema` suite alone (`scripts/e2e-schema.sh`,
`.github/workflows/e2e-schema.yml`).

- schema's dispatch offers only ubuntu/macos (its native `--json-schema` argv is
  unreliable through the Windows `.cmd` shim).
