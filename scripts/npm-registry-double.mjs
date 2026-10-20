#!/usr/bin/env node
// A local npm registry that serves packed tarballs to the real npm client, and
// can withhold a version from a package's packument for a set number of requests —
// how the public registry behaves after `npm publish` returns, when each
// package's new version becomes visible minutes apart (v0.25.8's
// @oneharness/cli-linux-x64 appeared ~411 s after publish-npm finished).
//
// It answers packuments with the public registry's own `cache-control: public,
// max-age=300` and an ETag, honoring If-None-Match, so npm's local cache
// behaves against it as it does against registry.npmjs.org.
//
// Usage: node scripts/npm-registry-double.mjs <tarball-dir> <port-file> <request-log>
//   <tarball-dir>/*.tgz   every package it serves (`npm pack` output)
//   <tarball-dir>/<name>@<version>.hidden
//                         where <name> has `/` written as `+`: `N [clock]`,
//                         withholding <version> until the registry has answered
//                         more than N packument requests for the package
//                         <clock> (default <name>). One `npm install` asks for
//                         its root package once but may ask for a dependency
//                         more than once, so a dependency clocked against the
//                         root lags by whole installs. Read on every request.
// Writes the port it listens on (127.0.0.1, chosen by the OS) to <port-file>
// once listening, and appends `<method> <name> <served versions>` per request
// to <request-log>.

