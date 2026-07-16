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
import type { HistoryRecords } from "./history-records.js";
import type { PermissionMode, RunOptions } from "./options.js";
import type { HarnessInfo, ListReport, ModeInfo } from "./registry.js";

export type BatchStrategy = BatchReport["strategy"];
export type ModeHeadless = ModeInfo["headless"];
export type SessionPhase = SessionReport["phase"];

export const ActionEventSchema = z.looseObject({
  index: z.int().gte(0),
  input: z.unknown().optional(),
  kind: z.string(),
  name: z.union([z.string(), z.null()]).optional(),
  output: z.union([z.string(), z.null()]).optional(),
}) as unknown as z.ZodType<ActionEvent>;

export const BatchReportSchema = z.looseObject({
  forked: z.boolean(),
  prompt_count: z.int().gte(0),
  strategy: z.lazy(() => BatchStrategySchema),
}) as unknown as z.ZodType<BatchReport>;

export const BatchStrategySchema = z.union([
  z.literal("speed"),
  z.literal("min-tokens"),
]) as unknown as z.ZodType<BatchStrategy>;

export const DetectInfoSchema = z.looseObject({
  available: z.boolean(),
  bin: z.string(),
  id: z.string(),
  path: z.union([z.string(), z.null()]).optional(),
  version: z.union([z.string(), z.null()]).optional(),
}) as unknown as z.ZodType<DetectInfo>;

export const DetectReportSchema = z.looseObject({
  detected: z.array(z.lazy(() => DetectInfoSchema)),
  schema_version: z.string(),
}) as unknown as z.ZodType<DetectReport>;

export const FailureKindSchema = z.union([
  z.literal("auth"),
  z.literal("rate_limit"),
  z.literal("model_not_found"),
  z.literal("quota"),
  z.literal("tool_deferred"),
]) as unknown as z.ZodType<FailureKind>;

export const FallThroughSchema = z.looseObject({
  harness: z.string(),
  reason: z.string(),
}) as unknown as z.ZodType<FallThrough>;

export const FallbackReportSchema = z.looseObject({
  fell_through: z.array(z.lazy(() => FallThroughSchema)),
  ran: z.union([z.string(), z.null()]).optional(),
}) as unknown as z.ZodType<FallbackReport>;

export const HarnessInfoSchema = z.looseObject({
  default_bin: z.string(),
  display: z.string(),
  example_command: z.array(z.string()),
  fork_reuses_cache: z.boolean(),
  id: z.string(),
  install_hint: z.string(),
  mock_rewrite: z.union([z.string(), z.null()]).optional(),
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
  sync_file: z.union([z.string(), z.null()]).optional(),
}) as unknown as z.ZodType<HarnessInfo>;

export const HistoryListSchema = z.array(
  z.lazy(() => HistorySessionSummarySchema),
) as unknown as z.ZodType<HistoryList>;

export const HistoryRecordSchema = z.looseObject({
  duration_ms: z.union([z.int().gte(0), z.null()]).optional(),
  events: z.union([z.array(z.lazy(() => ActionEventSchema)), z.null()]).optional(),
  exit_code: z.union([z.int(), z.null()]).optional(),
  failure_kind: z.union([z.lazy(() => FailureKindSchema), z.null()]).optional(),
  harness: z.string(),
  model: z.union([z.string(), z.null()]).optional(),
  name: z.string(),
  permission_mode: z.lazy(() => PermissionModeSchema),
  project: z.string(),
  prompt: z.string(),
  schema_version: z.string(),
  session: z.string(),
  session_id: z.union([z.string(), z.null()]).optional(),
  status: z.lazy(() => StatusSchema),
  text: z.union([z.string(), z.null()]).optional(),
  text_source: z.union([z.string(), z.null()]).optional(),
  timestamp: z.string(),
  usage: z.lazy(() => UsageSchema),
}) as unknown as z.ZodType<HistoryRecord>;

export const HistoryRecordsSchema = z.array(z.lazy(() => HistoryRecordSchema)) as unknown as z.ZodType<HistoryRecords>;

export const HistorySessionSummarySchema = z.looseObject({
  harnesses: z.array(z.string()),
  id: z.string(),
  name: z.string(),
  path: z.string(),
  project: z.string(),
  record_count: z.int().gte(0),
  started: z.string(),
}) as unknown as z.ZodType<HistorySessionSummary>;

export const ListReportSchema = z.looseObject({
  harnesses: z.array(z.lazy(() => HarnessInfoSchema)),
  schema_version: z.string(),
}) as unknown as z.ZodType<ListReport>;

export const ModeHeadlessSchema = z.union([
  z.literal("clean"),
  z.literal("hangs"),
]) as unknown as z.ZodType<ModeHeadless>;

