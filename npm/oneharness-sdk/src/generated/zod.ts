/* Generated from oneharness Rust JSON Schemas. Do not edit. */

import { z } from "zod";
import type {
  ActionEvent,
  BatchReport,
  FailureKind,
  FallThrough,
  FallbackReport,
  OutputFormat,
  RunReport,
  RunResult,
  SessionReport,
  Status,
  Usage,
} from "./contracts.js";
import type { DetectInfo, DetectReport } from "./detection.js";
import type { HistoryRecord } from "./history.js";
import type { HistoryList, HistorySessionSummary } from "./history-list.js";
import type { HistoryListOptions } from "./history-list-options.js";
import type { HistoryRecords } from "./history-records.js";
import type { PermissionMode, RunOptions } from "./options.js";
import type { HarnessInfo, ListReport, ModeInfo } from "./registry.js";

export type BatchStrategy = BatchReport["strategy"];
export type ModeHeadless = ModeInfo["headless"];
export type SessionPhase = SessionReport["phase"];

export const ActionEventSchema: z.ZodType<ActionEvent> = z.looseObject({
  index: z.int().gte(0),
  input: z.unknown(),
  kind: z.string(),
  name: z.union([z.string(), z.null()]),
  output: z.union([z.string(), z.null()]),
});

export const BatchReportSchema: z.ZodType<BatchReport> = z.looseObject({
  forked: z.boolean(),
  prompt_count: z.int().gte(0),
  strategy: z.lazy(() => BatchStrategySchema),
});

export const BatchStrategySchema: z.ZodType<BatchStrategy> = z.union([z.literal("speed"), z.literal("min-tokens")]);

export const DetectInfoSchema: z.ZodType<DetectInfo> = z.looseObject({
  available: z.boolean(),
  bin: z.string(),
  id: z.string(),
  path: z.union([z.string(), z.null()]),
  version: z.union([z.string(), z.null()]),
});

export const DetectReportSchema: z.ZodType<DetectReport> = z.looseObject({
  detected: z.array(z.lazy(() => DetectInfoSchema)),
  schema_version: z.string(),
});

export const FailureKindSchema: z.ZodType<FailureKind> = z.union([
  z.literal("auth"),
  z.literal("rate_limit"),
  z.literal("model_not_found"),
  z.literal("quota"),
  z.literal("tool_deferred"),
]);

export const FallThroughSchema: z.ZodType<FallThrough> = z.looseObject({
  harness: z.string(),
  reason: z.string(),
});

export const FallbackReportSchema: z.ZodType<FallbackReport> = z.looseObject({
  fell_through: z.array(z.lazy(() => FallThroughSchema)),
  ran: z.union([z.string(), z.null()]),
});

export const HarnessInfoSchema: z.ZodType<HarnessInfo> = z.looseObject({
  default_bin: z.string(),
  display: z.string(),
  example_command: z.array(z.string()),
  fork_reuses_cache: z.boolean(),
  id: z.string(),
  install_hint: z.string(),
  mock_rewrite: z.union([z.string(), z.null()]),
  modes: z.array(z.lazy(() => ModeInfoSchema)),
  output_format: z.lazy(() => OutputFormatSchema),
  session_capable: z.boolean(),
  supports_allowed_tools: z.boolean(),
  supports_denied_tools: z.boolean(),
  supports_fork: z.boolean(),
  supports_hooks: z.boolean(),
  supports_mock_deny: z.boolean(),
  supports_native_schema: z.boolean(),
  supports_prompt_stdin: z.boolean(),
  supports_reasoning: z.boolean(),
  supports_resume: z.boolean(),
  supports_system_file: z.boolean(),
  sync_file: z.union([z.string(), z.null()]),
});

export const HistoryListSchema: z.ZodType<HistoryList> = z.array(z.lazy(() => HistorySessionSummarySchema));

export const HistoryListOptionsSchema: z.ZodType<HistoryListOptions> = z.strictObject({
  allProjects: z.boolean().optional(),
  historyDir: z.string().optional(),
  project: z.string().optional(),
});

export const HistoryRecordSchema: z.ZodType<HistoryRecord> = z.looseObject({
  duration_ms: z.union([z.int().gte(0), z.null()]),
  events: z.union([z.array(z.lazy(() => ActionEventSchema)), z.null()]),
  exit_code: z.union([z.int(), z.null()]),
  failure_kind: z.union([z.lazy(() => FailureKindSchema), z.null()]),
  harness: z.string(),
  model: z.union([z.string(), z.null()]),
  name: z.string(),
  permission_mode: z.lazy(() => PermissionModeSchema),
  project: z.string(),
  prompt: z.string(),
  schema_version: z.string(),
  session: z.string(),
  session_id: z.union([z.string(), z.null()]),
  status: z.lazy(() => StatusSchema),
  text: z.union([z.string(), z.null()]),
  text_source: z.union([z.string(), z.null()]),
  timestamp: z.string(),
  usage: z.lazy(() => UsageSchema),
});

export const HistoryRecordsSchema: z.ZodType<HistoryRecords> = z.array(z.lazy(() => HistoryRecordSchema));

