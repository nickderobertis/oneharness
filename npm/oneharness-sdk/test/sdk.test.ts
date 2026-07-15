import { describe, expect, test } from "bun:test";
import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { OneHarness } from "../src/index.js";

const here = dirname(fileURLToPath(import.meta.url));
const binary = resolve(here, "../../../target/debug/oneharness");
const mock = resolve(here, "../../../target/debug/oneharness-mock-harness");

function sdk(): OneHarness {
	return new OneHarness({
		executable: binary,
		env: { ONEHARNESS_NO_CONFIG: "1" },
	});
}

describe("OneHarness", () => {
	test("crosses the Node to CLI boundary and preserves absent usage", async () => {
		const client = sdk();
		const report = await client.run({
			prompt: "sdk boundary",
			harnesses: ["claude-code"],
			mode: "bypass",
			env: { MOCK_STDOUT: '{"result":"hello from sdk"}' },
			bins: { "claude-code": mock },
		});
		expect(report.results[0]?.text).toBe("hello from sdk");
		expect(report.results[0]?.usage.input_tokens).toBeNull();

		const traced = await client.run({
			prompt: "sdk trace",
			harnesses: ["claude-code"],
			mode: "bypass",
			events: true,
			env: {
				MOCK_STDOUT: [
					'{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"echo hi"}}]}}',
					'{"type":"result","result":"done","usage":{"input_tokens":0,"output_tokens":1,"cache_read_input_tokens":0,"cache_creation_input_tokens":2}}',
				].join("\n"),
			},
			bins: { "claude-code": mock },
		});
		expect(traced.results[0]?.usage.input_tokens).toBe(0);
		expect(traced.results[0]?.usage.cache_read_tokens).toBe(0);
		expect(traced.results[0]?.events?.[0]?.name).toBe("Bash");
		expect(traced.results[0]?.events?.[0]?.input).toEqual({
			command: "echo hi",
		});
		expect(traced.results[0]?.events?.[0]?.kind).toBe("tool_call");
		expect(traced.results[0]?.events?.[0]?.index).toBe(0);
		expect(traced.results[0]?.events?.[0]?.output).toBeNull();
		expect(traced.results[0]?.structured).toBeNull();
	});

	test("lists and detects the open harness registry", async () => {
		const client = sdk();
		const listed = await client.list();
		const claude = listed.find(({ id }) => id === "claude-code");
		expect(claude?.supports_resume).toBe(true);
		expect(claude?.supports_fork).toBe(true);
		expect(claude?.supports_reasoning).toBe(true);
		expect(claude?.modes).toContainEqual({ mode: "bypass", headless: "clean" });
		const detected = await client.detect(["claude-code"]);
		expect(detected).toHaveLength(1);
		expect(detected[0]?.id).toBe("claude-code");
	});

	test("looks up standardized history created across the CLI boundary", async () => {
		const historyDir = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-"));
		const client = sdk();
		await client.run({
			prompt: "history sdk",
			harnesses: ["claude-code"],
			mode: "bypass",
			history: true,
			historyName: "node-session",
			historyDir,
			bins: { "claude-code": mock },
		});
		const records = await client.history({
			session: "node-session",
			historyDir,
		});
		expect(records[0]?.prompt).toBe("history sdk");
		expect(records[0]?.name).toBe("node-session");
		expect(records[0]?.status).toBe("ok");
	});

	test("continues a native session with the new user message", async () => {
		const argvFile = resolve(
			await mkdtemp(resolve(tmpdir(), "oneharness-sdk-resume-")),
			"argv",
		);
		const client = sdk();
		const first = await client.run({
			prompt: "first user message",
			harnesses: ["claude-code"],
			mode: "bypass",
			env: {
				MOCK_STDOUT: '{"result":"first","session_id":"sdk-session-1"}',
			},
			bins: { "claude-code": mock },
		});
		expect(first.results[0]?.session_id).toBe("sdk-session-1");

		const continued = await client.run({
			prompt: "second user message",
			harnesses: ["claude-code"],
			resume: first.results[0]?.session_id ?? "",
			mode: "bypass",
			env: {
				MOCK_ARGV_FILE: argvFile,
				MOCK_STDOUT: '{"result":"continued"}',
			},
			bins: { "claude-code": mock },
		});
		expect(continued.resume).toBe("sdk-session-1");
		expect(continued.prompt).toBe("second user message");
		const argv = (await readFile(argvFile, "utf8")).split("\n");
		expect(argv).toContain("sdk-session-1");
		expect(argv).toContain("second user message");
	});

	test("surfaces missing history and unsupported continuation selections", async () => {
		const historyDir = await mkdtemp(
			resolve(tmpdir(), "oneharness-sdk-missing-"),
		);
		const client = sdk();
		await expect(
			client.history({ session: "does-not-exist", historyDir }),
		).rejects.toThrow("oneharness exited 1");
		await expect(
			client.run({
				prompt: "cannot continue two providers",
				harnesses: ["claude-code", "codex"],
				resume: "sdk-session-1",
			}),
		).rejects.toThrow("--resume needs exactly one harness");
	});

	test("classifies provider failures and tolerates malformed provider output", async () => {
		const client = sdk();
		const failed = await client.run({
			prompt: "provider failure",
			harnesses: ["claude-code"],
			mode: "bypass",
			env: {
				MOCK_EXIT: "1",
				MOCK_STDERR: "rate limit exceeded",
				MOCK_STDOUT: "",
			},
			bins: { "claude-code": mock },
		});
		expect(failed.results[0]?.status).toBe("nonzero");
		expect(failed.results[0]?.failure_kind).toBe("rate_limit");
		expect(failed.results[0]?.failure_kind_source).toBe("stderr");

		const malformed = await client.run({
			prompt: "malformed provider response",
			harnesses: ["claude-code"],
			mode: "bypass",
			env: { MOCK_STDOUT: "{not-json" },
			bins: { "claude-code": mock },
		});
		expect(malformed.results[0]?.status).toBe("ok");
		expect(malformed.results[0]?.text).toBeNull();
		expect(malformed.results[0]?.stdout).toBe("{not-json");
	});

	test("forwards optional reasoning without confusing thinking with actions", async () => {
		const argvFile = resolve(
			await mkdtemp(resolve(tmpdir(), "oneharness-sdk-reasoning-")),
			"argv",
		);
		const report = await sdk().run({
			prompt: "think then act",
			harnesses: ["claude-code"],
			mode: "bypass",
			reasoning: "high",
			events: true,
			env: {
				MOCK_ARGV_FILE: argvFile,
				MOCK_STDOUT: [
					'{"type":"reasoning","text":"private thought"}',
					'{"type":"result","result":"done"}',
				].join("\n"),
			},
			bins: { "claude-code": mock },
		});
		expect(report.results[0]?.events).toBeNull();
		const argv = (await readFile(argvFile, "utf8")).split("\n");
		expect(argv).toContain("--effort");
		expect(argv).toContain("high");
	});

	test("rejects an empty prompt before spawning", async () => {
		await expect(new OneHarness().run({ prompt: "" })).rejects.toThrow(
			"prompt must not be empty",
		);
	});

	test("surfaces an executable spawn failure", async () => {
		const missing = resolve(
			await mkdtemp(resolve(tmpdir(), "oneharness-sdk-no-bin-")),
			"missing-oneharness",
		);
		await expect(
			new OneHarness({ executable: missing }).list(),
		).rejects.toThrow("ENOENT");
	});

	test("requires an explicit history selector", async () => {
		await expect(sdk().history()).rejects.toThrow(
			"history requires session or last",
		);
	});
});
