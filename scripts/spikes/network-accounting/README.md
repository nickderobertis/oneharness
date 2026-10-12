# spike-network-accounting harness (throwaway, never landed)

Counts one launch's real network bytes, received and sent, without root, on Linux.

    scripts/spikes/network-accounting/netmeasure.py [--label L] [--env K=V]... [--timeout S] -- ARGV...

Runs ARGV in a fresh unprivileged user+net+mount+PID namespace whose only way
out is a `pasta` tap interface (`fetch-pasta.sh` fetches pasta rootlessly with
`apt-get download` + `dpkg -x` when none is on PATH), and prints one JSON line:
`received_bytes`, `sent_bytes` (the tap interface's `/proc/net/dev` deltas over the
launch's whole process tree, detached children included), `method`
(`netns-pasta-ifcounters`), packet counts and timings. `--report FILE` turns it
into a transparent wrapper (stdin/stdout/exit pass through, report appended to
FILE), which is how `codex-shim.sh` measures every Codex launch of a live suite.

- `selfcheck.py`: the loopback self-check, run through `netmeasure.py`: a launch's
  child downloads and uploads generated payloads of chosen sizes from a host
  loopback server while an unrelated host process transfers a third.
- `codex_rpc.py`: drives `codex app-server` (stdio, or WebSocket over a
  `--listen unix://` socket) for the usage probe and one-turn cases, and `codex
  exec`; reports whether the launch's process logged the catalog fetch.
- `detached.sh`: starts `codex app-server --listen unix://SOCK` detached (`setsid -f`).
- `connect_proxy.py`: the byte-counting CONNECT proxy candidate, kept as evidence;
  `--deny` records destinations without tunnelling, for zero-cost routing checks.
- `codex-shim.sh`: a `codex` PATH shim adding `--disable plugins` under netmeasure.

Findings and figures: the `spike-network-accounting-report` document of
`authoring:codex-network-usage`.
