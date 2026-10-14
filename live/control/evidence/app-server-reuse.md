# Shared Codex app-server memory under reuse — one real run

`app-server-reuse.json` beside this file is the telemetry of one run of
`live/control/app-server-reuse.sh` against real **codex-cli 0.159.2** (the npm
launcher `codex`, whose child is the native server) on Linux, with no model
turn: one pooled server serving 5 sequential leases (an idle lease of the
workload's own holding the server from the first to the last), each
`initialize`, `thread/start`, one `thread/shellCommand` of `true` (no model
call — codex-cli 0.159.2 writes a thread's rollout only after a turn and
refuses to resume a thread without one), one `thread/resume`, a 1 s hold and a
release. Each reading waits until `thread/loaded/list` shows every thread
released so far unloaded (bounded at 90 s; none was retained), since Codex
unloads a released thread only after a 60 s grace (observed: `thread/unsubscribe`
answers `unsubscribed`, then 60 s later the server sends `thread/status/changed`
→ `notLoaded` and `thread/closed`).

| Reading | Whole-tree resident memory | Processes summed |
| --- | --- | --- |
| after the 1st release | 280,821,760 B | 892467 (the `codex` launcher), 892510 (the native server) |
| after the 5th release | 304,070,656 B | 892467, 892510 |

All 5 leases ran on server pid 892467. Ratio: 304,070,656 / 280,821,760 =
**1.083**, which `onebudgetspec check live/control/budgets.yaml` over that file
reports as within the 1.5 threshold of `codex-shared-server-rss-growth`
(headroom 0.417; host load ≈ 35 on 8 CPUs). An earlier run of the workload,
before its idle lease, read 0.940 — the ratio moves run to run by a few
percent of a ~290 MB server either way.

`app-server-sharing.json` is one run of `scripts/measure-app-server-sharing.sh`
against the same codex-cli 0.159.2 on the same host (load ≈ 15 on 8 CPUs): the
server held 347,783,168 B with one thread and 388,784,128 B with a second on a
second connection (+41,000,960 B), the first thread ready 1,391 ms after the
server's launch and the second 824 ms after its own connection opened.