export const HistorySessionSummarySchema: z.ZodType<HistorySessionSummary> = z.looseObject({
  harnesses: z.array(z.string()),
  id: z.string(),
  name: z.string(),
  path: z.string(),
  project: z.string(),
  record_count: z.int().gte(0),
  started: z.string(),
});

export const ListReportSchema: z.ZodType<ListReport> = z.looseObject({
  harnesses: z.array(z.lazy(() => HarnessInfoSchema)),
  schema_version: z.string(),
});

export const ModeHeadlessSchema: z.ZodType<ModeHeadless> = z.union([z.literal("clean"), z.literal("hangs")]);

export const ModeInfoSchema: z.ZodType<ModeInfo> = z.looseObject({
  headless: z.lazy(() => ModeHeadlessSchema),
  mode: z.lazy(() => PermissionModeSchema),
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

export const RunOptionsSchema: z.ZodType<RunOptions> = z.strictObject({
  bins: z.record(z.string(), z.string()).optional(),
  cwd: z.string().optional(),
  env: z.record(z.string(), z.string()).optional(),
  events: z.boolean().optional(),
  fork: z.boolean().optional(),
  harnesses: z.array(z.string()).optional(),
  history: z.boolean().optional(),
  historyDir: z.string().optional(),
  historyName: z.string().optional(),
  mode: z.lazy(() => PermissionModeSchema).optional(),
  models: z.array(z.string()).optional(),
  prompt: z.string().min(1),
  reasoning: z.string().optional(),
  resume: z.string().optional(),
  session: z.string().optional(),
  system: z.string().optional(),
  timeoutSeconds: z.int().gte(0).optional(),
});

export const RunReportSchema: z.ZodType<RunReport> = z.looseObject({
  batch: z.union([z.lazy(() => BatchReportSchema), z.null()]),
  bypass_permissions: z.boolean(),
  config_files: z.array(z.string()),
  dry_run: z.boolean(),
  fallback: z.union([z.lazy(() => FallbackReportSchema), z.null()]),
  fork: z.boolean(),
  history_file: z.union([z.string(), z.null()]),
  mock_rules: z.unknown(),
  model: z.union([z.string(), z.null()]),
  models: z.union([z.array(z.string()), z.null()]),
  oneharness_version: z.string(),
  permission_mode: z.lazy(() => PermissionModeSchema),
  prompt: z.string(),
  results: z.array(z.lazy(() => RunResultSchema)),
  resume: z.union([z.string(), z.null()]),
  schema: z.unknown(),
  schema_max_retries: z.union([z.int().gte(0), z.null()]),
  schema_version: z.string(),
  session: z.union([z.lazy(() => SessionReportSchema), z.null()]),
  spy_file: z.union([z.string(), z.null()]),
});

export const RunResultSchema: z.ZodType<RunResult> = z.looseObject({
  available: z.boolean(),
  bin: z.string(),
  command: z.array(z.string()),
  duration_ms: z.union([z.int().gte(0), z.null()]),
  error: z.union([z.string(), z.null()]),
  events: z.union([z.array(z.lazy(() => ActionEventSchema)), z.null()]),
  events_source: z.union([z.string(), z.null()]),
  exit_code: z.union([z.int(), z.null()]),
  failure_kind: z.union([z.lazy(() => FailureKindSchema), z.null()]),
  failure_kind_source: z.union([z.string(), z.null()]),
  harness: z.string(),
  model: z.union([z.string(), z.null()]),
  output_format: z.lazy(() => OutputFormatSchema),
  prompt: z.union([z.string(), z.null()]),
  schema_attempts: z.union([z.int().gte(0), z.null()]),
  schema_error: z.union([z.string(), z.null()]),
  schema_valid: z.union([z.boolean(), z.null()]),
  session_id: z.union([z.string(), z.null()]),
  status: z.lazy(() => StatusSchema),
  stderr: z.string(),
  stdout: z.string(),
  structured: z.unknown(),
  text: z.union([z.string(), z.null()]),
  text_source: z.union([z.string(), z.null()]),
  usage: z.lazy(() => UsageSchema),
  usage_source: z.union([z.string(), z.null()]),
});

export const SessionPhaseSchema: z.ZodType<SessionPhase> = z.union([z.literal("create"), z.literal("continue")]);

export const SessionReportSchema: z.ZodType<SessionReport> = z.looseObject({
  name: z.string(),
  phase: z.lazy(() => SessionPhaseSchema),
  store_file: z.union([z.string(), z.null()]),
  token: z.union([z.string(), z.null()]),
});

export const StatusSchema: z.ZodType<Status> = z.union([
  z.literal("ok"),
  z.literal("nonzero"),
  z.literal("timeout"),
  z.literal("spawn-error"),
  z.literal("skipped"),
  z.literal("planned"),
]);

export const UsageSchema: z.ZodType<Usage> = z.looseObject({
  cache_read_tokens: z.union([z.int().gte(0), z.null()]),
  cache_write_tokens: z.union([z.int().gte(0), z.null()]),
  cost_usd: z.union([z.number(), z.null()]),
  input_tokens: z.union([z.int().gte(0), z.null()]),
  output_tokens: z.union([z.int().gte(0), z.null()]),
});
