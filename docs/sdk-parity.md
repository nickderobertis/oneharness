# CLI ↔ SDK parity

`oneharness` has three consumer surfaces, and they are not the same kind of
thing:

* **Rust** — `oneharness-core` is a real library. A Rust consumer that has to
  spawn the `oneharness` binary and parse its stdout to learn something is the
  gap. Closing it means an entry point that **returns** the value.
* **Python** (`python/oneharness-sdk`) and **TypeScript**
  (`npm/oneharness-sdk`) are typed CLI wrappers. They cannot link a Rust
  library, and the subprocess is not the defect — dropping to raw argv is.
  Closing a gap here means a **typed method** exists, so a caller never
  hand-assembles a command line and never parses stdout itself.

This document is the audit: every capability, every flag, every output field,
every surface. It is generated from
`crates/oneharness-core/src/domain/capability.rs` and the schema bundle (what is
declared) and from each client's own source and checked-in schemas (what is
actually implemented), so a row that says "yes" is a statement about code that
exists.

Regenerate with `just parity-audit`.

## How the coverage is held in place

Three mechanisms, all deriving from the manifest rather than from a list someone
remembers to update:

1. **`tests/capability.rs`** reconciles the manifest against the command tree
   clap actually builds. Every verb must be a capability or carry a written
   reason it is not one; every long flag must be bound to an SDK option, listed
   in the capability's always-sent argv, or declined with a reason. A flag added
   to `src/cli.rs` fails the build until someone decides which of the three it
   is. There is no default and no silence.
2. **`tests/library_surface.rs`** exercises every capability's Rust entry point
   in process — a real config file, real PATH resolution, real probes — with a
   failure path apiece. That is the proof the Rust column is true rather than
   aspirational.
3. **`scripts/check-sdk-coverage.sh`** fails when a capability has no method in
   a language client, reading the declared side from Rust and the defined side
   from each client's own source. The sibling `sdk-check` gates compare schemas
   and types, so they catch a method that *drifted*; this catches the one nobody
   wrote, which is the failure that let five verbs ship uncovered. Its own red
   is behaviorally tested by `scripts/check-sdk-coverage-test.sh` — a gate whose
   entire purpose is to fail is worth nothing unproven.

Which recipe runs each of them is the justfile's to say, not this document's, so
the *Which gate runs them* table below is generated from it. That table is the
one place to look — and to link to — for what a single gate run composes.

## Status

The **Rust core** column is complete: every capability has an
`oneharness-core` entry point that returns what the CLI prints, and
`tests/library_surface.rs` drives each one. In particular
`io::usage::report` is the whole `usage` verb — selection, variant identities,
configured binaries, bounded concurrency, and the single clock read — not just
the single-identity `io::usage::probe` that was already public. A consumer doing
provider health checks (`oneagentgraph/src/health.rs`) can call it directly and
delete its `Command::new(oneharness_bin)`.

The **Python and TypeScript** columns are complete too: every capability has a
typed method, so no consumer of either client has to hand-assemble a command
line or parse stdout for anything the CLI can do. Neither client names a flag of
its own — both render argv from the manifest below (`CAPABILITIES` in
TypeScript, the generated `capabilities.json` in Python) — which is what makes
the per-flag tables a description of both clients at once rather than of one
implementation each.

So there is nothing left uncovered, and no capability is declared deliberately
uncovered: every verb the CLI exposes is worth reaching from a program, which is
what this CLI is for.

## Notes on the deliberately-uncovered

Three kinds of flag are declined rather than bound, each with its reason in the
tables below:

* **`--compact`** is always sent. The SDKs parse the JSON, so the only sensible
  rendering is the compact one and there is nothing to choose.
* **`--format text`** is the human-readable view of data the JSON already
  carries. An SDK consuming the contract has no use for it.
* **`--bypass` / `--no-bypass`** are shorthands for `--mode`. One setting with
  two spellings is how a caller ends up passing both, which clap then refuses.

Three verbs' outputs are **text, not a contract**, and the tables say so:
`init` writes a human confirmation line because its deliverable is the file, and
`gate` / `mock` answer with a harness's own native verdict — or with nothing at
all when the call is allowed through. Their typed methods return that text (or
`null`) rather than pretending to have parsed a document.

One option is renamed rather than declined. A TypedDict declares its fields as
class-body annotations, so `sync`'s `--global` cannot be a Python field called
`global` — that is a syntax error, not a field. The generator gives a
keyword-named option the conventional trailing underscore (`global_`) and
`input-keys.json` carries the mapping back, so the CLI still receives its own
spelling and only the Python caller sees the suffix.

