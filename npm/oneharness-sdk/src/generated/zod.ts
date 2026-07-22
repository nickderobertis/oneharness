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
  ToolCallStatus,
  Usage,
} from "./contracts.js";
import type { DetectInfo, DetectReport } from "./detection.js";
import type { HistoryRecord } from "./history.js";
import type { HistoryLine } from "./history-line.js";
import type { HistoryList, HistorySessionSummary } from "./history-list.js";
import type { HistoryListOptions } from "./history-list-options.js";
import type { HistoryLookup, HistoryLookupByLast, HistoryLookupBySession } from "./history-lookup.js";
import type { HistoryRecords } from "./history-records.js";
import type { HistoryEventLine, HistoryStreamEnvelope } from "./history-stream-envelope.js";
import type { HistoryWatchOptions } from "./history-watch-options.js";
import type { HistoryLabels, PermissionMode, RunOptions } from "./options.js";
import type { HarnessInfo, ListReport, ModeInfo } from "./registry.js";
import type { RunStreamEnvelope } from "./run-stream-envelope.js";

export type BatchStrategy = BatchReport["strategy"];
export type ModeHeadless = ModeInfo["headless"];
export type SessionPhase = SessionReport["phase"];

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
  tool_call_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const BatchReportSchema: z.ZodType<BatchReport> = z.looseObject({
  forked: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  prompt_count: z
    .int()
    .gte(0)
    .refine((value) => value !== undefined, { message: "Required" }),
  strategy: z.lazy(() => BatchStrategySchema).refine((value) => value !== undefined, { message: "Required" }),
});

export const BatchStrategySchema: z.ZodType<BatchStrategy> = z.union([z.literal("speed"), z.literal("min-tokens")]);