import { createHash } from "node:crypto";
import { appendFileSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";

// Fatal startup and listener failures name the fixture and what to do about it.
function die(message, next, status = 1) {
  process.stderr.write(`npm-registry-double: ${message}\n  Next: ${next}\n`);
  process.exit(status);
}

const [dir, portFile, requestLog] = process.argv.slice(2);
const usage = "node scripts/npm-registry-double.mjs <tarball-dir> <port-file> <request-log>";
if (!dir) die("no <tarball-dir> given", `pass the directory \`npm pack\` wrote to: ${usage}`, 2);
if (!portFile) die("no <port-file> given", `pass a path to write the listening port to: ${usage}`, 2);
if (!requestLog) die("no <request-log> given", `pass a path to append each request to: ${usage}`, 2);

// Package names as npm accepts them, and versions as the verifier does; both
// become URL and file name parts, so nothing else is served.
const NAME = /^(@[a-z0-9][a-z0-9._-]*\/)?[a-z0-9][a-z0-9._-]*$/;
const VERSION = /^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?$/;

// The one entry a tarball's manifest lives at: `package/package.json` in the
// ustar stream `npm pack` writes (512-byte headers, size in octal at 124).
function manifestOf(tgz) {
  const tar = gunzipSync(tgz);
  for (let offset = 0; offset + 512 <= tar.length; ) {
    const name = tar.subarray(offset, offset + 100).toString("utf8").replace(/\0.*$/s, "");
    if (name === "") break;
    const field = tar.subarray(offset + 124, offset + 136).toString("utf8").replace(/\0.*$/s, "").trim();
    if (!/^[0-7]+$/.test(field)) throw new Error(`entry ${name} has a size field ${JSON.stringify(field)} that is not octal`);
    const size = Number.parseInt(field, 8);
    if (offset + 512 + size > tar.length) throw new Error(`entry ${name} claims ${size} bytes past the end of the archive`);
    if (name === "package/package.json") {
      const manifest = JSON.parse(tar.subarray(offset + 512, offset + 512 + size).toString("utf8"));
      if (typeof manifest?.name !== "string" || !NAME.test(manifest.name)) {
        throw new Error(`its manifest names no valid package: ${JSON.stringify(manifest?.name)}`);
      }
      if (typeof manifest.version !== "string" || !VERSION.test(manifest.version)) {
        throw new Error(`its manifest carries no valid version: ${JSON.stringify(manifest.version)}`);
      }
      return manifest;
    }
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  throw new Error("no package/package.json in the archive");
}

const packages = new Map();
let files;
try {
  files = readdirSync(dir).filter((entry) => entry.endsWith(".tgz"));
} catch (error) {
  die(`cannot read the tarball directory ${dir}: ${error.message}`, "pass a directory `npm pack --pack-destination` wrote to");
}
for (const file of files) {
  try {
    const bytes = readFileSync(join(dir, file));
    const manifest = manifestOf(bytes);
    const versions = packages.get(manifest.name) ?? new Map();
    versions.set(manifest.version, { manifest, bytes, file });
    packages.set(manifest.name, versions);
  } catch (error) {
    die(`cannot serve ${join(dir, file)}: ${error.message}`, "rebuild it with `npm pack`, or remove it from the directory");
  }
}

const requests = new Map();

// Malformed control files are the test's own mistake, so they fail the request
// loudly rather than silently serving the version.
function withheld(name, version) {
  const control = join(dir, `${name.replace("/", "+")}@${version}.hidden`);
  let text;
  try {
    text = readFileSync(control, "utf8");
  } catch (error) {
    if (error.code === "ENOENT") return false;
    throw new Error(`cannot read ${control}: ${error.message}`);
  }
  const spec = text.trim().split(/\s+/);
  if (spec.length > 2 || !/^[0-9]+$/.test(spec[0]) || (spec[1] !== undefined && !NAME.test(spec[1]))) {
    throw new Error(`${control} holds ${JSON.stringify(text)}; write \`<whole number> [package name]\``);
  }
  return (requests.get(spec[1] ?? name) ?? 0) <= Number.parseInt(spec[0], 10);
}

function serve(request, response, origin) {
  let path;
  try {
    path = decodeURIComponent(new URL(request.url, origin).pathname).slice(1);
  } catch {
    appendFileSync(requestLog, `${request.method} ${request.url} 400\n`);
    response.writeHead(400, { "content-type": "application/json" }).end('{"error":"malformed request path"}');
    return;
  }
  const tarball = path.match(/^(.+)\/-\/([^/]+\.tgz)$/);
  if (tarball) {
    const entry = [...(packages.get(tarball[1])?.values() ?? [])].find((item) => item.file === tarball[2]);
    appendFileSync(requestLog, `${request.method} ${tarball[1]} tarball ${tarball[2]}\n`);
    if (!entry) {
      response.writeHead(404, { "content-type": "application/json" }).end('{"error":"Not found"}');
      return;
    }
    response.writeHead(200, { "content-type": "application/octet-stream" }).end(entry.bytes);
    return;
  }
  const versions = packages.get(path);
  if (!versions) {
    appendFileSync(requestLog, `${request.method} ${path} 404\n`);
    response.writeHead(404, { "content-type": "application/json" }).end('{"error":"Not found"}');
    return;
  }
  requests.set(path, (requests.get(path) ?? 0) + 1);
  const served = {};
  for (const [version, { manifest, bytes, file }] of versions) {
    if (withheld(path, version)) continue;
    served[version] = {
      ...manifest,
      _id: `${path}@${version}`,
      dist: {
        tarball: `${origin}/${path}/-/${file}`,
        shasum: createHash("sha1").update(bytes).digest("hex"),
        integrity: `sha512-${createHash("sha512").update(bytes).digest("base64")}`,
      },
    };
  }
  const listed = Object.keys(served);
  appendFileSync(requestLog, `${request.method} ${path} ${listed.join(",") || "(none)"}\n`);
  const body = JSON.stringify({
    name: path,
    "dist-tags": listed.length > 0 ? { latest: listed[listed.length - 1] } : {},
    versions: served,
  });
  const etag = `"${createHash("sha1").update(body).digest("hex")}"`;
  const headers = { "content-type": "application/json", "cache-control": "public, max-age=300", etag };
  if (request.headers["if-none-match"] === etag) {
    response.writeHead(304, headers).end();
    return;
  }
  response.writeHead(200, headers).end(body);
}

const server = createServer((request, response) => {
  try {
    serve(request, response, `http://127.0.0.1:${server.address().port}`);
  } catch (error) {
    // npm sees a server error and fails its install; the cause goes to stderr.
    process.stderr.write(`npm-registry-double: ${request.method} ${request.url} failed: ${error.message}\n`);
    if (!response.headersSent) response.writeHead(500, { "content-type": "application/json" });
    response.end('{"error":"registry double failed; see its stderr"}');
  }
});

server.on("error", (error) => {
  die(`cannot listen on 127.0.0.1: ${error.message}`, "check that loopback networking is available to this process");
});

server.listen(0, "127.0.0.1", () => {
  try {
    writeFileSync(portFile, String(server.address().port));
  } catch (error) {
    die(`cannot write the port to ${portFile}: ${error.message}`, "pass a <port-file> in a writable directory");
  }
});
