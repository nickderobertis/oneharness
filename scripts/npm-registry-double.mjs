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

const [dir, portFile, requestLog] = process.argv.slice(2);
if (!dir || !portFile || !requestLog) {
  process.stderr.write("npm-registry-double: usage: npm-registry-double.mjs <tarball-dir> <port-file> <request-log>\n");
  process.exit(2);
}

// The one entry a tarball's manifest lives at: `package/package.json` in the
// ustar stream `npm pack` writes (512-byte headers, size in octal at 124).
function manifestOf(tgz) {
  const tar = gunzipSync(tgz);
  for (let offset = 0; offset + 512 <= tar.length; ) {
    const name = tar.subarray(offset, offset + 100).toString("utf8").replace(/\0.*$/s, "");
    if (name === "") break;
    const size = Number.parseInt(tar.subarray(offset + 124, offset + 136).toString("utf8").replace(/\0.*$/s, "").trim(), 8);
    if (name === "package/package.json") {
      return JSON.parse(tar.subarray(offset + 512, offset + 512 + size).toString("utf8"));
    }
    offset += 512 + Math.ceil(size / 512) * 512;
  }
  throw new Error("no package/package.json in the tarball");
}

const packages = new Map();
for (const file of readdirSync(dir).filter((entry) => entry.endsWith(".tgz"))) {
  const bytes = readFileSync(join(dir, file));
  const manifest = manifestOf(bytes);
  const versions = packages.get(manifest.name) ?? new Map();
  versions.set(manifest.version, { manifest, bytes, file });
  packages.set(manifest.name, versions);
}

const requests = new Map();

function withheld(name, version) {
  let spec;
  try {
    spec = readFileSync(join(dir, `${name.replace("/", "+")}@${version}.hidden`), "utf8").trim().split(/\s+/);
  } catch {
    return false;
  }
  return (requests.get(spec[1] ?? name) ?? 0) <= Number.parseInt(spec[0], 10);
}

const server = createServer((request, response) => {
  const origin = `http://127.0.0.1:${server.address().port}`;
  const path = decodeURIComponent(new URL(request.url, origin).pathname).slice(1);
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
});

server.listen(0, "127.0.0.1", () => {
  writeFileSync(portFile, String(server.address().port));
});