<!-- BEGIN GENERATED: capability-tables -->
<!-- Generated by `just parity-audit`. The capability, flag and output rows come
     from `domain::capability::CAPABILITIES` and the schema bundle; the
     Python/TypeScript columns are read from each client's own source and
     checked-in schemas; the gate table is read from the justfile's `check`
     recipe. Edit those, not this block. -->

### Which gate runs them

Read from the justfile's own `check` recipe, so this is the composition a
run actually has rather than a second copy of it. Each recipe below is
runnable alone while iterating; `just check` runs every one of them, along
with `fmt-check`, `lint`, `lint-sh`, `coverage`, `build`, `smoke`.

| Recipe | What it holds in place |
| --- | --- |
| `just test` | `tests/capability.rs` and `tests/library_surface.rs` |
| `just lint-workflows` | `scripts/check-sdk-coverage.sh` and `scripts/check-parity-audit.sh` |
| `just sdk-check` | the TypeScript client's generated-contract drift check, lint, types, and packaged-CLI e2e |
| `just python-sdk-check` | the same for the Python client, on the oldest supported interpreter |

`just parity-audit` regenerates this document; the `check-parity-audit.sh`
run inside `lint-workflows` is what fails when the checked-in copy no longer
matches either source.

### Capabilities

One row per thing the CLI can do. **Rust core** names the `oneharness-core`
entry point a consumer calls instead of spawning the binary; **Python** and
**TypeScript** say whether that client defines the method today.

| Capability | CLI | Rust core | Python | TypeScript | Output |
| --- | --- | --- | --- | --- | --- |
| `run` | `oneharness run` | `oneharness_core::io::run::run` | yes | yes | `run_report` |
| `runStream` | `oneharness run` | `oneharness_core::io::run::run` | yes | yes | `run_stream_envelope` (one per line) |
| `list` | `oneharness list` | `oneharness_core::io::registry::list` | yes | yes | `list_report` |
| `detect` | `oneharness detect` | `oneharness_core::io::detect::detect` | yes | yes | `detect_report` |
| `config` | `oneharness config` | `oneharness_core::domain::config::explain` | yes | yes | `config_report` |
| `sync` | `oneharness sync` | `oneharness_core::io::sync::sync` | yes | yes | `sync_report` |
| `init` | `oneharness init` | `oneharness_core::io::init::init` | yes | yes | text (see notes) |
| `usage` | `oneharness usage` | `oneharness_core::io::usage::report` | yes | yes | `usage_report` |
| `gate` | `oneharness gate` | `oneharness_core::domain::gate::render_deny` | yes | yes | text (see notes) |
| `mock` | `oneharness mock` | `oneharness_core::domain::mock::decide` | yes | yes | text (see notes) |
| `interrupt` | `oneharness interrupt` | `oneharness_core::io::control::send` | yes | yes | `interrupt_response` |
| `history` | `oneharness history show` | `oneharness_core::io::history::read_session` | yes | yes | `history_records` |
| `historyList` | `oneharness history list` | `oneharness_core::io::history::list_sessions` | yes | yes | `history_list` |
| `historyWatch` | `oneharness history watch` | `oneharness_core::io::history::HistoryWatcher` | yes | yes | `history_stream_envelope` (one per line) |
| `historyClear` | `oneharness history clear` | `oneharness_core::io::history::remove_sessions` | yes | yes | `history_clear_report` |
| `historyMigrate` | `oneharness history migrate` | `oneharness_core::io::history::migrate` | yes | yes | `history_migrate_report` |

### Flags, per capability

Every long flag each verb declares, and the SDK option that renders it. This
is the **declared binding** — what a method must send once it exists — not a
claim that a client implements it; the capability table above is what says
which clients do. `tests/capability.rs` fails if a flag appears in
`src/cli.rs` and in neither column here.

