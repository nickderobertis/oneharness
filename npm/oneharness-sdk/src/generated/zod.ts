/* Generated from oneharness Rust JSON Schemas. Do not edit. */

import { z } from "zod";
import type { ConfigOptions } from "./config-options.js";
import type {
  ConfigReport,
  Field,
  Field10,
  Field2,
  Field3,
  Field4,
  Field5,
  Field6,
  Field7,
  Field8,
  Field9,
  HarnessReport,
  HookEntry,
  VariantReport,
} from "./config-report.js";
import type {
  ActionEvent,
  BatchReport,
  ControlEvent,
  ControlReason,
  ControlReport,
  ControlVerb,
  ExecutionTelemetry,
  FailureKind,
  FallThrough,
  FallThroughReason,
  FallbackReport,
  RunReport,
  RunResult,
  SessionReport,
  Status,
  TimingSource,
  ToolCallStatus,
  Usage,
  UtcInstant,
} from "./contracts.js";
import type { DetectOptions } from "./detect-options.js";
import type { DetectInfo, DetectReport } from "./detection.js";
import type { GateOptions } from "./gate-options.js";
import type { HistoryRecord } from "./history.js";
import type { HistoryClearOptions } from "./history-clear-options.js";
import type { HistoryClearDryRun, HistoryClearRemoved, HistoryClearReport } from "./history-clear-report.js";
import type { HistoryLine } from "./history-line.js";
import type { HistoryList, HistorySessionSummary } from "./history-list.js";
import type { HistoryListOptions } from "./history-list-options.js";
import type { HistoryLookup, HistoryLookupByLast, HistoryLookupBySession } from "./history-lookup.js";
import type { HistoryMigrateOptions } from "./history-migrate-options.js";
import type { HistoryMigrateReport, MigrationSummary } from "./history-migrate-report.js";
import type { HistoryRecords } from "./history-records.js";
import type { HistoryEventLine, HistoryStreamEnvelope } from "./history-stream-envelope.js";
import type { HistoryWatchOptions } from "./history-watch-options.js";
import type { InitOptions } from "./init-options.js";
import type { InterruptOptions } from "./interrupt-options.js";
import type { InterruptResponse } from "./interrupt-response.js";
import type { MockOptions } from "./mock-options.js";
import type { HistoryLabels, OutputFormat, PermissionMode, RunMode, RunOptions } from "./options.js";
import type { HarnessInfo, ListReport, ModeInfo, VariantInfo } from "./registry.js";
import type { RunStreamEnvelope } from "./run-stream-envelope.js";
import type { SyncOptions } from "./sync-options.js";
import type { FileStatus, HookFileResult, SyncReport, SyncResult, SyncStatus } from "./sync-report.js";
import type { UsageOptions } from "./usage-options.js";
import type {
  AuthMode,
  QuotaAmount,
  QuotaCounters,
  QuotaUnit,
  UnavailableReason,
  UnknownReason,
  UsageAvailability,
  UsageIdentity,
  UsageReport,
  UsageWindow,
  WindowUsage,
  Windows,
} from "./usage-report.js";

export type AbsolutePath = ControlReport["socket"];
export type BatchStrategy = BatchReport["strategy"];
export type ControlShape = ControlReport["mechanism"];
export type IdentitySelector = UsageIdentity["selector"];
export type ModeHeadless = ModeInfo["headless"];
export type SessionPhase = SessionReport["phase"];
export type UsedPercent = number;

export const AbsolutePathSchema: z.ZodType<AbsolutePath> = z.string();