export const ModeInfoSchema = z.looseObject({
  headless: z.lazy(() => ModeHeadlessSchema),
  mode: z.lazy(() => PermissionModeSchema),
}) as unknown as z.ZodType<ModeInfo>;

export const OutputFormatSchema = z.union([
  z.literal("text"),
  z.literal("json"),
  z.literal("stream-json"),
]) as unknown as z.ZodType<OutputFormat>;

export const PermissionModeSchema = z.union([
  z.literal("read-only"),
  z.literal("plan"),
  z.literal("default"),
  z.literal("edit"),
  z.literal("auto"),
  z.literal("bypass"),
]) as unknown as z.ZodType<PermissionMode>;

export const RunOptionsSchema = z.strictObject({
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
}) as unknown as z.ZodType<RunOptions>;

export const RunReportSchema = z.looseObject({
  batch: z.union([z.lazy(() => BatchReportSchema), z.null()]).optional(),
  bypass_permissions: z.boolean(),
  config_files: z.array(z.string()),
  dry_run: z.boolean(),
  fallback: z.union([z.lazy(() => FallbackReportSchema), z.null()]).optional(),
  fork: z.boolean(),
  history_file: z.union([z.string(), z.null()]).optional(),
  mock_rules: z.unknown().optional(),
  model: z.union([z.string(), z.null()]).optional(),
  models: z.union([z.array(z.string()), z.null()]).optional(),
  oneharness_version: z.string(),
  permission_mode: z.lazy(() => PermissionModeSchema),
  prompt: z.string(),
  results: z.array(z.lazy(() => RunResultSchema)),
  resume: z.union([z.string(), z.null()]).optional(),
  schema: z.unknown().optional(),
  schema_max_retries: z.union([z.int().gte(0), z.null()]).optional(),
  schema_version: z.string(),
  session: z.union([z.lazy(() => SessionReportSchema), z.null()]).optional(),
  spy_file: z.union([z.string(), z.null()]).optional(),
}) as unknown as z.ZodType<RunReport>;

export const RunResultSchema = z.looseObject({
  available: z.boolean(),
  bin: z.string(),
  command: z.array(z.string()),
  duration_ms: z.union([z.int().gte(0), z.null()]).optional(),
  error: z.union([z.string(), z.null()]).optional(),
  events: z.union([z.array(z.lazy(() => ActionEventSchema)), z.null()]).optional(),
  events_source: z.union([z.string(), z.null()]).optional(),
  exit_code: z.union([z.int(), z.null()]).optional(),
  failure_kind: z.union([z.lazy(() => FailureKindSchema), z.null()]).optional(),
  failure_kind_source: z.union([z.string(), z.null()]).optional(),
  harness: z.string(),
  model: z.union([z.string(), z.null()]).optional(),
  output_format: z.lazy(() => OutputFormatSchema),
  prompt: z.union([z.string(), z.null()]).optional(),
  schema_attempts: z.union([z.int().gte(0), z.null()]).optional(),
  schema_error: z.union([z.string(), z.null()]).optional(),
  schema_valid: z.union([z.boolean(), z.null()]).optional(),
  session_id: z.union([z.string(), z.null()]).optional(),
  status: z.lazy(() => StatusSchema),
  stderr: z.string(),
  stdout: z.string(),
  structured: z.unknown().optional(),
  text: z.union([z.string(), z.null()]).optional(),
  text_source: z.union([z.string(), z.null()]).optional(),
  usage: z.lazy(() => UsageSchema),
  usage_source: z.union([z.string(), z.null()]).optional(),
}) as unknown as z.ZodType<RunResult>;

export const SessionPhaseSchema = z.union([
  z.literal("create"),
  z.literal("continue"),
]) as unknown as z.ZodType<SessionPhase>;

export const SessionReportSchema = z.looseObject({
  name: z.string(),
  phase: z.lazy(() => SessionPhaseSchema),
  store_file: z.union([z.string(), z.null()]).optional(),
  token: z.union([z.string(), z.null()]).optional(),
}) as unknown as z.ZodType<SessionReport>;

export const StatusSchema = z.union([
  z.literal("ok"),
  z.literal("nonzero"),
  z.literal("timeout"),
  z.literal("spawn-error"),
  z.literal("skipped"),
  z.literal("planned"),
]) as unknown as z.ZodType<Status>;

export const UsageSchema = z.looseObject({
  cache_read_tokens: z.union([z.int().gte(0), z.null()]).optional(),
  cache_write_tokens: z.union([z.int().gte(0), z.null()]).optional(),
  cost_usd: z.union([z.number(), z.null()]).optional(),
  input_tokens: z.union([z.int().gte(0), z.null()]).optional(),
  output_tokens: z.union([z.int().gte(0), z.null()]).optional(),
}) as unknown as z.ZodType<Usage>;
