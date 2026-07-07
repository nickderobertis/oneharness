# oneharness

One CLI to drive many agentic coding harnesses (Claude Code, Codex, OpenCode,
Goose, Qwen Code, Crush, Copilot CLI, Cursor) non-interactively and return one
stable JSON shape.

```console
npm install -g oneharness-cli   # installs the `oneharness` command
oneharness detect --all         # which harnesses are installed?
oneharness run --all --prompt "reply with pong"
```

This package ships the **prebuilt** `oneharness` binary — no Rust toolchain and
no compile step. The platform-specific binary is delivered through a
per-platform optional dependency (`@oneharness/cli-<platform>-<arch>`); npm
installs only the one matching your OS and CPU. Supported platforms: Linux and
macOS (x64, arm64) and Windows (x64).

Other install paths — PyPI (`pip install oneharness-cli`), the install script,
crates.io, and building from source — plus full docs are in the
[project README](https://github.com/nickderobertis/oneharness#readme).

MIT licensed.