#### `run` — `oneharness run`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| `--no-stream` | _(always sent)_ | fixed |
| `--prompt` | `prompt` | `--flag VALUE` |
| `--prompt` | `batchPrompts` | `--flag VALUE` per element |
| `--prompt-file` | `promptFiles` | `--flag VALUE` per element |
| `--harness` | `harnesses` | `--flag VALUE` per element |
| `--mock-harness` | `mockHarnesses` | `--flag VALUE` per element |
| `--all` | `all` | `--flag` when true (refused beside `harnesses`) |
| `--exclude` | `exclude` | `--flag VALUE` per element |
| `--model` | `models` | `--flag VALUE` per element |
| `--system` | `system` | `--flag VALUE` |
| `--system-file` | `systemFile` | `--flag VALUE` (refused beside `system`) |
| `--reasoning` | `reasoning` | `--flag VALUE` |
| `--resume` | `resume` | `--flag VALUE` |
| `--fork` | `fork` | `--flag` when true |
| `--session` | `session` | `--flag VALUE` |
| `--session-dir` | `sessionDir` | `--flag VALUE` |
| `--control` | `control` | `--flag` when true |
| `--output-format` | `outputFormat` | `--flag VALUE` |
| `--events` | `events` | `--flag` when true |
| `--mock-rules` | `mockRules` | `--flag VALUE` |
| `--spy-file` | `spyFile` | `--flag VALUE` |
| `--schema` | `schema` | `--flag VALUE` |
| `--schema-max-retries` | `schemaMaxRetries` | `--flag VALUE` |
| `--output-dir` | `outputDir` | `--flag VALUE` |
| `--timeout` | `timeoutSeconds` | `--flag VALUE` |
| `--cwd` | `cwd` | `--flag VALUE` |
| `--env` | `env` | `--flag KEY=VALUE` per entry |
| `--mode` | `mode` | `--flag VALUE` |
| `--permit-prompts` | `permitPrompts` | `--flag` when true |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |
| `--max-parallel` | `maxParallel` | `--flag VALUE` |
| `--batch-strategy` | `batchStrategy` | `--flag VALUE` |
| `--run-mode` | `runMode` | `--flag VALUE` |
| `--print-command` | `printCommand` | `--flag` when true |
| `--bin` | `bins` | `--flag KEY=VALUE` per entry |
| `--require-available` | `requireAvailable` | `--flag` when true |
| `--history` | `history` | `--flag` when true (refused beside `noHistory`) |
| `--no-history` | `noHistory` | `--flag` when true |
| `--history-dir` | `historyDir` | `--flag VALUE` |
| `--history-name` | `historyName` | `--flag VALUE` |
| `--history-label` | `historyLabels` | `--flag KEY=VALUE` per entry |
| _(after `--`)_ | `passthrough` | appended verbatim |
| `--bypass` | **deliberately none** | `mode: "bypass"` is the same request |
| `--no-bypass` | **deliberately none** | `mode: "default"` is the same request |
| `--stream` | **deliberately none** | `runStream()` is the streaming method; this one returns one report |

#### `runStream` — `oneharness run`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| `--stream` | _(always sent)_ | fixed |
| `--prompt` | `prompt` | `--flag VALUE` |
| `--prompt` | `batchPrompts` | `--flag VALUE` per element |
| `--prompt-file` | `promptFiles` | `--flag VALUE` per element |
| `--harness` | `harnesses` | `--flag VALUE` per element |
| `--mock-harness` | `mockHarnesses` | `--flag VALUE` per element |
| `--all` | `all` | `--flag` when true (refused beside `harnesses`) |
| `--exclude` | `exclude` | `--flag VALUE` per element |
| `--model` | `models` | `--flag VALUE` per element |
| `--system` | `system` | `--flag VALUE` |
| `--system-file` | `systemFile` | `--flag VALUE` (refused beside `system`) |
| `--reasoning` | `reasoning` | `--flag VALUE` |
| `--resume` | `resume` | `--flag VALUE` |
| `--fork` | `fork` | `--flag` when true |
| `--session` | `session` | `--flag VALUE` |
| `--session-dir` | `sessionDir` | `--flag VALUE` |
| `--control` | `control` | `--flag` when true |
| `--output-format` | `outputFormat` | `--flag VALUE` |
| `--events` | `events` | `--flag` when true |
| `--mock-rules` | `mockRules` | `--flag VALUE` |
| `--spy-file` | `spyFile` | `--flag VALUE` |
| `--schema` | `schema` | `--flag VALUE` |
| `--schema-max-retries` | `schemaMaxRetries` | `--flag VALUE` |
| `--output-dir` | `outputDir` | `--flag VALUE` |
| `--timeout` | `timeoutSeconds` | `--flag VALUE` |
| `--cwd` | `cwd` | `--flag VALUE` |
| `--env` | `env` | `--flag KEY=VALUE` per entry |
| `--mode` | `mode` | `--flag VALUE` |
| `--permit-prompts` | `permitPrompts` | `--flag` when true |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |
| `--max-parallel` | `maxParallel` | `--flag VALUE` |
| `--batch-strategy` | `batchStrategy` | `--flag VALUE` |
| `--run-mode` | `runMode` | `--flag VALUE` |
| `--print-command` | `printCommand` | `--flag` when true |
| `--bin` | `bins` | `--flag KEY=VALUE` per entry |
| `--require-available` | `requireAvailable` | `--flag` when true |
| `--history` | `history` | `--flag` when true (refused beside `noHistory`) |
| `--no-history` | `noHistory` | `--flag` when true |
| `--history-dir` | `historyDir` | `--flag VALUE` |
| `--history-name` | `historyName` | `--flag VALUE` |
| `--history-label` | `historyLabels` | `--flag KEY=VALUE` per entry |
| _(after `--`)_ | `passthrough` | appended verbatim |
| `--bypass` | **deliberately none** | `mode: "bypass"` is the same request |
| `--no-bypass` | **deliberately none** | `mode: "default"` is the same request |
| `--no-stream` | **deliberately none** | this method streams by definition, so the negative half cannot apply |

