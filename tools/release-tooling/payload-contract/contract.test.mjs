import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { assertPayload, makePayload, assertPackResponse } from "./contract.mjs";
import { assertPackageDirectory } from "./io.mjs";
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
    ...["01.2.3", "1.2.3-..", "1.2.3-01", "1.2.3+.."].map(version => ({ version, tag: `v${version}` })),
    { tag: "v2.0.0" },
    { binarySha256: "not-a-hash" },
    { wheel: "../outside.whl" },
  ])
    assert.throws(
      () => assertPayload({ ...golden, ...changed }),
      /Invalid release payload/,
    );
  for (const version of ["0.0.0", "1.2.3-alpha.1", "1.2.3+build.01", "1.2.3-alpha+build"]) {
    const payload = makePayload({ ...fields, version, tag: `v${version}` });
    assert.deepEqual(assertPayload(JSON.parse(JSON.stringify(payload))), payload);
  }
  assert.equal(
    assertPackResponse([{ filename: "launcher.tgz" }], "/staging"),
    "launcher.tgz",
  );
  for (const value of [
    null,
    [],
    [null],
    [{ filename: "../outside.tgz" }],
    [{ filename: "a.tgz" }, { filename: "b.tgz" }],
  ])
    assert.throws(() => assertPackResponse(value, "/staging"), /npm pack/);
});

test("package-directory boundary refuses non-string builder output before trimming it", () => {
  for (const returned of [undefined, null, 42, Buffer.from("/tmp/stage"), ["/tmp/stage"]]) {
    assert.throws(
      () => assertPackageDirectory(returned, "/tmp/stage", "oneharness-cli", "1.2.3"),
      (error) =>
        !(error instanceof TypeError) &&
        /Package builder output was .*expected a staging directory path\. Next:/.test(error.message),
    );
  }
});
