# Shared Codex app-server memory under reuse — one real run

`app-server-reuse.json` beside this file is the telemetry of one run of
`live/control/app-server-reuse.sh` against real **codex-cli 0.163.0** (the npm
launcher `codex`, whose child is the native server; the version the live
turn-control CI job installs) on Linux, 2026-10-18, with no model turn: one
pooled server, launched with `-c thread_unload_delay_secs=1`, serving 5
sequential leases (an idle lease of the workload's own holding the server from
the first to the last), each `initialize`, `thread/start`, one
`thread/shellCommand` of `true` (no model call — codex-cli writes a thread's
rollout only after a turn and refuses to resume a thread without one), one
`thread/resume`, a 1 s hold and a release. Each reading waits until
`thread/loaded/list` shows every thread released so far unloaded (bounded at
90 s; none was retained).

| Reading | Whole-tree resident memory | Processes summed |
| --- | --- | --- |
| after the 1st release | 360,267,776 B | 2253902 (the `codex` launcher), 2254015 (the native server) |
| after the 5th release | 370,155,520 B | 2253902, 2254015 |

All 5 leases ran on server pid 2253902. Ratio: 370,155,520 / 360,267,776 =
**1.027**, which `onebudgetspec check live/control/budgets.yaml` over that file
reports as within the 1.5 threshold of `codex-shared-server-rss-growth`
(headroom 0.473; host load ≈ 21 on 8 CPUs). The earlier record, on codex-cli
0.159.2, read 1.083.

Why the unload delay: Codex unloads a thread only once it has had no
subscriber and no activity for `thread_unload_delay_secs`. Observed on this
host with a stdio client that started a thread, ran the shell turn, resumed it
and sent `thread/unsubscribe` (answered `unsubscribed`; a second answered
`notSubscribed`): codex-cli 0.159.2 sent `thread/status/changed` → `notLoaded`
and `thread/closed` 60 s later, its default; codex-cli 0.163.0 at its default
still listed the thread in `thread/loaded/list` 690 s later (its default is
1800 s, `thread_unload_delay_secs` in codex-rs's `config_toml.rs`), and with
`-c thread_unload_delay_secs=5` unloaded it 5 s after the release. The same
workload on 0.163.0 without the override failed with all 5 released threads
still loaded at the 90 s bound, the server growing from 284,598,272 B after the
first release to 366,407,680 B after the fifth.

`app-server-sharing.json` is one run of `scripts/measure-app-server-sharing.sh`
against the same codex-cli 0.159.2 on the same host (load ≈ 15 on 8 CPUs): the
server held 347,783,168 B with one thread and 388,784,128 B with a second on a
second connection (+41,000,960 B), the first thread ready 1,391 ms after the
server's launch and the second 824 ms after its own connection opened.