#### `list` — `oneharness list`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |

#### `detect` — `oneharness detect`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| `--harness` | `harnesses` | `--flag VALUE` per element |
| `--all` | `all` | `--flag` when true (refused beside `harnesses`) |
| `--exclude` | `exclude` | `--flag VALUE` per element |
| `--bin` | `bins` | `--flag KEY=VALUE` per entry |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |
| `--require-available` | `requireAvailable` | `--flag` when true |

#### `config` — `oneharness config`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| `--cwd` | `cwd` | `--flag VALUE` |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |

#### `sync` — `oneharness sync`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| `--cwd` | `cwd` | `--flag VALUE` |
| `--harness` | `harnesses` | `--flag VALUE` per element |
| `--check` | `check` | `--flag` when true |
| `--global` | `global` | `--flag` when true |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |

#### `init` — `oneharness init`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| _(positional)_ | `path` | positional argument |
| `--force` | `force` | `--flag` when true |

#### `usage` — `oneharness usage`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| `--harness` | `harnesses` | `--flag VALUE` per element |
| `--all` | `all` | `--flag` when true (refused beside `harnesses`) |
| `--exclude` | `exclude` | `--flag VALUE` per element |
| `--bin` | `bins` | `--flag KEY=VALUE` per entry |
| `--cwd` | `cwd` | `--flag VALUE` |
| `--timeout` | `timeoutSeconds` | `--flag VALUE` |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |
| `--format` | **deliberately none** | the SDKs consume the JSON contract; `--format text` is the human-readable view of the same data, carrying nothing the JSON does not |

#### `gate` — `oneharness gate`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| _(positional)_ | `harness` | positional argument |
| `--deny-if-contains` | `denyIfContains` | `--flag VALUE` |
| `--reason` | `reason` | `--flag VALUE` |

#### `mock` — `oneharness mock`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| _(positional)_ | `harness` | positional argument |
| `--rules` | `rules` | `--flag VALUE` |
| `--spy-file` | `spyFile` | `--flag VALUE` |

#### `interrupt` — `oneharness interrupt`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| `--session` | `session` | `--flag VALUE` |
| `--input` | `input` | `--flag VALUE` |
| `--session-dir` | `sessionDir` | `--flag VALUE` |
| `--cwd` | `cwd` | `--flag VALUE` |

#### `history` — `oneharness history show`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| _(positional)_ | `session` | positional argument (suppressed by `last`) |
| `--last` | `last` | `--flag` when true |
| `--all` | `all` | `--flag` when true |
| `--project` | `project` | `--flag VALUE` (refused beside `allProjects`) |
| `--all-projects` | `allProjects` | `--flag` when true |
| `--history-dir` | `historyDir` | `--flag VALUE` |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |
| `--format` | **deliberately none** | the SDKs consume the JSON contract; `--format text` is the human-readable view of the same data, carrying nothing the JSON does not |

#### `historyList` — `oneharness history list`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| `--variant` | `variant` | `--flag VALUE` |
| `--project` | `project` | `--flag VALUE` (refused beside `allProjects`) |
| `--all-projects` | `allProjects` | `--flag` when true |
| `--history-dir` | `historyDir` | `--flag VALUE` |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |
| `--format` | **deliberately none** | the SDKs consume the JSON contract; `--format text` is the human-readable view of the same data, carrying nothing the JSON does not |

