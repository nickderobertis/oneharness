import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { assertPayload, makePayload, assertPackResponse } from "./contract.mjs";
test("release payload v1 golden round-trips through the producer and consumer API", () => {
  const golden = JSON.parse(
    readFileSync(new URL("./payload-v1.json", import.meta.url), "utf8"),
  );
  const { schema_version, ...fields } = golden;
  assert.deepEqual(JSON.parse(JSON.stringify(makePayload(fields))), golden);
  assert.deepEqual(assertPayload(JSON.parse(JSON.stringify(golden))), golden);
  for (const changed of [
    { schema_version: 2 },
    { target: "unknown" },
    { version: [] },
    { tag: "v2.0.0" },
    { binarySha256: "not-a-hash" },
    { wheel: "../outside.whl" },
  ])
    assert.throws(
      () => assertPayload({ ...golden, ...changed }),
      /Invalid release payload/,
    );
  assert.equal(
    assertPackResponse([{ filename: "launcher.tgz" }], "/staging"),
    "launcher.tgz",
  );
  for (const value of [
    null,
    [],
    [{ filename: "../outside.tgz" }],
    [{ filename: "a.tgz" }, { filename: "b.tgz" }],
  ])
    assert.throws(() => assertPackResponse(value, "/staging"), /npm pack/);
});