export const DetectInfoSchema: z.ZodType<DetectInfo> = z.looseObject({
  available: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  bin: z.string().refine((value) => value !== undefined, { message: "Required" }),
  id: z.string().refine((value) => value !== undefined, { message: "Required" }),
  path: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  version: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const DetectReportSchema: z.ZodType<DetectReport> = z.looseObject({
  detected: z.array(z.lazy(() => DetectInfoSchema)).refine((value) => value !== undefined, { message: "Required" }),
  schema_version: z.string().refine((value) => value !== undefined, { message: "Required" }),
});

export const FailureKindSchema: z.ZodType<FailureKind> = z.union([
  z.literal("auth"),
  z.literal("rate_limit"),
  z.literal("model_not_found"),
  z.literal("quota"),
  z.literal("tool_deferred"),
]);

export const FallThroughSchema: z.ZodType<FallThrough> = z.looseObject({
  harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
  reason: z.string().refine((value) => value !== undefined, { message: "Required" }),
});

export const FallbackReportSchema: z.ZodType<FallbackReport> = z.looseObject({
  fell_through: z
    .array(z.lazy(() => FallThroughSchema))
    .refine((value) => value !== undefined, { message: "Required" }),
  ran: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
});

export const HarnessInfoSchema: z.ZodType<HarnessInfo> = z.looseObject({
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
});

export const HistoryEventLineSchema: z.ZodType<HistoryEventLine> = z.looseObject({
  event: z.lazy(() => ActionEventSchema).refine((value) => value !== undefined, { message: "Required" }),
  harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
  run_id: z
    .string()
    .min(36)
    .regex(
      new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
    )
    .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
    .refine((value) => value !== undefined, { message: "Required" }),
  schema_version: z.string().refine((value) => value !== undefined, { message: "Required" }),
});

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
  z.looseObject({
    event: z.lazy(() => ActionEventSchema).refine((value) => value !== undefined, { message: "Required" }),
    harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
    run_id: z
      .string()
      .min(36)
      .regex(
        new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
      )
      .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
      .refine((value) => value !== undefined, { message: "Required" }),
    schema_version: z.literal("1.0").refine((value) => value !== undefined, { message: "Required" }),
    type: z.literal("event").refine((value) => value !== undefined, { message: "Required" }),
  }),
  z.union([
    z.looseObject({
      duration_ms: z
        .int()
        .gte(0)
        .refine((value) => value !== undefined, { message: "Required" }),
      exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      failure_kind: z
        .union([z.lazy(() => FailureKindSchema), z.null()])
        .refine((value) => value !== undefined, { message: "Required" }),
      finished_at: z.string().refine((value) => value !== undefined, { message: "Required" }),
      harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
      history_id: z
        .string()
        .min(36)
        .regex(
          new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
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
      permission_mode: z
        .lazy(() => PermissionModeSchema)
        .refine((value) => value !== undefined, { message: "Required" }),
      project: z.string().refine((value) => value !== undefined, { message: "Required" }),
      prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
      schema_version: z.literal("1.0").refine((value) => value !== undefined, { message: "Required" }),
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
      text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      time_to_first_token_ms: z.union([z.int().gte(0), z.null()]).optional(),
      timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
      tool_ms: z
        .int()
        .gte(0)
        .refine((value) => value !== undefined, { message: "Required" }),
      type: z.literal("run").refine((value) => value !== undefined, { message: "Required" }),
      usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
    }),
    z.looseObject({
      duration_ms: z
        .int()
        .gte(0)
        .refine((value) => value !== undefined, { message: "Required" }),
      exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      failure_kind: z
        .union([z.lazy(() => FailureKindSchema), z.null()])
        .refine((value) => value !== undefined, { message: "Required" }),
      finished_at: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
      history_id: z
        .string()
        .min(36)
        .regex(
          new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
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
      permission_mode: z
        .lazy(() => PermissionModeSchema)
        .refine((value) => value !== undefined, { message: "Required" }),
      project: z.string().refine((value) => value !== undefined, { message: "Required" }),
      prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
      schema_version: z.literal("1.0").refine((value) => value !== undefined, { message: "Required" }),
      session: z.string().refine((value) => value !== undefined, { message: "Required" }),
      session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      started_at: z
        .string()
        .min(1)
        .refine((value) => value !== undefined, { message: "Required" }),
      status: z
        .union([z.literal("timeout"), z.literal("spawn-error"), z.literal("skipped"), z.literal("planned")])
        .refine((value) => value !== undefined, { message: "Required" }),
      text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      time_to_first_token_ms: z.union([z.int().gte(0), z.null()]).optional(),
      timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
      tool_ms: z
        .int()
        .gte(0)
        .refine((value) => value !== undefined, { message: "Required" }),
      type: z.literal("run").refine((value) => value !== undefined, { message: "Required" }),
      usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
    }),
    z.looseObject({
      duration_ms: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      exit_code: z.union([z.int(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      failure_kind: z
        .union([z.lazy(() => FailureKindSchema), z.null()])
        .refine((value) => value !== undefined, { message: "Required" }),
      finished_at: z.null().refine((value) => value !== undefined, { message: "Required" }),
      harness: z.string().refine((value) => value !== undefined, { message: "Required" }),
      history_id: z
        .string()
        .min(36)
        .regex(
          new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
        )
        .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
        .refine((value) => value !== undefined, { message: "Required" }),
      labels: z.lazy(() => HistoryLabelsSchema).optional(),
      model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      model_ms: z.never().optional(),
      name: z.string().refine((value) => value !== undefined, { message: "Required" }),
      permission_mode: z
        .lazy(() => PermissionModeSchema)
        .refine((value) => value !== undefined, { message: "Required" }),
      project: z.string().refine((value) => value !== undefined, { message: "Required" }),
      prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
      schema_version: z.literal("1.0").refine((value) => value !== undefined, { message: "Required" }),
      session: z.string().refine((value) => value !== undefined, { message: "Required" }),
      session_id: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      started_at: z.never().optional(),
      status: z.lazy(() => StatusSchema).refine((value) => value !== undefined, { message: "Required" }),
      text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
      time_to_first_token_ms: z.never().optional(),
      timestamp: z.string().refine((value) => value !== undefined, { message: "Required" }),
      tool_ms: z.never().optional(),
      type: z.literal("run").refine((value) => value !== undefined, { message: "Required" }),
      usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
    }),
  ]),
]);

export const HistoryListSchema: z.ZodType<HistoryList> = z.array(z.lazy(() => HistorySessionSummarySchema));

export const HistoryListOptionsSchema: z.ZodType<HistoryListOptions> = z.strictObject({
  allProjects: z.boolean().optional(),
  historyDir: z.string().optional(),
  project: z.string().optional(),
});

export const HistoryLookupSchema: z.ZodType<HistoryLookup> = z.union([
  z.lazy(() => HistoryLookupByLastSchema),
  z.lazy(() => HistoryLookupBySessionSchema),
]);

export const HistoryLookupByLastSchema: z.ZodType<HistoryLookupByLast> = z.strictObject({
  allProjects: z.boolean().optional(),
  historyDir: z.string().optional(),
  last: z.literal(true).refine((value) => value !== undefined, { message: "Required" }),
  project: z.string().optional(),
  session: z.string().optional(),
});

export const HistoryLookupBySessionSchema: z.ZodType<HistoryLookupBySession> = z.strictObject({
  allProjects: z.boolean().optional(),
  historyDir: z.string().optional(),
  last: z.boolean().optional(),
  project: z.string().optional(),
  session: z
    .string()
    .min(1)
    .refine((value) => value !== undefined, { message: "Required" }),
});

export const HistoryRecordSchema: z.ZodType<HistoryRecord> = z.union([
  z.looseObject({
    duration_ms: z
      .int()
      .gte(0)
      .refine((value) => value !== undefined, { message: "Required" }),
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
    history_id: z
      .string()
      .min(36)
      .regex(
        new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
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
    permission_mode: z.lazy(() => PermissionModeSchema).refine((value) => value !== undefined, { message: "Required" }),
    project: z.string().refine((value) => value !== undefined, { message: "Required" }),
    prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
    schema_version: z.literal("1.0").refine((value) => value !== undefined, { message: "Required" }),
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
  }),
  z.looseObject({
    duration_ms: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
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
            output: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
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
    history_id: z
      .string()
      .min(36)
      .regex(
        new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
      )
      .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
      .refine((value) => value !== undefined, { message: "Required" }),
    labels: z.lazy(() => HistoryLabelsSchema).optional(),
    model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
    model_ms: z.never().optional(),
    name: z.string().refine((value) => value !== undefined, { message: "Required" }),
    permission_mode: z.lazy(() => PermissionModeSchema).refine((value) => value !== undefined, { message: "Required" }),
    project: z.string().refine((value) => value !== undefined, { message: "Required" }),
    prompt: z.string().refine((value) => value !== undefined, { message: "Required" }),
    schema_version: z.literal("1.0").refine((value) => value !== undefined, { message: "Required" }),
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
  }),
  z.looseObject({
    duration_ms: z.union([z.int().gte(0), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
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
            output: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
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
    history_id: z
      .string()
      .min(36)
      .regex(
        new RegExp("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[1-8][0-9a-fA-F]{3}-[89abAB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}$", "u"),
      )
      .refine((value) => [...value].length <= 36, { message: "Too long: expected at most 36 characters" })
      .refine((value) => value !== undefined, { message: "Required" }),
    labels: z.lazy(() => HistoryLabelsSchema).optional(),
    model: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
    model_ms: z.union([z.int().gte(0), z.null()]).optional(),
    name: z.string().refine((value) => value !== undefined, { message: "Required" }),
    permission_mode: z.lazy(() => PermissionModeSchema).refine((value) => value !== undefined, { message: "Required" }),
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
  }),
]);

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
  events: z.boolean().optional(),
  historyDir: z.string().optional(),
  labels: z.lazy(() => HistoryLabelsSchema).optional(),
  project: z.string().optional(),
});

export const ListReportSchema: z.ZodType<ListReport> = z.looseObject({
  harnesses: z.array(z.lazy(() => HarnessInfoSchema)).refine((value) => value !== undefined, { message: "Required" }),
  schema_version: z.string().refine((value) => value !== undefined, { message: "Required" }),
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

export const RunOptionsSchema: z.ZodType<RunOptions> = z.strictObject({
  bins: z.record(z.string(), z.string()).optional(),
  cwd: z.string().optional(),
  env: z.record(z.string(), z.string()).optional(),
  events: z.boolean().optional(),
  fork: z.boolean().optional(),
  harnesses: z.array(z.string()).optional(),
  history: z.boolean().optional(),
  historyDir: z.string().optional(),
  historyLabels: z.lazy(() => HistoryLabelsSchema).optional(),
  historyName: z.string().optional(),
  mode: z.lazy(() => PermissionModeSchema).optional(),
  models: z.array(z.string()).optional(),
  prompt: z
    .string()
    .min(1)
    .refine((value) => value !== undefined, { message: "Required" }),
  reasoning: z.string().optional(),
  resume: z.string().optional(),
  session: z.string().optional(),
  system: z.string().optional(),
  timeoutSeconds: z.int().gte(0).optional(),
});

export const RunReportSchema: z.ZodType<RunReport> = z.looseObject({
  batch: z
    .union([z.lazy(() => BatchReportSchema), z.null()])
    .refine((value) => value !== undefined, { message: "Required" }),
  bypass_permissions: z.boolean().refine((value) => value !== undefined, { message: "Required" }),
  config_files: z.array(z.string()).refine((value) => value !== undefined, { message: "Required" }),
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
  text: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  text_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
  usage: z.lazy(() => UsageSchema).refine((value) => value !== undefined, { message: "Required" }),
  usage_source: z.union([z.string(), z.null()]).refine((value) => value !== undefined, { message: "Required" }),
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
  z.literal("spawn-error"),
  z.literal("skipped"),
  z.literal("planned"),
]);

export const ToolCallStatusSchema: z.ZodType<ToolCallStatus> = z.union([
  z.literal("completed"),
  z.literal("failed"),
  z.literal("timeout"),
  z.literal("interrupted"),
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