#### `historyWatch` — `oneharness history watch`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--format` | _(always sent)_ | fixed |
| `--after` | `after` | `--flag VALUE` |
| `--label` | `labels` | `--flag KEY=VALUE` per entry |
| `--variant` | `variant` | `--flag VALUE` |
| `--project` | `project` | `--flag VALUE` (refused beside `allProjects`) |
| `--all-projects` | `allProjects` | `--flag` when true |
| `--history-dir` | `historyDir` | `--flag VALUE` |
| `--events` | `events` | `--flag` when true |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |

#### `historyClear` — `oneharness history clear`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| `--project` | `project` | `--flag VALUE` (refused beside `allProjects`) |
| `--all-projects` | `allProjects` | `--flag` when true |
| `--yes` | `yes` | `--flag` when true |
| `--history-dir` | `historyDir` | `--flag VALUE` |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |

#### `historyMigrate` — `oneharness history migrate`

| CLI flag | SDK option | How it is sent |
| --- | --- | --- |
| `--compact` | _(always sent)_ | fixed |
| `--history-dir` | `historyDir` | `--flag VALUE` |
| `--config` | `config` | `--flag VALUE` (refused beside `noConfig`) |
| `--no-config` | `noConfig` | `--flag` when true |

### Output contracts, field by field

One row per document the CLI prints. **Fields** are the ones on the document
itself, past any array or union wrapper; **all** counts every field reachable
through it, nested ones included, and that whole set is what the coverage
marks are computed over — a nested field a client's bundle is missing shows
up here as surely as a top-level one.

There is one source: `sdk_schema::bundle` generates the Rust type's schema,
and each client checks in what it was handed. **Rust core** returns the typed
value itself, so it carries every field by construction. Neither client
strips what it validates, so an additive field a newer CLI emits reaches a
caller rather than being dropped.

| Output | Fields on the document | All | Rust core | Python | TypeScript |
| --- | --- | --- | --- | --- | --- |
| `run_report` | `batch`, `bypass_permissions`, `config_files`, `control`, `dry_run`, `fallback`, `fork`, `history_file`, `mock_rules`, `model`, `models`, `oneharness_version`, `permission_mode`, `prompt`, `results`, `resume`, `schema`, `schema_max_retries`, `schema_version`, `session`, `spy_file` | 85 | yes | yes | yes |
| `run_stream_envelope` | `event`, `report`, `type` | 88 | yes | yes | yes |
| `list_report` | `harnesses`, `schema_version` | 36 | yes | yes | yes |
| `detect_report` | `detected`, `schema_version` | 7 | yes | yes | yes |
| `config_report` | `all`, `allowed_tools`, `bypass`, `config_files`, `denied_tools`, `env`, `exclude`, `harness`, `harnesses`, `history`, `history_dir`, `history_labels`, `hooks`, `max_parallel`, `mode`, `model`, `models`, `output_format`, `reasoning`, `require_available`, `run_mode`, `schema_file`, `schema_max_retries`, `schema_version`, `stream`, `system`, `timeout` | 39 | yes | yes | yes |
| `sync_report` | `check`, `config_files`, `results`, `schema_version` | 9 | yes | yes | yes |
| `usage_report` | `identities`, `observed_at`, `schema_version` | 33 | yes | yes | yes |
| `interrupt_response` | `error`, `mechanism`, `ok`, `reason`, `redirected`, `v` | 6 | yes | yes | yes |
| `history_records` | `duration_ms`, `error`, `events`, `exit_code`, `failure_kind`, `finished_at`, `harness`, `harness_id`, `history_id`, `labels`, `model`, `model_ms`, `name`, `observed_tool_ms`, `permission_mode`, `project`, `prompt`, `schema_version`, `session`, `session_id`, `started_at`, `status`, `text`, `text_source`, `time_to_first_token_ms`, `timestamp`, `tool_ms`, `usage`, `variant`, `work` | 41 | yes | yes | yes |
| `history_list` | `harnesses`, `id`, `labels`, `name`, `path`, `project`, `record_count`, `started` | 8 | yes | yes | yes |
| `history_stream_envelope` | `line`, `record`, `type` | 46 | yes | yes | yes |
| `history_clear_report` | `dry_run`, `files`, `hint`, `removed`, `would_remove` | 5 | yes | yes | yes |
| `history_migrate_report` | `files`, `files_processed` | 6 | yes | yes | yes |

<!-- END GENERATED: capability-tables -->