export const ActionEventSchema: z.ZodType<ActionEvent> = z.looseObject({
  duration_ms: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  finished_at: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  index: z
    .int()
    .gte(0)
    .refine((value) => value !== undefined, { message: "Required" }),
  input: z.unknown().refine((value) => value !== undefined, { message: "Required" }),
  kind: z.string().refine((value) => value !== undefined, { message: "Required" }),
  name: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  output: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  started_at: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  status: z
    .union([z.lazy(() => ToolCallStatusSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  timing_source: z.union([z.lazy(() => TimingSourceSchema), z.null()]).optional(),
  tool_call_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const AuthModeSchema: z.ZodType<AuthMode> = z.union([
  z.literal("subscription"),
  z.literal("api_key"),
  z.literal("unknown"),
]);

export const BatchReportSchema: z.ZodType<BatchReport> = z.looseObject({
  forked: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  prompt_count: z
    .int()
    .gte(0)
    .refine((value) => value !== undefined, { message: "Required" }),
  strategy: z.lazy(() => BatchStrategySchema).refine((value) => value !== undefined, { message: "Required" }),
});

export const BatchStrategySchema: z.ZodType<BatchStrategy> = z.union([z.literal("speed"), z.literal("min-tokens")]);

export const ConfigOptionsSchema: z.ZodType<ConfigOptions> = z.strictObject({
  config: z.string().optional(),
  cwd: z.string().optional(),
  noConfig: z.boolean().optional(),
});

export const ConfigReportSchema: z.ZodType<ConfigReport> = z.looseObject({
  all: z.lazy(() => FieldSchema).refine((value) => value !== undefined, { message: "Required" }),
  allowed_tools: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  bypass: z.lazy(() => FieldSchema).refine((value) => value !== undefined, { message: "Required" }),
  config_files: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
  denied_tools: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  env: z
    .record(
      z.string(),
      z.lazy(() => Field3Schema),
    )
    .refine((value) => value !== undefined, { message: "Required" }),
  exclude: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  harness: z
    .record(
      z.string(),
      z.lazy(() => HarnessReportSchema),
    )
    .refine((value) => value !== undefined, { message: "Required" }),
  harnesses: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  history: z.lazy(() => FieldSchema).refine((value) => value !== undefined, { message: "Required" }),
  history_dir: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  history_labels: z
    .record(
      z.string(),
      z.lazy(() => Field3Schema),
    )
    .refine((value) => value !== undefined, { message: "Required" }),
  hooks: z.lazy(() => Field10Schema).refine((value) => value !== undefined, { message: "Required" }),
  max_parallel: z.lazy(() => Field8Schema).refine((value) => value !== undefined, { message: "Required" }),
  mode: z.lazy(() => Field4Schema).refine((value) => value !== undefined, { message: "Required" }),
  model: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  models: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  output_format: z.lazy(() => Field6Schema).refine((value) => value !== undefined, { message: "Required" }),
  reasoning: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  require_available: z.lazy(() => FieldSchema).refine((value) => value !== undefined, { message: "Required" }),
  run_mode: z.lazy(() => Field9Schema).refine((value) => value !== undefined, { message: "Required" }),
  schema_file: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  schema_max_retries: z.lazy(() => Field7Schema).refine((value) => value !== undefined, { message: "Required" }),
  schema_version: z.string().refine((value) => value !== undefined, { message: "Required" }),
  stream: z.lazy(() => FieldSchema).refine((value) => value !== undefined, { message: "Required" }),
  system: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  timeout: z.lazy(() => Field5Schema).refine((value) => value !== undefined, { message: "Required" }),
});

export const ControlEventSchema: z.ZodType<ControlEvent> = z.union([
  z.looseObject({
    at: z.lazy(() => UtcInstantSchema).refine((value) => value !== undefined, { message: "Required" }),
    outcome: z.literal("served").refine((value) => value !== undefined, { message: "Required" }),
    redirected: z.boolean().optional(),
    verb: z.lazy(() => ControlVerbSchema).refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    at: z.lazy(() => UtcInstantSchema).refine((value) => value !== undefined, { message: "Required" }),
    outcome: z.literal("refused").refine((value) => value !== undefined, { message: "Required" }),
    reason: z.lazy(() => ControlReasonSchema).refine((value) => value !== undefined, { message: "Required" }),
    verb: z.lazy(() => ControlVerbSchema).refine((value) => value !== undefined, { message: "Required" }),
  }),
]);

export const ControlReasonSchema: z.ZodType<ControlReason> = z.union([
  z.literal("unsupported"),
  z.literal("no_active_turn"),
  z.literal("not_running"),
]);

export const ControlReportSchema: z.ZodType<ControlReport> = z.looseObject({
  interrupts: z.array(z.lazy(() => ControlEventSchema)).refine((value) => value !== undefined, { message: "Required" }),
  mechanism: z.lazy(() => ControlShapeSchema).refine((value) => value !== undefined, { message: "Required" }),
  socket: z.lazy(() => AbsolutePathSchema).refine((value) => value !== undefined, { message: "Required" }),
});

export const ControlShapeSchema: z.ZodType<ControlShape> = z.union([
  z.literal("claude-control-request"),
  z.literal("codex-app-server"),
  z.literal("opencode-http"),
  z.literal("acp-cancel"),
  z.literal("crush-http"),
]);

export const ControlVerbSchema: z.ZodType<ControlVerb> = z.literal("interrupt");

export const DetectInfoSchema: z.ZodType<DetectInfo> = z.looseObject({
  available: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  bin: z.string().refine((value) => value !== undefined, { message: "Required" }),
  id: z.string().refine((value) => value !== undefined, { message: "Required" }),
  path: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  version: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const DetectOptionsSchema: z.ZodType<DetectOptions> = z.strictObject({
  all: z.boolean().optional(),
  bins: z.record(z.string(), z.string()).optional(),
  config: z.string().optional(),
  exclude: z.array(z.string()).optional(),
  harnesses: z.array(z.string()).optional(),
  noConfig: z.boolean().optional(),
  requireAvailable: z.boolean().optional(),
});

export const DetectReportSchema: z.ZodType<DetectReport> = z.looseObject({
  detected: z.array(z.lazy(() => DetectInfoSchema)).refine((value) => value !== undefined, { message: "Required" }),
  schema_version: z.string().refine((value) => value !== undefined, { message: "Required" }),
});

export const ExecutionTelemetrySchema: z.ZodType<ExecutionTelemetry> = z.union([
  z.looseObject({
    finished_at: z
      .union([
        z
          .string()
          .min(24)
          .regex(
            new RegExp(
              "^\\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\\d|3[01])T([01]\\d|2[0-3]):[0-5]\\d:([0-5]\\d|60)\\.\\d{3}Z$",
              "u",
            ),
          )
          .refine((value) => [...value].length <= 24, { message: "Too long: expected at most 24 characters" }),
        z.null(),
      ])
      .refine((value) => value !== undefined, { message: "Required" }),
    model_ms: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
    source: z.literal("provider_measured").refine((value) => value !== undefined, { message: "Required" }),
    started_at: z
      .string()
      .min(24)
      .regex(
        new RegExp(
          "^\\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\\d|3[01])T([01]\\d|2[0-3]):[0-5]\\d:([0-5]\\d|60)\\.\\d{3}Z$",
          "u",
        ),
      )
      .refine((value) => [...value].length <= 24, { message: "Too long: expected at most 24 characters" })
      .refine((value) => value !== undefined, { message: "Required" }),
    time_to_first_token_ms: z
      .union([z.int().gte(0), z.null()])
      .refine((value) => value !== undefined, { message: "Required" }),
    tool_ms: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    source: z.literal("partial_invocation").refine((value) => value !== undefined, { message: "Required" }),
    started_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    source: z.literal("stdout_observed").refine((value) => value !== undefined, { message: "Required" }),
    tool_ms: z
      .int()
      .gte(0)
      .refine((value) => value !== undefined, { message: "Required" }),
  }),
]);

export const FailureKindSchema: z.ZodType<FailureKind> = z.union([
  z.literal("auth"),
  z.literal("rate_limit"),
  z.literal("model_not_found"),
  z.literal("quota"),
  z.literal("session_not_found"),
  z.literal("tool_deferred"),
  z.literal("untrusted_directory"),
  z.literal("input_too_large"),
]);

export const FallThroughSchema: z.ZodType<FallThrough> = z.looseObject({
  detail: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
  reason: z.lazy(() => FallThroughReasonSchema).refine((value) => value !== undefined, { message: "Required" }),
});

export const FallThroughReasonSchema: z.ZodType<FallThroughReason> = z.union([
  z.literal("not-installed"),
  z.literal("spawn-error"),
  z.literal("auth"),
  z.literal("quota"),
  z.literal("session-not-found"),
  z.literal("untrusted-directory"),
  z.literal("input-too-large"),
  z.literal("model-not-found"),
  z.literal("rate-limit"),
]);

export const FallbackReportSchema: z.ZodType<FallbackReport> = z.looseObject({
  fell_through: z
    .array(z.lazy(() => FallThroughSchema))
    .refine((value) => value !== undefined, { message: "Required" }),
  ran: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const FieldSchema: z.ZodType<Field> = z.looseObject({
  source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  value: z.union([z.boolean(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const Field10Schema: z.ZodType<Field10> = z.looseObject({
  source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  value: z
    .union([z.array(z.lazy(() => HookEntrySchema)), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const Field2Schema: z.ZodType<Field2> = z.looseObject({
  source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  value: z.union([z.array(z.string()), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const Field3Schema: z.ZodType<Field3> = z.looseObject({
  source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  value: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const Field4Schema: z.ZodType<Field4> = z.looseObject({
  source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  value: z
    .union([z.lazy(() => PermissionModeSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const Field5Schema: z.ZodType<Field5> = z.looseObject({
  source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  value: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const Field6Schema: z.ZodType<Field6> = z.looseObject({
  source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  value: z
    .union([z.lazy(() => OutputFormatSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const Field7Schema: z.ZodType<Field7> = z.looseObject({
  source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  value: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const Field8Schema: z.ZodType<Field8> = z.looseObject({
  source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  value: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const Field9Schema: z.ZodType<Field9> = z.looseObject({
  source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  value: z
    .union([z.lazy(() => RunModeSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const FileStatusSchema: z.ZodType<FileStatus> = z.union([
  z.literal("created"),
  z.literal("updated"),
  z.literal("unchanged"),
]);

export const GateOptionsSchema: z.ZodType<GateOptions> = z.strictObject({
  denyIfContains: z.string().optional(),
  event: z.string().refine((value) => value !== undefined, { message: "Required" }),
  harness: z
    .string()
    .min(1)
    .refine((value) => value !== undefined, { message: "Required" }),
  reason: z.string().optional(),
});

export const HarnessInfoSchema: z.ZodType<HarnessInfo> = z.looseObject({
  control: z
    .union([z.lazy(() => ControlShapeSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  default_bin: z.string().refine((value) => value !== undefined, { message: "Required" }),
  display: z.string().refine((value) => value !== undefined, { message: "Required" }),
  example_command: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
  fork_reuses_cache: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  id: z.string().refine((value) => value !== undefined, { message: "Required" }),
  install_hint: z.string().refine((value) => value !== undefined, { message: "Required" }),
  mock_rewrite: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  modes: z.array(z.lazy(() => ModeInfoSchema)).refine((value) => value !== undefined, { message: "Required" }),
  output_format: z.lazy(() => OutputFormatSchema).refine((value) => value !== undefined, { message: "Required" }),
  session_capable: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  supports_allowed_tools: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  supports_denied_tools: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  supports_fork: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  supports_hooks: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  supports_mock_deny: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  supports_native_schema: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  supports_prompt_stdin: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  supports_reasoning: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  supports_resume: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  supports_system_file: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  sync_file: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  variants: z.array(z.lazy(() => VariantInfoSchema)).refine((value) => value !== undefined, { message: "Required" }),
});

export const HarnessReportSchema: z.ZodType<HarnessReport> = z.looseObject({
  allowed_tools: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  args: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  bin: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  denied_tools: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  env: z
    .record(
      z.string(),
      z.lazy(() => Field3Schema),
    )
    .refine((value) => value !== undefined, { message: "Required" }),
  hooks: z.looseObject({}).refine((value) => value !== undefined, { message: "Required" }),
  model: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  reasoning: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  settings: z.looseObject({}).refine((value) => value !== undefined, { message: "Required" }),
  variant: z
    .record(
      z.string(),
      z.lazy(() => VariantReportSchema),
    )
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const HistoryClearDryRunSchema: z.ZodType<HistoryClearDryRun> = z.looseObject({
  dry_run: z.literal(true).refine((value) => value !== undefined, { message: "Required" }),
  files: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
  hint: z.string().refine((value) => value !== undefined, { message: "Required" }),
  would_remove: z
    .int()
    .gte(0)
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const HistoryClearOptionsSchema: z.ZodType<HistoryClearOptions> = z.strictObject({
  allProjects: z.boolean().optional(),
  config: z.string().optional(),
  historyDir: z.string().optional(),
  noConfig: z.boolean().optional(),
  project: z.string().optional(),
  yes: z.boolean().optional(),
});

export const HistoryClearRemovedSchema: z.ZodType<HistoryClearRemoved> = z.looseObject({
  dry_run: z.literal(false).refine((value) => value !== undefined, { message: "Required" }),
  files: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
  removed: z
    .int()
    .gte(0)
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const HistoryClearReportSchema: z.ZodType<HistoryClearReport> = z.union([
  z.lazy(() => HistoryClearRemovedSchema),
  z.lazy(() => HistoryClearDryRunSchema),
]);

export const HistoryEventLineSchema: z.ZodType<HistoryEventLine> = z.union([
  z.looseObject({
    event: z.lazy(() => ActionEventSchema).refine((value) => value !== undefined, { message: "Required" }),
    harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
    harness_id: z.union([z.string(), z.null()]).optional(),
    run_id: z
      .string()
      .min(36)
      .regex(
        new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
      )
      .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
      .refine((value) => value !== undefined, { message: "Required" }),
    schema_version: z
      .union([z.literal("1.2"), z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
      .refine((value) => value !== undefined, { message: "Required" }),
    variant: z.union([z.string(), z.null()]).optional(),
  }),
  z.looseObject({
    event: z
      .intersection(
        z.lazy(() => ActionEventSchema),
        z.looseObject({
          timing_source: z.never().optional(),
        }),
      )
      .refine((value) => value !== undefined, { message: "Required" }),
    harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
    harness_id: z.union([z.string(), z.null()]).optional(),
    run_id: z
      .string()
      .min(36)
      .regex(
        new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
      )
      .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
      .refine((value) => value !== undefined, { message: "Required" }),
    schema_version: z
      .union([z.literal("1.0"), z.literal("1.1")])
      .refine((value) => value !== undefined, { message: "Required" }),
    variant: z.union([z.string(), z.null()]).optional(),
  }),
]);

export const HistoryLabelsSchema: z.ZodType<HistoryLabels> = z.record(
  z
    .string()
    .regex(new RegExp("^[A-Za-z0-9]", "u"))
    .refine((value) => [...value].length <= 64, { message: "Too long: expected at most 64 characters" })
    .refine((value) => !new RegExp("[^A-Za-z0-9._-]", "u").test(value), {
      message: "Invalid string: must not contain [^A-Za-z0-9._-]",
    }),
  z
    .string()
    .min(1)
    .refine((value) => [...value].length <= 256, { message: "Too long: expected at most 256 characters" })
    .refine((value) => !new RegExp("[\\u0000-\\u001f\\u007f-\\u009f]", "u").test(value), {
      message: "Invalid string: must not contain [\\u0000-\\u001f\\u007f-\\u009f]",
    }),
);

export const HistoryLineSchema: z.ZodType<HistoryLine> = z.union([
  z.union([
    z.looseObject({
      event: z.lazy(() => ActionEventSchema).refine((value) => value !== undefined, { message: "Required" }),
      harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
      harness_id: z.union([z.string(), z.null()]).optional(),
      run_id: z
        .string()
        .min(36)
        .regex(
          new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
        )
        .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
        .refine((value) => value !== undefined, { message: "Required" }),
      schema_version: z
        .union([z.literal("1.2"), z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
        .refine((value) => value !== undefined, { message: "Required" }),
      type: z.literal("event").refine((value) => value !== undefined, { message: "Required" }),
      variant: z.union([z.string(), z.null()]).optional(),
    }),
    z.looseObject({
      event: z
        .intersection(
          z.lazy(() => ActionEventSchema),
          z.looseObject({
            timing_source: z.never().optional(),
          }),
        )
        .refine((value) => value !== undefined, { message: "Required" }),
      harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
      harness_id: z.union([z.string(), z.null()]).optional(),
      run_id: z
        .string()
        .min(36)
        .regex(
          new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
        )
        .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
        .refine((value) => value !== undefined, { message: "Required" }),
      schema_version: z
        .union([z.literal("1.0"), z.literal("1.1")])
        .refine((value) => value !== undefined, { message: "Required" }),
      type: z.literal("event").refine((value) => value !== undefined, { message: "Required" }),
      variant: z.union([z.string(), z.null()]).optional(),
    }),
  ]),
  z.intersection(
    z.intersection(
      z.intersection(
        z.union([
          z.looseObject({
            duration_ms: z
              .int()
              .gte(0)
              .refine((value) => value !== undefined, { message: "Required" }),
            error: z
              .union([
                z
                  .string()
                  .min(1)
                  .refine((value) => [...value].length <= 2048, {
                    message: "Too long: expected at most 2048 characters",
                  }),
                z.null(),
              ])
              .optional(),
            exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            failure_kind: z
              .union([z.lazy(() => FailureKindSchema), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            finished_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
            harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
            harness_id: z.union([z.string(), z.null()]).optional(),
            history_id: z
              .string()
              .min(36)
              .regex(
                new RegExp(
                  "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                  "u",
                ),
              )
              .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
              .refine((value) => value !== undefined, { message: "Required" }),
            labels: z.lazy(() => HistoryLabelsSchema).optional(),
            model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            model_ms: z
              .int()
              .gte(0)
              .refine((value) => value !== undefined, { message: "Required" }),
            name: z.string().refine((value) => value !== undefined, { message: "Required" }),
            observed_tool_ms: z.never().optional(),
            permission_mode: z
              .lazy(() => PermissionModeSchema)
              .refine((value) => value !== undefined, { message: "Required" }),
            project: z.string().refine((value) => value !== undefined, { message: "Required" }),
            prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
            schema_version: z
              .union([
                z.literal("1.0"),
                z.literal("1.1"),
                z.literal("1.2"),
                z.literal("1.3"),
                z.literal("1.4"),
                z.literal("1.5"),
                z.literal("1.6"),
              ])
              .refine((value) => value !== undefined, { message: "Required" }),
            session: z.string().refine((value) => value !== undefined, { message: "Required" }),
            session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            started_at: z
              .string()
              .min(1)
              .refine((value) => value !== undefined, { message: "Required" }),
            status: z
              .union([z.literal("ok"), z.literal("nonzero")])
              .refine((value) => value !== undefined, { message: "Required" }),
            text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            text_source: z
              .union([z.string(), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            time_to_first_token_ms: z.union([z.int().gte(0), z.null()]).optional(),
            timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
            tool_ms: z
              .int()
              .gte(0)
              .refine((value) => value !== undefined, { message: "Required" }),
            type: z.literal("run").refine((value) => value !== undefined, { message: "Required" }),
            usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
            variant: z.union([z.string(), z.null()]).optional(),
          }),
          z.looseObject({
            duration_ms: z
              .int()
              .gte(0)
              .refine((value) => value !== undefined, { message: "Required" }),
            error: z
              .union([
                z
                  .string()
                  .min(1)
                  .refine((value) => [...value].length <= 2048, {
                    message: "Too long: expected at most 2048 characters",
                  }),
                z.null(),
              ])
              .optional(),
            exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            failure_kind: z
              .union([z.lazy(() => FailureKindSchema), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            finished_at: z
              .union([z.string(), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
            harness_id: z.union([z.string(), z.null()]).optional(),
            history_id: z
              .string()
              .min(36)
              .regex(
                new RegExp(
                  "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                  "u",
                ),
              )
              .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
              .refine((value) => value !== undefined, { message: "Required" }),
            labels: z.lazy(() => HistoryLabelsSchema).optional(),
            model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            model_ms: z
              .int()
              .gte(0)
              .refine((value) => value !== undefined, { message: "Required" }),
            name: z.string().refine((value) => value !== undefined, { message: "Required" }),
            observed_tool_ms: z.never().optional(),
            permission_mode: z
              .lazy(() => PermissionModeSchema)
              .refine((value) => value !== undefined, { message: "Required" }),
            project: z.string().refine((value) => value !== undefined, { message: "Required" }),
            prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
            schema_version: z
              .union([
                z.literal("1.0"),
                z.literal("1.1"),
                z.literal("1.2"),
                z.literal("1.3"),
                z.literal("1.4"),
                z.literal("1.5"),
                z.literal("1.6"),
              ])
              .refine((value) => value !== undefined, { message: "Required" }),
            session: z.string().refine((value) => value !== undefined, { message: "Required" }),
            session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            started_at: z
              .string()
              .min(1)
              .refine((value) => value !== undefined, { message: "Required" }),
            status: z
              .union([
                z.literal("timeout"),
                z.literal("cancelled"),
                z.literal("spawn-error"),
                z.literal("skipped"),
                z.literal("planned"),
              ])
              .refine((value) => value !== undefined, { message: "Required" }),
            text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            text_source: z
              .union([z.string(), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            time_to_first_token_ms: z.union([z.int().gte(0), z.null()]).optional(),
            timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
            tool_ms: z
              .int()
              .gte(0)
              .refine((value) => value !== undefined, { message: "Required" }),
            type: z.literal("run").refine((value) => value !== undefined, { message: "Required" }),
            usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
            variant: z.union([z.string(), z.null()]).optional(),
          }),
          z.looseObject({
            duration_ms: z
              .int()
              .gte(0)
              .refine((value) => value !== undefined, { message: "Required" }),
            error: z
              .union([
                z
                  .string()
                  .min(1)
                  .refine((value) => [...value].length <= 2048, {
                    message: "Too long: expected at most 2048 characters",
                  }),
                z.null(),
              ])
              .optional(),
            exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            failure_kind: z
              .union([z.lazy(() => FailureKindSchema), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
            harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
            harness_id: z.union([z.string(), z.null()]).optional(),
            history_id: z
              .string()
              .min(36)
              .regex(
                new RegExp(
                  "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                  "u",
                ),
              )
              .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
              .refine((value) => value !== undefined, { message: "Required" }),
            labels: z.lazy(() => HistoryLabelsSchema).optional(),
            model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            model_ms: z.never().optional(),
            name: z.string().refine((value) => value !== undefined, { message: "Required" }),
            observed_tool_ms: z
              .int()
              .gte(0)
              .refine((value) => value !== undefined, { message: "Required" }),
            permission_mode: z
              .lazy(() => PermissionModeSchema)
              .refine((value) => value !== undefined, { message: "Required" }),
            project: z.string().refine((value) => value !== undefined, { message: "Required" }),
            prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
            schema_version: z
              .union([z.literal("1.2"), z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
              .refine((value) => value !== undefined, { message: "Required" }),
            session: z.string().refine((value) => value !== undefined, { message: "Required" }),
            session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            started_at: z.never().optional(),
            status: z.lazy(() => StatusSchema).refine((value) => value !== undefined, { message: "Required" }),
            text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            text_source: z
              .union([z.string(), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            time_to_first_token_ms: z.never().optional(),
            timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
            tool_ms: z.never().optional(),
            type: z.literal("run").refine((value) => value !== undefined, { message: "Required" }),
            usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
            variant: z.union([z.string(), z.null()]).optional(),
          }),
          z.looseObject({
            duration_ms: z
              .union([z.int().gte(0), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            error: z
              .union([
                z
                  .string()
                  .min(1)
                  .refine((value) => [...value].length <= 2048, {
                    message: "Too long: expected at most 2048 characters",
                  }),
                z.null(),
              ])
              .optional(),
            exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            failure_kind: z
              .union([z.lazy(() => FailureKindSchema), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
            harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
            harness_id: z.union([z.string(), z.null()]).optional(),
            history_id: z
              .string()
              .min(36)
              .regex(
                new RegExp(
                  "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                  "u",
                ),
              )
              .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
              .refine((value) => value !== undefined, { message: "Required" }),
            labels: z.lazy(() => HistoryLabelsSchema).optional(),
            model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            model_ms: z.never().optional(),
            name: z.string().refine((value) => value !== undefined, { message: "Required" }),
            observed_tool_ms: z.never().optional(),
            permission_mode: z
              .lazy(() => PermissionModeSchema)
              .refine((value) => value !== undefined, { message: "Required" }),
            project: z.string().refine((value) => value !== undefined, { message: "Required" }),
            prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
            schema_version: z
              .union([
                z.literal("1.0"),
                z.literal("1.1"),
                z.literal("1.2"),
                z.literal("1.3"),
                z.literal("1.4"),
                z.literal("1.5"),
                z.literal("1.6"),
              ])
              .refine((value) => value !== undefined, { message: "Required" }),
            session: z.string().refine((value) => value !== undefined, { message: "Required" }),
            session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            started_at: z.never().optional(),
            status: z
              .union([
                z.literal("nonzero"),
                z.literal("timeout"),
                z.literal("cancelled"),
                z.literal("spawn-error"),
                z.literal("skipped"),
              ])
              .refine((value) => value !== undefined, { message: "Required" }),
            text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            text_source: z
              .union([z.string(), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            time_to_first_token_ms: z.never().optional(),
            timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
            tool_ms: z.never().optional(),
            type: z.literal("run").refine((value) => value !== undefined, { message: "Required" }),
            usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
            variant: z.union([z.string(), z.null()]).optional(),
          }),
          z.looseObject({
            duration_ms: z
              .int()
              .gte(0)
              .refine((value) => value !== undefined, { message: "Required" }),
            error: z
              .union([
                z
                  .string()
                  .min(1)
                  .refine((value) => [...value].length <= 2048, {
                    message: "Too long: expected at most 2048 characters",
                  }),
                z.null(),
              ])
              .optional(),
            exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            failure_kind: z
              .union([z.lazy(() => FailureKindSchema), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
            harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
            harness_id: z.union([z.string(), z.null()]).optional(),
            history_id: z
              .string()
              .min(36)
              .regex(
                new RegExp(
                  "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                  "u",
                ),
              )
              .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
              .refine((value) => value !== undefined, { message: "Required" }),
            labels: z.lazy(() => HistoryLabelsSchema).optional(),
            model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            model_ms: z.never().optional(),
            name: z.string().refine((value) => value !== undefined, { message: "Required" }),
            observed_tool_ms: z.never().optional(),
            permission_mode: z
              .lazy(() => PermissionModeSchema)
              .refine((value) => value !== undefined, { message: "Required" }),
            project: z.string().refine((value) => value !== undefined, { message: "Required" }),
            prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
            schema_version: z
              .union([z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
              .refine((value) => value !== undefined, { message: "Required" }),
            session: z.string().refine((value) => value !== undefined, { message: "Required" }),
            session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            started_at: z
              .string()
              .min(1)
              .refine((value) => value !== undefined, { message: "Required" }),
            status: z
              .union([
                z.literal("nonzero"),
                z.literal("timeout"),
                z.literal("cancelled"),
                z.literal("spawn-error"),
                z.literal("skipped"),
              ])
              .refine((value) => value !== undefined, { message: "Required" }),
            text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            text_source: z
              .union([z.string(), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            time_to_first_token_ms: z.union([z.int().gte(0), z.null()]).optional(),
            timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
            tool_ms: z.never().optional(),
            type: z.literal("run").refine((value) => value !== undefined, { message: "Required" }),
            usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
            variant: z.union([z.string(), z.null()]).optional(),
          }),
          z.looseObject({
            duration_ms: z
              .union([z.int().gte(0), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            error: z
              .union([
                z
                  .string()
                  .min(1)
                  .refine((value) => [...value].length <= 2048, {
                    message: "Too long: expected at most 2048 characters",
                  }),
                z.null(),
              ])
              .optional(),
            exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            failure_kind: z
              .union([z.lazy(() => FailureKindSchema), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
            harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
            harness_id: z.union([z.string(), z.null()]).optional(),
            history_id: z
              .string()
              .min(36)
              .regex(
                new RegExp(
                  "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                  "u",
                ),
              )
              .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
              .refine((value) => value !== undefined, { message: "Required" }),
            labels: z.lazy(() => HistoryLabelsSchema).optional(),
            model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            model_ms: z.never().optional(),
            name: z.string().refine((value) => value !== undefined, { message: "Required" }),
            observed_tool_ms: z.never().optional(),
            permission_mode: z
              .lazy(() => PermissionModeSchema)
              .refine((value) => value !== undefined, { message: "Required" }),
            project: z.string().refine((value) => value !== undefined, { message: "Required" }),
            prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
            schema_version: z
              .union([
                z.literal("1.0"),
                z.literal("1.1"),
                z.literal("1.2"),
                z.literal("1.3"),
                z.literal("1.4"),
                z.literal("1.5"),
                z.literal("1.6"),
              ])
              .refine((value) => value !== undefined, { message: "Required" }),
            session: z.string().refine((value) => value !== undefined, { message: "Required" }),
            session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            started_at: z.never().optional(),
            status: z
              .union([z.literal("ok"), z.literal("planned")])
              .refine((value) => value !== undefined, { message: "Required" }),
            text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
            text_source: z
              .union([z.string(), z.null()])
              .refine((value) => value !== undefined, { message: "Required" }),
            time_to_first_token_ms: z.never().optional(),
            timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
            tool_ms: z.never().optional(),
            type: z.literal("run").refine((value) => value !== undefined, { message: "Required" }),
            usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
            variant: z.union([z.string(), z.null()]).optional(),
          }),
        ]),
        z.union([
          z.looseObject({
            error: z.null().optional(),
          }),
          z.intersection(
            z.looseObject({
              error: z.string().refine((value) => value !== undefined, { message: "Required" }),
              schema_version: z
                .union([z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
                .optional(),
            }),
            z.union([
              z.looseObject({
                status: z
                  .union([
                    z.literal("nonzero"),
                    z.literal("timeout"),
                    z.literal("cancelled"),
                    z.literal("spawn-error"),
                    z.literal("skipped"),
                  ])
                  .optional(),
              }),
              z.looseObject({
                failure_kind: z
                  .literal("tool_deferred")
                  .refine((value) => value !== undefined, { message: "Required" }),
              }),
            ]),
          ),
        ]),
      ),
      z.union([
        z.looseObject({
          status: z
            .union([
              z.literal("ok"),
              z.literal("nonzero"),
              z.literal("timeout"),
              z.literal("spawn-error"),
              z.literal("skipped"),
              z.literal("planned"),
            ])
            .optional(),
        }),
        z.looseObject({
          schema_version: z.union([z.literal("1.4"), z.literal("1.5"), z.literal("1.6")]).optional(),
        }),
      ]),
    ),
    z.intersection(
      z.union([
        z.looseObject({
          failure_kind: z
            .union([
              z.literal("auth"),
              z.literal("rate_limit"),
              z.literal("model_not_found"),
              z.literal("quota"),
              z.literal("tool_deferred"),
              z.literal("untrusted_directory"),
              z.literal("input_too_large"),
              z.literal(null),
            ])
            .optional(),
        }),
        z.looseObject({
          schema_version: z.union([z.literal("1.5"), z.literal("1.6")]).optional(),
        }),
      ]),
      z.union([
        z.looseObject({
          failure_kind: z
            .union([
              z.literal("auth"),
              z.literal("rate_limit"),
              z.literal("model_not_found"),
              z.literal("quota"),
              z.literal("session_not_found"),
              z.literal("tool_deferred"),
              z.literal(null),
            ])
            .optional(),
        }),
        z.looseObject({
          schema_version: z.literal("1.6").optional(),
        }),
      ]),
    ),
  ),
]);

export const HistoryListSchema: z.ZodType<HistoryList> = z.array(z.lazy(() => HistorySessionSummarySchema));

export const HistoryListOptionsSchema: z.ZodType<HistoryListOptions> = z.strictObject({
  allProjects: z.boolean().optional(),
  config: z.string().optional(),
  historyDir: z.string().optional(),
  noConfig: z.boolean().optional(),
  project: z.string().optional(),
  variant: z.string().optional(),
});

export const HistoryLookupSchema: z.ZodType<HistoryLookup> = z.union([
  z.lazy(() => HistoryLookupByLastSchema),
  z.lazy(() => HistoryLookupBySessionSchema),
]);

export const HistoryLookupByLastSchema: z.ZodType<HistoryLookupByLast> = z.strictObject({
  all: z.boolean().optional(),
  allProjects: z.boolean().optional(),
  config: z.string().optional(),
  historyDir: z.string().optional(),
  last: z.literal(true).refine((value) => value !== undefined, { message: "Required" }),
  noConfig: z.boolean().optional(),
  project: z.string().optional(),
  session: z.string().optional(),
});

export const HistoryLookupBySessionSchema: z.ZodType<HistoryLookupBySession> = z.strictObject({
  all: z.boolean().optional(),
  allProjects: z.boolean().optional(),
  config: z.string().optional(),
  historyDir: z.string().optional(),
  last: z.boolean().optional(),
  noConfig: z.boolean().optional(),
  project: z.string().optional(),
  session: z
    .string()
    .min(1)
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const HistoryMigrateOptionsSchema: z.ZodType<HistoryMigrateOptions> = z.strictObject({
  config: z.string().optional(),
  historyDir: z.string().optional(),
  noConfig: z.boolean().optional(),
});

export const HistoryMigrateReportSchema: z.ZodType<HistoryMigrateReport> = z.looseObject({
  files: z.array(z.lazy(() => MigrationSummarySchema)).refine((value) => value !== undefined, { message: "Required" }),
  files_processed: z
    .int()
    .gte(0)
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const HistoryRecordSchema: z.ZodType<HistoryRecord> = z.intersection(
  z.intersection(
    z.intersection(
      z.union([
        z.looseObject({
          duration_ms: z
            .int()
            .gte(0)
            .refine((value) => value !== undefined, { message: "Required" }),
          error: z
            .union([
              z
                .string()
                .min(1)
                .refine((value) => [...value].length <= 2048, {
                  message: "Too long: expected at most 2048 characters",
                }),
              z.null(),
            ])
            .optional(),
          events: z
            .union([
              z.array(
                z.union([
                  z.intersection(
                    z.lazy(() => ActionEventSchema),
                    z.looseObject({
                      duration_ms: z
                        .int()
                        .gte(0)
                        .refine((value) => value !== undefined, { message: "Required" }),
                      finished_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                      kind: z.literal("tool_call").refine((value) => value !== undefined, { message: "Required" }),
                      started_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                      status: z.literal("completed").refine((value) => value !== undefined, { message: "Required" }),
                      tool_call_id: z
                        .string()
                        .min(1)
                        .refine((value) => value !== undefined, { message: "Required" }),
                    }),
                  ),
                  z.intersection(
                    z.lazy(() => ActionEventSchema),
                    z.looseObject({
                      duration_ms: z
                        .int()
                        .gte(0)
                        .refine((value) => value !== undefined, { message: "Required" }),
                      finished_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                      kind: z.literal("tool_call").refine((value) => value !== undefined, { message: "Required" }),
                      started_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                      status: z.literal("failed").refine((value) => value !== undefined, { message: "Required" }),
                      tool_call_id: z
                        .string()
                        .min(1)
                        .refine((value) => value !== undefined, { message: "Required" }),
                    }),
                  ),
                  z.intersection(
                    z.lazy(() => ActionEventSchema),
                    z.looseObject({
                      kind: z.literal("tool_call").refine((value) => value !== undefined, { message: "Required" }),
                      started_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                      status: z.literal("timeout").refine((value) => value !== undefined, { message: "Required" }),
                      tool_call_id: z
                        .string()
                        .min(1)
                        .refine((value) => value !== undefined, { message: "Required" }),
                    }),
                  ),
                  z.intersection(
                    z.lazy(() => ActionEventSchema),
                    z.looseObject({
                      kind: z.literal("tool_call").refine((value) => value !== undefined, { message: "Required" }),
                      started_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                      status: z.literal("interrupted").refine((value) => value !== undefined, { message: "Required" }),
                      tool_call_id: z
                        .string()
                        .min(1)
                        .refine((value) => value !== undefined, { message: "Required" }),
                    }),
                  ),
                  z.intersection(
                    z.lazy(() => ActionEventSchema),
                    z.looseObject({
                      kind: z.literal("tool_result").optional(),
                    }),
                  ),
                ]),
              ),
              z.null(),
            ])
            .refine((value) => value !== undefined, { message: "Required" }),
          exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          failure_kind: z
            .union([z.lazy(() => FailureKindSchema), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          finished_at: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
          harness_id: z.string().refine((value) => value !== undefined, { message: "Required" }),
          history_id: z
            .string()
            .min(36)
            .regex(
              new RegExp(
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                "u",
              ),
            )
            .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
            .refine((value) => value !== undefined, { message: "Required" }),
          labels: z.lazy(() => HistoryLabelsSchema).optional(),
          model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          model_ms: z
            .int()
            .gte(0)
            .refine((value) => value !== undefined, { message: "Required" }),
          name: z.string().refine((value) => value !== undefined, { message: "Required" }),
          observed_tool_ms: z.never().optional(),
          permission_mode: z
            .lazy(() => PermissionModeSchema)
            .refine((value) => value !== undefined, { message: "Required" }),
          project: z.string().refine((value) => value !== undefined, { message: "Required" }),
          prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
          schema_version: z
            .union([z.literal("1.2"), z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
            .refine((value) => value !== undefined, { message: "Required" }),
          session: z.string().refine((value) => value !== undefined, { message: "Required" }),
          session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          started_at: z
            .string()
            .min(1)
            .refine((value) => value !== undefined, { message: "Required" }),
          status: z.lazy(() => StatusSchema).refine((value) => value !== undefined, { message: "Required" }),
          text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          time_to_first_token_ms: z.union([z.int().gte(0), z.null()]).optional(),
          timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
          tool_ms: z
            .int()
            .gte(0)
            .refine((value) => value !== undefined, { message: "Required" }),
          usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
          variant: z.union([z.string(), z.null()]).optional(),
        }),
        z.looseObject({
          duration_ms: z
            .int()
            .gte(0)
            .refine((value) => value !== undefined, { message: "Required" }),
          error: z
            .union([
              z
                .string()
                .min(1)
                .refine((value) => [...value].length <= 2048, {
                  message: "Too long: expected at most 2048 characters",
                }),
              z.null(),
            ])
            .optional(),
          events: z
            .union([
              z.array(
                z.intersection(
                  z.union([
                    z.intersection(
                      z.lazy(() => ActionEventSchema),
                      z.looseObject({
                        duration_ms: z
                          .int()
                          .gte(0)
                          .refine((value) => value !== undefined, { message: "Required" }),
                        finished_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                        kind: z.literal("tool_call").refine((value) => value !== undefined, { message: "Required" }),
                        started_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                        status: z.literal("completed").refine((value) => value !== undefined, { message: "Required" }),
                        tool_call_id: z
                          .string()
                          .min(1)
                          .refine((value) => value !== undefined, { message: "Required" }),
                      }),
                    ),
                    z.intersection(
                      z.lazy(() => ActionEventSchema),
                      z.looseObject({
                        duration_ms: z
                          .int()
                          .gte(0)
                          .refine((value) => value !== undefined, { message: "Required" }),
                        finished_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                        kind: z.literal("tool_call").refine((value) => value !== undefined, { message: "Required" }),
                        started_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                        status: z.literal("failed").refine((value) => value !== undefined, { message: "Required" }),
                        tool_call_id: z
                          .string()
                          .min(1)
                          .refine((value) => value !== undefined, { message: "Required" }),
                      }),
                    ),
                    z.intersection(
                      z.lazy(() => ActionEventSchema),
                      z.looseObject({
                        kind: z.literal("tool_call").refine((value) => value !== undefined, { message: "Required" }),
                        started_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                        status: z.literal("timeout").refine((value) => value !== undefined, { message: "Required" }),
                        tool_call_id: z
                          .string()
                          .min(1)
                          .refine((value) => value !== undefined, { message: "Required" }),
                      }),
                    ),
                    z.intersection(
                      z.lazy(() => ActionEventSchema),
                      z.looseObject({
                        kind: z.literal("tool_call").refine((value) => value !== undefined, { message: "Required" }),
                        started_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
                        status: z
                          .literal("interrupted")
                          .refine((value) => value !== undefined, { message: "Required" }),
                        tool_call_id: z
                          .string()
                          .min(1)
                          .refine((value) => value !== undefined, { message: "Required" }),
                      }),
                    ),
                    z.intersection(
                      z.lazy(() => ActionEventSchema),
                      z.looseObject({
                        kind: z.literal("tool_result").optional(),
                      }),
                    ),
                  ]),
                  z.looseObject({
                    timing_source: z.never().optional(),
                  }),
                ),
              ),
              z.null(),
            ])
            .refine((value) => value !== undefined, { message: "Required" }),
          exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          failure_kind: z
            .union([z.lazy(() => FailureKindSchema), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          finished_at: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
          harness_id: z.string().optional(),
          history_id: z
            .string()
            .min(36)
            .regex(
              new RegExp(
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                "u",
              ),
            )
            .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
            .refine((value) => value !== undefined, { message: "Required" }),
          labels: z.lazy(() => HistoryLabelsSchema).optional(),
          model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          model_ms: z
            .int()
            .gte(0)
            .refine((value) => value !== undefined, { message: "Required" }),
          name: z.string().refine((value) => value !== undefined, { message: "Required" }),
          observed_tool_ms: z.never().optional(),
          permission_mode: z
            .lazy(() => PermissionModeSchema)
            .refine((value) => value !== undefined, { message: "Required" }),
          project: z.string().refine((value) => value !== undefined, { message: "Required" }),
          prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
          schema_version: z
            .union([z.literal("1.0"), z.literal("1.1")])
            .refine((value) => value !== undefined, { message: "Required" }),
          session: z.string().refine((value) => value !== undefined, { message: "Required" }),
          session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          started_at: z
            .string()
            .min(1)
            .refine((value) => value !== undefined, { message: "Required" }),
          status: z.lazy(() => StatusSchema).refine((value) => value !== undefined, { message: "Required" }),
          text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          time_to_first_token_ms: z.union([z.int().gte(0), z.null()]).optional(),
          timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
          tool_ms: z
            .int()
            .gte(0)
            .refine((value) => value !== undefined, { message: "Required" }),
          usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
          variant: z.union([z.string(), z.null()]).optional(),
        }),
        z.looseObject({
          duration_ms: z
            .union([z.int().gte(0), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          error: z
            .union([
              z
                .string()
                .min(1)
                .refine((value) => [...value].length <= 2048, {
                  message: "Too long: expected at most 2048 characters",
                }),
              z.null(),
            ])
            .optional(),
          events: z
            .union([
              z.array(
                z.looseObject({
                  duration_ms: z.null().optional(),
                  finished_at: z.null().optional(),
                  index: z
                    .int()
                    .gte(0)
                    .refine((value) => value !== undefined, { message: "Required" }),
                  input: z.unknown().refine((value) => value !== undefined, { message: "Required" }),
                  kind: z.string().refine((value) => value !== undefined, { message: "Required" }),
                  name: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
                  output: z
                    .union([z.string(), z.null()])
                    .refine((value) => value !== undefined, { message: "Required" }),
                  started_at: z.null().optional(),
                  status: z.null().optional(),
                  tool_call_id: z.union([z.string(), z.null()]).optional(),
                }),
              ),
              z.null(),
            ])
            .refine((value) => value !== undefined, { message: "Required" }),
          exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          failure_kind: z
            .union([z.lazy(() => FailureKindSchema), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
          harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
          harness_id: z.string().refine((value) => value !== undefined, { message: "Required" }),
          history_id: z
            .string()
            .min(36)
            .regex(
              new RegExp(
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                "u",
              ),
            )
            .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
            .refine((value) => value !== undefined, { message: "Required" }),
          labels: z.lazy(() => HistoryLabelsSchema).optional(),
          model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          model_ms: z.never().optional(),
          name: z.string().refine((value) => value !== undefined, { message: "Required" }),
          observed_tool_ms: z.never().optional(),
          permission_mode: z
            .lazy(() => PermissionModeSchema)
            .refine((value) => value !== undefined, { message: "Required" }),
          project: z.string().refine((value) => value !== undefined, { message: "Required" }),
          prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
          schema_version: z
            .union([z.literal("1.2"), z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
            .refine((value) => value !== undefined, { message: "Required" }),
          session: z.string().refine((value) => value !== undefined, { message: "Required" }),
          session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          started_at: z.never().optional(),
          status: z
            .union([z.literal("ok"), z.literal("planned")])
            .refine((value) => value !== undefined, { message: "Required" }),
          text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          time_to_first_token_ms: z.never().optional(),
          timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
          tool_ms: z.never().optional(),
          usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
          variant: z.union([z.string(), z.null()]).optional(),
        }),
        z.looseObject({
          duration_ms: z
            .int()
            .gte(0)
            .refine((value) => value !== undefined, { message: "Required" }),
          error: z
            .union([
              z
                .string()
                .min(1)
                .refine((value) => [...value].length <= 2048, {
                  message: "Too long: expected at most 2048 characters",
                }),
              z.null(),
            ])
            .optional(),
          events: z
            .union([z.array(z.lazy(() => ActionEventSchema)), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          failure_kind: z
            .union([z.lazy(() => FailureKindSchema), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
          harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
          harness_id: z.string().refine((value) => value !== undefined, { message: "Required" }),
          history_id: z
            .string()
            .min(36)
            .regex(
              new RegExp(
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                "u",
              ),
            )
            .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
            .refine((value) => value !== undefined, { message: "Required" }),
          labels: z.lazy(() => HistoryLabelsSchema).optional(),
          model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          model_ms: z.never().optional(),
          name: z.string().refine((value) => value !== undefined, { message: "Required" }),
          observed_tool_ms: z
            .int()
            .gte(0)
            .refine((value) => value !== undefined, { message: "Required" }),
          permission_mode: z
            .lazy(() => PermissionModeSchema)
            .refine((value) => value !== undefined, { message: "Required" }),
          project: z.string().refine((value) => value !== undefined, { message: "Required" }),
          prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
          schema_version: z
            .union([z.literal("1.2"), z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
            .refine((value) => value !== undefined, { message: "Required" }),
          session: z.string().refine((value) => value !== undefined, { message: "Required" }),
          session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          started_at: z.never().optional(),
          status: z.lazy(() => StatusSchema).refine((value) => value !== undefined, { message: "Required" }),
          text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          time_to_first_token_ms: z.never().optional(),
          timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
          tool_ms: z.never().optional(),
          usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
          variant: z.union([z.string(), z.null()]).optional(),
        }),
        z.looseObject({
          duration_ms: z
            .union([z.int().gte(0), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          error: z
            .union([
              z
                .string()
                .min(1)
                .refine((value) => [...value].length <= 2048, {
                  message: "Too long: expected at most 2048 characters",
                }),
              z.null(),
            ])
            .optional(),
          events: z
            .union([z.array(z.lazy(() => ActionEventSchema)), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          failure_kind: z
            .union([z.lazy(() => FailureKindSchema), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
          harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
          harness_id: z.string().refine((value) => value !== undefined, { message: "Required" }),
          history_id: z
            .string()
            .min(36)
            .regex(
              new RegExp(
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                "u",
              ),
            )
            .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
            .refine((value) => value !== undefined, { message: "Required" }),
          labels: z.lazy(() => HistoryLabelsSchema).optional(),
          model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          model_ms: z.never().optional(),
          name: z.string().refine((value) => value !== undefined, { message: "Required" }),
          observed_tool_ms: z.never().optional(),
          permission_mode: z
            .lazy(() => PermissionModeSchema)
            .refine((value) => value !== undefined, { message: "Required" }),
          project: z.string().refine((value) => value !== undefined, { message: "Required" }),
          prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
          schema_version: z
            .union([z.literal("1.2"), z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
            .refine((value) => value !== undefined, { message: "Required" }),
          session: z.string().refine((value) => value !== undefined, { message: "Required" }),
          session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          started_at: z.never().optional(),
          status: z
            .union([
              z.literal("nonzero"),
              z.literal("timeout"),
              z.literal("cancelled"),
              z.literal("spawn-error"),
              z.literal("skipped"),
            ])
            .refine((value) => value !== undefined, { message: "Required" }),
          text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          time_to_first_token_ms: z.never().optional(),
          timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
          tool_ms: z.never().optional(),
          usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
          variant: z.union([z.string(), z.null()]).optional(),
        }),
        z.looseObject({
          duration_ms: z
            .int()
            .gte(0)
            .refine((value) => value !== undefined, { message: "Required" }),
          error: z
            .union([
              z
                .string()
                .min(1)
                .refine((value) => [...value].length <= 2048, {
                  message: "Too long: expected at most 2048 characters",
                }),
              z.null(),
            ])
            .optional(),
          events: z
            .union([z.array(z.lazy(() => ActionEventSchema)), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          failure_kind: z
            .union([z.lazy(() => FailureKindSchema), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
          harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
          harness_id: z.string().refine((value) => value !== undefined, { message: "Required" }),
          history_id: z
            .string()
            .min(36)
            .regex(
              new RegExp(
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                "u",
              ),
            )
            .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
            .refine((value) => value !== undefined, { message: "Required" }),
          labels: z.lazy(() => HistoryLabelsSchema).optional(),
          model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          model_ms: z.never().optional(),
          name: z.string().refine((value) => value !== undefined, { message: "Required" }),
          observed_tool_ms: z.never().optional(),
          permission_mode: z
            .lazy(() => PermissionModeSchema)
            .refine((value) => value !== undefined, { message: "Required" }),
          project: z.string().refine((value) => value !== undefined, { message: "Required" }),
          prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
          schema_version: z
            .union([z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
            .refine((value) => value !== undefined, { message: "Required" }),
          session: z.string().refine((value) => value !== undefined, { message: "Required" }),
          session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          started_at: z
            .string()
            .min(1)
            .refine((value) => value !== undefined, { message: "Required" }),
          status: z
            .union([
              z.literal("nonzero"),
              z.literal("timeout"),
              z.literal("cancelled"),
              z.literal("spawn-error"),
              z.literal("skipped"),
            ])
            .refine((value) => value !== undefined, { message: "Required" }),
          text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          time_to_first_token_ms: z.union([z.int().gte(0), z.null()]).optional(),
          timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
          tool_ms: z.never().optional(),
          usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
          variant: z.union([z.string(), z.null()]).optional(),
        }),
        z.looseObject({
          duration_ms: z
            .union([z.int().gte(0), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          error: z
            .union([
              z
                .string()
                .min(1)
                .refine((value) => [...value].length <= 2048, {
                  message: "Too long: expected at most 2048 characters",
                }),
              z.null(),
            ])
            .optional(),
          events: z
            .union([
              z.array(
                z.intersection(
                  z.lazy(() => ActionEventSchema),
                  z.looseObject({
                    timing_source: z.never().optional(),
                  }),
                ),
              ),
              z.null(),
            ])
            .refine((value) => value !== undefined, { message: "Required" }),
          exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          failure_kind: z
            .union([z.lazy(() => FailureKindSchema), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
          harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
          harness_id: z.string().optional(),
          history_id: z
            .string()
            .min(36)
            .regex(
              new RegExp(
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                "u",
              ),
            )
            .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
            .refine((value) => value !== undefined, { message: "Required" }),
          labels: z.lazy(() => HistoryLabelsSchema).optional(),
          model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          model_ms: z.never().optional(),
          name: z.string().refine((value) => value !== undefined, { message: "Required" }),
          observed_tool_ms: z.never().optional(),
          permission_mode: z
            .lazy(() => PermissionModeSchema)
            .refine((value) => value !== undefined, { message: "Required" }),
          project: z.string().refine((value) => value !== undefined, { message: "Required" }),
          prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
          schema_version: z
            .union([z.literal("1.0"), z.literal("1.1")])
            .refine((value) => value !== undefined, { message: "Required" }),
          session: z.string().refine((value) => value !== undefined, { message: "Required" }),
          session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          started_at: z.never().optional(),
          status: z
            .union([
              z.literal("nonzero"),
              z.literal("timeout"),
              z.literal("cancelled"),
              z.literal("spawn-error"),
              z.literal("skipped"),
            ])
            .refine((value) => value !== undefined, { message: "Required" }),
          text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          time_to_first_token_ms: z.never().optional(),
          timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
          tool_ms: z.never().optional(),
          usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
          variant: z.union([z.string(), z.null()]).optional(),
        }),
        z.looseObject({
          duration_ms: z
            .union([z.int().gte(0), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          error: z
            .union([
              z
                .string()
                .min(1)
                .refine((value) => [...value].length <= 2048, {
                  message: "Too long: expected at most 2048 characters",
                }),
              z.null(),
            ])
            .optional(),
          events: z
            .union([
              z.array(
                z.intersection(
                  z.looseObject({
                    duration_ms: z.null().optional(),
                    finished_at: z.null().optional(),
                    index: z
                      .int()
                      .gte(0)
                      .refine((value) => value !== undefined, { message: "Required" }),
                    input: z.unknown().refine((value) => value !== undefined, { message: "Required" }),
                    kind: z.string().refine((value) => value !== undefined, { message: "Required" }),
                    name: z
                      .union([z.string(), z.null()])
                      .refine((value) => value !== undefined, { message: "Required" }),
                    output: z
                      .union([z.string(), z.null()])
                      .refine((value) => value !== undefined, { message: "Required" }),
                    started_at: z.null().optional(),
                    status: z.null().optional(),
                    tool_call_id: z.union([z.string(), z.null()]).optional(),
                  }),
                  z.looseObject({
                    timing_source: z.never().optional(),
                  }),
                ),
              ),
              z.null(),
            ])
            .refine((value) => value !== undefined, { message: "Required" }),
          exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          failure_kind: z
            .union([z.lazy(() => FailureKindSchema), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
          harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
          harness_id: z.string().optional(),
          history_id: z
            .string()
            .min(36)
            .regex(
              new RegExp(
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                "u",
              ),
            )
            .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
            .refine((value) => value !== undefined, { message: "Required" }),
          labels: z.lazy(() => HistoryLabelsSchema).optional(),
          model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          model_ms: z.never().optional(),
          name: z.string().refine((value) => value !== undefined, { message: "Required" }),
          observed_tool_ms: z.never().optional(),
          permission_mode: z
            .lazy(() => PermissionModeSchema)
            .refine((value) => value !== undefined, { message: "Required" }),
          project: z.string().refine((value) => value !== undefined, { message: "Required" }),
          prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
          schema_version: z
            .union([z.literal("1.0"), z.literal("1.1")])
            .refine((value) => value !== undefined, { message: "Required" }),
          session: z.string().refine((value) => value !== undefined, { message: "Required" }),
          session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          started_at: z.never().optional(),
          status: z
            .union([z.literal("ok"), z.literal("planned")])
            .refine((value) => value !== undefined, { message: "Required" }),
          text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          time_to_first_token_ms: z.never().optional(),
          timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
          tool_ms: z.never().optional(),
          usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
          variant: z.union([z.string(), z.null()]).optional(),
        }),
        z.looseObject({
          duration_ms: z
            .union([z.int().gte(0), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          error: z
            .union([
              z
                .string()
                .min(1)
                .refine((value) => [...value].length <= 2048, {
                  message: "Too long: expected at most 2048 characters",
                }),
              z.null(),
            ])
            .optional(),
          events: z
            .union([
              z.array(
                z.looseObject({
                  index: z
                    .int()
                    .gte(0)
                    .refine((value) => value !== undefined, { message: "Required" }),
                  input: z.unknown().refine((value) => value !== undefined, { message: "Required" }),
                  kind: z.string().refine((value) => value !== undefined, { message: "Required" }),
                  name: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
                  output: z
                    .union([z.string(), z.null()])
                    .refine((value) => value !== undefined, { message: "Required" }),
                }),
              ),
              z.null(),
            ])
            .refine((value) => value !== undefined, { message: "Required" }),
          exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          failure_kind: z
            .union([z.lazy(() => FailureKindSchema), z.null()])
            .refine((value) => value !== undefined, { message: "Required" }),
          finished_at: z.union([z.string(), z.null()]).optional(),
          harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
          harness_id: z.string().refine((value) => value !== undefined, { message: "Required" }),
          history_id: z
            .string()
            .min(36)
            .regex(
              new RegExp(
                "^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$",
                "u",
              ),
            )
            .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
            .refine((value) => value !== undefined, { message: "Required" }),
          labels: z.lazy(() => HistoryLabelsSchema).optional(),
          model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          model_ms: z.union([z.int().gte(0), z.null()]).optional(),
          name: z.string().refine((value) => value !== undefined, { message: "Required" }),
          observed_tool_ms: z.union([z.int().gte(0), z.null()]).optional(),
          permission_mode: z
            .lazy(() => PermissionModeSchema)
            .refine((value) => value !== undefined, { message: "Required" }),
          project: z.string().refine((value) => value !== undefined, { message: "Required" }),
          prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
          schema_version: z
            .union([z.literal("0.1"), z.literal("0.2")])
            .refine((value) => value !== undefined, { message: "Required" }),
          session: z.string().refine((value) => value !== undefined, { message: "Required" }),
          session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          started_at: z.union([z.string(), z.null()]).optional(),
          status: z.lazy(() => StatusSchema).refine((value) => value !== undefined, { message: "Required" }),
          text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
          time_to_first_token_ms: z.union([z.int().gte(0), z.null()]).optional(),
          timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
          tool_ms: z.union([z.int().gte(0), z.null()]).optional(),
          usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
          variant: z.union([z.string(), z.null()]).optional(),
        }),
      ]),
      z.union([
        z.looseObject({
          error: z.null().optional(),
        }),
        z.intersection(
          z.looseObject({
            error: z.string().refine((value) => value !== undefined, { message: "Required" }),
            schema_version: z
              .union([z.literal("1.3"), z.literal("1.4"), z.literal("1.5"), z.literal("1.6")])
              .optional(),
          }),
          z.union([
            z.looseObject({
              status: z
                .union([
                  z.literal("nonzero"),
                  z.literal("timeout"),
                  z.literal("cancelled"),
                  z.literal("spawn-error"),
                  z.literal("skipped"),
                ])
                .optional(),
            }),
            z.looseObject({
              failure_kind: z.literal("tool_deferred").refine((value) => value !== undefined, { message: "Required" }),
            }),
          ]),
        ),
      ]),
    ),
    z.union([
      z.looseObject({
        status: z
          .union([
            z.literal("ok"),
            z.literal("nonzero"),
            z.literal("timeout"),
            z.literal("spawn-error"),
            z.literal("skipped"),
            z.literal("planned"),
          ])
          .optional(),
      }),
      z.looseObject({
        schema_version: z.union([z.literal("1.4"), z.literal("1.5"), z.literal("1.6")]).optional(),
      }),
    ]),
  ),
  z.intersection(
    z.union([
      z.looseObject({
        failure_kind: z
          .union([
            z.literal("auth"),
            z.literal("rate_limit"),
            z.literal("model_not_found"),
            z.literal("quota"),
            z.literal("tool_deferred"),
            z.literal("untrusted_directory"),
            z.literal("input_too_large"),
            z.literal(null),
          ])
          .optional(),
      }),
      z.looseObject({
        schema_version: z.union([z.literal("1.5"), z.literal("1.6")]).optional(),
      }),
    ]),
    z.union([
      z.looseObject({
        failure_kind: z
          .union([
            z.literal("auth"),
            z.literal("rate_limit"),
            z.literal("model_not_found"),
            z.literal("quota"),
            z.literal("session_not_found"),
            z.literal("tool_deferred"),
            z.literal(null),
          ])
          .optional(),
      }),
      z.looseObject({
        schema_version: z.literal("1.6").optional(),
      }),
    ]),
  ),
);

export const HistoryRecordsSchema: z.ZodType<HistoryRecords> = z.array(z.lazy(() => HistoryRecordSchema));

export const HistorySessionSummarySchema: z.ZodType<HistorySessionSummary> = z.looseObject({
  harnesses: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
  id: z.string().refine((value) => value !== undefined, { message: "Required" }),
  labels: z.lazy(() => HistoryLabelsSchema).optional(),
  name: z.string().refine((value) => value !== undefined, { message: "Required" }),
  path: z.string().refine((value) => value !== undefined, { message: "Required" }),
  project: z.string().refine((value) => value !== undefined, { message: "Required" }),
  record_count: z
    .int()
    .gte(0)
    .refine((value) => value !== undefined, { message: "Required" }),
  started: z.string().refine((value) => value !== undefined, { message: "Required" }),
});

export const HistoryStreamEnvelopeSchema: z.ZodType<HistoryStreamEnvelope> = z.union([
  z.looseObject({
    record: z.lazy(() => HistoryRecordSchema).refine((value) => value !== undefined, { message: "Required" }),
    type: z.literal("record").refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    line: z.lazy(() => HistoryEventLineSchema).refine((value) => value !== undefined, { message: "Required" }),
    type: z.literal("event").refine((value) => value !== undefined, { message: "Required" }),
  }),
]);

export const HistoryWatchOptionsSchema: z.ZodType<HistoryWatchOptions> = z.strictObject({
  after: z
    .string()
    .min(36)
    .regex(
      new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
    )
    .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
    .optional(),
  allProjects: z.boolean().optional(),
  config: z.string().optional(),
  events: z.boolean().optional(),
  historyDir: z.string().optional(),
  labels: z.lazy(() => HistoryLabelsSchema).optional(),
  noConfig: z.boolean().optional(),
  project: z.string().optional(),
  variant: z.string().optional(),
});

export const HookEntrySchema: z.ZodType<HookEntry> = z.strictObject({
  command: z.string().refine((value) => value !== undefined, { message: "Required" }),
  harnesses: z.union([z.array(z.string()), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  matcher: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  plugin_name: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  timeout: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const HookFileResultSchema: z.ZodType<HookFileResult> = z.looseObject({
  file: z.string().refine((value) => value !== undefined, { message: "Required" }),
  status: z.lazy(() => FileStatusSchema).refine((value) => value !== undefined, { message: "Required" }),
});

export const IdentitySelectorSchema: z.ZodType<IdentitySelector> = z.union([
  z.looseObject({
    env: z.string().refine((value) => value !== undefined, { message: "Required" }),
    kind: z.literal("env_path").refine((value) => value !== undefined, { message: "Required" }),
    path: z.string().refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    env: z.string().refine((value) => value !== undefined, { message: "Required" }),
    kind: z.literal("env_secret").refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    kind: z.literal("ambient").refine((value) => value !== undefined, { message: "Required" }),
  }),
]);

export const InitOptionsSchema: z.ZodType<InitOptions> = z.strictObject({
  force: z.boolean().optional(),
  path: z.string().optional(),
});

export const InterruptOptionsSchema: z.ZodType<InterruptOptions> = z.strictObject({
  cwd: z.string().optional(),
  input: z.string().optional(),
  session: z
    .string()
    .min(1)
    .refine((value) => value !== undefined, { message: "Required" }),
  sessionDir: z.string().optional(),
});

export const InterruptResponseSchema: z.ZodType<InterruptResponse> = z.union([
  z.looseObject({
    error: z.never().optional(),
    mechanism: z.lazy(() => ControlShapeSchema).refine((value) => value !== undefined, { message: "Required" }),
    ok: z.literal(true).refine((value) => value !== undefined, { message: "Required" }),
    reason: z.never().optional(),
    redirected: z.boolean().optional(),
    v: z.literal(2).refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    error: z.string().refine((value) => value !== undefined, { message: "Required" }),
    mechanism: z.never().optional(),
    ok: z.literal(false).refine((value) => value !== undefined, { message: "Required" }),
    reason: z.lazy(() => ControlReasonSchema).refine((value) => value !== undefined, { message: "Required" }),
    redirected: z.literal(false).optional(),
    v: z.literal(2).refine((value) => value !== undefined, { message: "Required" }),
  }),
]);

export const ListReportSchema: z.ZodType<ListReport> = z.looseObject({
  harnesses: z.array(z.lazy(() => HarnessInfoSchema)).refine((value) => value !== undefined, { message: "Required" }),
  schema_version: z.string().refine((value) => value !== undefined, { message: "Required" }),
});

export const MigrationSummarySchema: z.ZodType<MigrationSummary> = z.looseObject({
  already_current: z
    .int()
    .gte(0)
    .refine((value) => value !== undefined, { message: "Required" }),
  path: z.string().refine((value) => value !== undefined, { message: "Required" }),
  records_migrated: z
    .int()
    .gte(0)
    .refine((value) => value !== undefined, { message: "Required" }),
  skipped: z
    .int()
    .gte(0)
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const MockOptionsSchema: z.ZodType<MockOptions> = z.strictObject({
  event: z.string().refine((value) => value !== undefined, { message: "Required" }),
  harness: z
    .string()
    .min(1)
    .refine((value) => value !== undefined, { message: "Required" }),
  rules: z.string().optional(),
  spyFile: z.string().optional(),
});

export const ModeHeadlessSchema: z.ZodType<ModeHeadless> = z.union([z.literal("clean"), z.literal("hangs")]);

export const ModeInfoSchema: z.ZodType<ModeInfo> = z.looseObject({
  headless: z.lazy(() => ModeHeadlessSchema).refine((value) => value !== undefined, { message: "Required" }),
  mode: z.lazy(() => PermissionModeSchema).refine((value) => value !== undefined, { message: "Required" }),
});

export const OutputFormatSchema: z.ZodType<OutputFormat> = z.union([
  z.literal("text"),
  z.literal("json"),
  z.literal("stream-json"),
]);

export const PermissionModeSchema: z.ZodType<PermissionMode> = z.union([
  z.literal("read-only"),
  z.literal("plan"),
  z.literal("default"),
  z.literal("edit"),
  z.literal("auto"),
  z.literal("bypass"),
]);

export const QuotaAmountSchema: z.ZodType<QuotaAmount> = z.int().gte(0);

export const QuotaCountersSchema: z.ZodType<QuotaCounters> = z.looseObject({
  entitlement: z.lazy(() => QuotaAmountSchema).refine((value) => value !== undefined, { message: "Required" }),
  has_quota: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  overage_permitted: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  remaining: z.int().refine((value) => value !== undefined, { message: "Required" }),
  unit: z.lazy(() => QuotaUnitSchema).refine((value) => value !== undefined, { message: "Required" }),
  used: z.lazy(() => QuotaAmountSchema).refine((value) => value !== undefined, { message: "Required" }),
});

export const QuotaUnitSchema: z.ZodType<QuotaUnit> = z.union([z.literal("ai_credits"), z.literal("unspecified")]);

export const RunModeSchema: z.ZodType<RunMode> = z.union([z.literal("parallel"), z.literal("fallback")]);

export const RunOptionsSchema: z.ZodType<RunOptions> = z.strictObject({
  all: z.boolean().optional(),
  batchPrompts: z.array(z.string()).optional(),
  batchStrategy: z.lazy(() => BatchStrategySchema).optional(),
  bins: z.record(z.string(), z.string()).optional(),
  config: z.string().optional(),
  control: z.boolean().optional(),
  cwd: z.string().optional(),
  env: z.record(z.string(), z.string()).optional(),
  events: z.boolean().optional(),
  exclude: z.array(z.string()).optional(),
  fork: z.boolean().optional(),
  harnesses: z.array(z.string()).optional(),
  history: z.boolean().optional(),
  historyDir: z.string().optional(),
  historyLabels: z.lazy(() => HistoryLabelsSchema).optional(),
  historyName: z.string().optional(),
  maxParallel: z.int().gte(0).optional(),
  mockHarnesses: z.array(z.string()).optional(),
  mockRules: z.string().optional(),
  mode: z.lazy(() => PermissionModeSchema).optional(),
  models: z.array(z.string()).optional(),
  noConfig: z.boolean().optional(),
  noHistory: z.boolean().optional(),
  outputDir: z.string().optional(),
  outputFormat: z.lazy(() => OutputFormatSchema).optional(),
  passthrough: z.array(z.string()).optional(),
  permitPrompts: z.boolean().optional(),
  printCommand: z.boolean().optional(),
  prompt: z
    .string()
    .min(1)
    .refine((value) => value !== undefined, { message: "Required" }),
  promptFiles: z.array(z.string()).optional(),
  reasoning: z.string().optional(),
  requireAvailable: z.boolean().optional(),
  resume: z.string().optional(),
  runMode: z.lazy(() => RunModeSchema).optional(),
  schema: z.string().optional(),
  schemaMaxRetries: z.int().gte(0).optional(),
  session: z.string().optional(),
  sessionDir: z.string().optional(),
  spyFile: z.string().optional(),
  system: z.string().optional(),
  systemFile: z.string().optional(),
  timeoutSeconds: z.int().gte(0).optional(),
});

export const RunReportSchema: z.ZodType<RunReport> = z.looseObject({
  batch: z
    .union([z.lazy(() => BatchReportSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  bypass_permissions: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  config_files: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
  control: z
    .union([z.lazy(() => ControlReportSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  dry_run: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  fallback: z
    .union([z.lazy(() => FallbackReportSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  fork: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  history_file: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  mock_rules: z.unknown().refine((value) => value !== undefined, { message: "Required" }),
  model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  models: z.union([z.array(z.string()), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  oneharness_version: z.string().refine((value) => value !== undefined, { message: "Required" }),
  permission_mode: z.lazy(() => PermissionModeSchema).refine((value) => value !== undefined, { message: "Required" }),
  prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
  results: z.array(z.lazy(() => RunResultSchema)).refine((value) => value !== undefined, { message: "Required" }),
  resume: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  schema: z.unknown().refine((value) => value !== undefined, { message: "Required" }),
  schema_max_retries: z
    .union([z.int().gte(0), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  schema_version: z.string().refine((value) => value !== undefined, { message: "Required" }),
  session: z
    .union([z.lazy(() => SessionReportSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  spy_file: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const RunResultSchema: z.ZodType<RunResult> = z.looseObject({
  available: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  bin: z.string().refine((value) => value !== undefined, { message: "Required" }),
  command: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
  duration_ms: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  error: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  events: z
    .union([z.array(z.lazy(() => ActionEventSchema)), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  events_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  failure_kind: z
    .union([z.lazy(() => FailureKindSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  failure_kind_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
  harness_id: z.string().refine((value) => value !== undefined, { message: "Required" }),
  model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  output_format: z.lazy(() => OutputFormatSchema).refine((value) => value !== undefined, { message: "Required" }),
  prompt: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  schema_attempts: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  schema_error: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  schema_valid: z.union([z.boolean(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  status: z.lazy(() => StatusSchema).refine((value) => value !== undefined, { message: "Required" }),
  stderr: z.string().refine((value) => value !== undefined, { message: "Required" }),
  stdout: z.string().refine((value) => value !== undefined, { message: "Required" }),
  structured: z.unknown().refine((value) => value !== undefined, { message: "Required" }),
  telemetry: z
    .union([z.lazy(() => ExecutionTelemetrySchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
  usage_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  variant: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const RunStreamEnvelopeSchema: z.ZodType<RunStreamEnvelope> = z.union([
  z.looseObject({
    event: z.lazy(() => ActionEventSchema).refine((value) => value !== undefined, { message: "Required" }),
    type: z.literal("event").refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    report: z.lazy(() => RunReportSchema).refine((value) => value !== undefined, { message: "Required" }),
    type: z.literal("result").refine((value) => value !== undefined, { message: "Required" }),
  }),
]);

export const SessionPhaseSchema: z.ZodType<SessionPhase> = z.union([z.literal("create"), z.literal("continue")]);

export const SessionReportSchema: z.ZodType<SessionReport> = z.looseObject({
  name: z.string().refine((value) => value !== undefined, { message: "Required" }),
  phase: z.lazy(() => SessionPhaseSchema).refine((value) => value !== undefined, { message: "Required" }),
  store_file: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  token: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const StatusSchema: z.ZodType<Status> = z.union([
  z.literal("ok"),
  z.literal("nonzero"),
  z.literal("timeout"),
  z.literal("cancelled"),
  z.literal("spawn-error"),
  z.literal("skipped"),
  z.literal("planned"),
]);

export const SyncOptionsSchema: z.ZodType<SyncOptions> = z.strictObject({
  check: z.boolean().optional(),
  config: z.string().optional(),
  cwd: z.string().optional(),
  global: z.boolean().optional(),
  harnesses: z.array(z.string()).optional(),
  noConfig: z.boolean().optional(),
});

export const SyncReportSchema: z.ZodType<SyncReport> = z.looseObject({
  check: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  config_files: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
  results: z.array(z.lazy(() => SyncResultSchema)).refine((value) => value !== undefined, { message: "Required" }),
  schema_version: z.string().refine((value) => value !== undefined, { message: "Required" }),
});

export const SyncResultSchema: z.ZodType<SyncResult> = z.looseObject({
  file: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
  hooks: z.array(z.lazy(() => HookFileResultSchema)).refine((value) => value !== undefined, { message: "Required" }),
  status: z.lazy(() => SyncStatusSchema).refine((value) => value !== undefined, { message: "Required" }),
  unmapped: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
});

export const SyncStatusSchema: z.ZodType<SyncStatus> = z.union([
  z.literal("created"),
  z.literal("updated"),
  z.literal("unchanged"),
  z.literal("skipped"),
]);

export const TimingSourceSchema: z.ZodType<TimingSource> = z.union([
  z.literal("provider_measured"),
  z.literal("stdout_observed"),
]);

export const ToolCallStatusSchema: z.ZodType<ToolCallStatus> = z.union([
  z.literal("completed"),
  z.literal("failed"),
  z.literal("timeout"),
  z.literal("interrupted"),
]);

export const UnavailableReasonSchema: z.ZodType<UnavailableReason> = z.union([
  z.literal("api_key_auth"),
  z.literal("not_logged_in"),
  z.literal("no_windows_reported"),
  z.literal("no_plan_quota"),
  z.literal("no_headroom_reader"),
]);

export const UnknownReasonSchema: z.ZodType<UnknownReason> = z.union([
  z.looseObject({
    kind: z.literal("unprobed").refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    kind: z.literal("probe_failed").refine((value) => value !== undefined, { message: "Required" }),
    message: z.string().refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    bin: z.string().refine((value) => value !== undefined, { message: "Required" }),
    kind: z.literal("binary_missing").refine((value) => value !== undefined, { message: "Required" }),
  }),
]);

export const UsageSchema: z.ZodType<Usage> = z.looseObject({
  cache_read_tokens: z
    .union([z.int().gte(0), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  cache_write_tokens: z
    .union([z.int().gte(0), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  cost_usd: z.union([z.number(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  input_tokens: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  output_tokens: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const UsageAvailabilitySchema: z.ZodType<UsageAvailability> = z.union([
  z.looseObject({
    state: z.literal("available").refine((value) => value !== undefined, { message: "Required" }),
    windows: z.lazy(() => WindowsSchema).refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    reason: z.lazy(() => UnavailableReasonSchema).refine((value) => value !== undefined, { message: "Required" }),
    state: z.literal("unavailable").refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    reason: z.lazy(() => UnknownReasonSchema).refine((value) => value !== undefined, { message: "Required" }),
    state: z.literal("unknown").refine((value) => value !== undefined, { message: "Required" }),
  }),
]);

export const UsageIdentitySchema: z.ZodType<UsageIdentity> = z.looseObject({
  auth_mode: z.lazy(() => AuthModeSchema).refine((value) => value !== undefined, { message: "Required" }),
  availability: z.lazy(() => UsageAvailabilitySchema).refine((value) => value !== undefined, { message: "Required" }),
  harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
  plan: z.union([z.string(), z.null()]).optional(),
  selector: z.lazy(() => IdentitySelectorSchema).refine((value) => value !== undefined, { message: "Required" }),
  variant: z.union([z.string(), z.null()]).optional(),
});

export const UsageOptionsSchema: z.ZodType<UsageOptions> = z.strictObject({
  all: z.boolean().optional(),
  bins: z.record(z.string(), z.string()).optional(),
  config: z.string().optional(),
  cwd: z.string().optional(),
  exclude: z.array(z.string()).optional(),
  harnesses: z.array(z.string()).optional(),
  noConfig: z.boolean().optional(),
  timeoutSeconds: z.int().gte(0).optional(),
});

export const UsageReportSchema: z.ZodType<UsageReport> = z.looseObject({
  identities: z
    .array(z.lazy(() => UsageIdentitySchema))
    .refine((value) => value !== undefined, { message: "Required" }),
  observed_at: z.lazy(() => UtcInstantSchema).refine((value) => value !== undefined, { message: "Required" }),
  schema_version: z.literal("0.1").refine((value) => value !== undefined, { message: "Required" }),
});

export const UsageWindowSchema: z.ZodType<UsageWindow> = z.intersection(
  z.looseObject({
    id: z.string().refine((value) => value !== undefined, { message: "Required" }),
    is_binding: z.union([z.boolean(), z.null()]).optional(),
    label: z.union([z.string(), z.null()]).optional(),
    resets_at: z.union([z.lazy(() => UtcInstantSchema), z.null()]).optional(),
    scope: z.union([z.string(), z.null()]).optional(),
    usage: z.lazy(() => WindowUsageSchema).refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.union([
    z.looseObject({
      window_seconds: z
        .int()
        .gte(1)
        .refine((value) => value !== undefined, { message: "Required" }),
      window_seconds_source: z.literal("reported").refine((value) => value !== undefined, { message: "Required" }),
    }),
    z.looseObject({
      window_seconds: z
        .int()
        .gte(1)
        .refine((value) => value !== undefined, { message: "Required" }),
      window_seconds_source: z
        .literal("inferred_from_id")
        .refine((value) => value !== undefined, { message: "Required" }),
    }),
    z.looseObject({
      window_seconds_source: z.literal("unknown").refine((value) => value !== undefined, { message: "Required" }),
    }),
  ]),
);

export const UsedPercentSchema: z.ZodType<UsedPercent> = z.number().gte(0);

export const UtcInstantSchema: z.ZodType<UtcInstant> = z.string();

export const VariantInfoSchema: z.ZodType<VariantInfo> = z.looseObject({
  args: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
  bin: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  env_file: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  env_from: z.record(z.string(), z.string()).refine((value) => value !== undefined, { message: "Required" }),
  env_keys: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
  harness_id: z.string().refine((value) => value !== undefined, { message: "Required" }),
  model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  name: z.string().refine((value) => value !== undefined, { message: "Required" }),
  unset_env: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
});

export const VariantReportSchema: z.ZodType<VariantReport> = z.looseObject({
  allowed_tools: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  args: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  bin: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  denied_tools: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
  env: z
    .record(
      z.string(),
      z.lazy(() => Field3Schema),
    )
    .refine((value) => value !== undefined, { message: "Required" }),
  env_file: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  env_from: z
    .record(
      z.string(),
      z.lazy(() => Field3Schema),
    )
    .refine((value) => value !== undefined, { message: "Required" }),
  hooks: z.looseObject({}).refine((value) => value !== undefined, { message: "Required" }),
  model: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  reasoning: z.lazy(() => Field3Schema).refine((value) => value !== undefined, { message: "Required" }),
  settings: z.looseObject({}).refine((value) => value !== undefined, { message: "Required" }),
  unset_env: z.lazy(() => Field2Schema).refine((value) => value !== undefined, { message: "Required" }),
});

export const WindowUsageSchema: z.ZodType<WindowUsage> = z.union([
  z.looseObject({
    counters: z.union([z.lazy(() => QuotaCountersSchema), z.null()]).optional(),
    kind: z.literal("metered").refine((value) => value !== undefined, { message: "Required" }),
    used_percent: z.lazy(() => UsedPercentSchema).refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.looseObject({
    kind: z.literal("unlimited").refine((value) => value !== undefined, { message: "Required" }),
  }),
]);

export const WindowsSchema: z.ZodType<Windows> = z.array(z.lazy(() => UsageWindowSchema));
