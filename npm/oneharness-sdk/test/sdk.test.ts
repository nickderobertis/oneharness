import { describe, expect, test } from "bun:test";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { OneHarness } from "../src/index.js";

const here = dirname(fileURLToPath(import.meta.url));
const binary = resolve(here, "../../../target/debug/oneharness");
const mock = resolve(here, "../../../target/debug/oneharness-mock-harness");
const fixture = resolve(here, "fixture.mjs");

describe("OneHarness", () => {
	test("crosses the Node to CLI boundary and preserves absent usage", async () => {
		process.env.ONEHARNESS_BIN = binary;
		const sdk = new OneHarness();
		const report = await sdk.run({
			prompt: "sdk boundary",
			harnesses: ["claude-code"],
			mode: "bypass",
			env: { MOCK_STDOUT: '{"result":"hello from sdk"}' },
			bins: { "claude-code": mock },
		});
		expect(report.results[0]?.text).toBe("hello from sdk");
		expect(report.results[0]?.usage.input_tokens).toBeNull();

		const traced = await sdk.run({
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
	});

	test("lists and detects the open harness registry", async () => {
		process.env.ONEHARNESS_BIN = binary;
		const sdk = new OneHarness();
		const listed = await sdk.list();
		expect(listed.some(({ id }) => id === "claude-code")).toBe(true);
		const detected = await sdk.detect(["claude-code"]);
		expect(detected).toHaveLength(1);
		expect(detected[0]?.id).toBe("claude-code");
	});

	test("looks up standardized history created across the CLI boundary", async () => {
		process.env.ONEHARNESS_BIN = binary;
		const historyDir = await mkdtemp(resolve(tmpdir(), "oneharness-sdk-"));
		const sdk = new OneHarness();
		await sdk.run({
			prompt: "history sdk",
			harnesses: ["claude-code"],
			mode: "bypass",
			history: true,
			historyName: "node-session",
			historyDir,
			bins: { "claude-code": mock },
		});
		const records = await sdk.history({ session: "node-session", historyDir });
		expect(records[0]?.prompt).toBe("history sdk");
	});

	test("rejects an empty prompt before spawning", async () => {
		await expect(new OneHarness().run({ prompt: "" })).rejects.toThrow(
			"prompt must not be empty",
		);
	});

	test("rejects malformed subprocess responses and failures", async () => {
		const sdk = new OneHarness({
			executable: process.execPath,
			executableArgs: [fixture],
		});
		process.env.SDK_FIXTURE_MODE = "invalid-json";
		await expect(sdk.list()).rejects.toThrow("invalid JSON");
		process.env.SDK_FIXTURE_MODE = "invalid-contract";
		await expect(sdk.run({ prompt: "x" })).rejects.toThrow(
			"invalid oneharness run contract",
		);
		await expect(sdk.history({ session: "x" })).rejects.toThrow(
			"invalid history contract",
		);
		process.env.SDK_FIXTURE_MODE = "fail";
		await expect(sdk.detect()).rejects.toThrow("oneharness exited 7");
	});
});
