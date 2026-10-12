# Evidence (spike-network-accounting, 2026-10-12, codex-cli 0.159.2)

- `live-codex-launches.jsonl` / `.md`: every Codex launch of one `just live-codex`
  pass under `codex-shim.sh` (`--disable plugins`), 49 records, in launch order.
- `spike-launches.jsonl`: every measurement 1–6 launch (usage probe, exec,
  app-server turn, apps off, Claude Code baseline, detached server).
- `selfcheck.json`: the loopback self-check on the final tree.
- `boundary-*.json`: `boundary.py`, the zero-paid-turn comparison of TAP
  counters against pasta's host-side socket (tcp_info) and the host NIC.

All `received_bytes`/`sent_bytes` in the two launch files are HISTORICAL TAP
measurements: namespace-side bytes between the launch and pasta (payload + 54 B
per GSO-sized TAP frame), not host-wire bytes. What boundary.py verified:
- payload (TAP − 54 B × frames) equals pasta's host-socket byte stream to within
  124 B received / 168 B sent in a 20 MB / 5 MB transfer;
- host wire ≈ payload + 54 B per real segment (MSS 1388, no TCP timestamps):
  20,804,034 B estimated vs 20,806,466 B on the host NIC (whole-host counter, quiet
  window), so TAP received = 0.965 of host wire;
- namespace-side loss inflates TAP sent (5 % netem: 11.13–11.50 MB for a 10 MB
  body) without touching the host wire; newer reports carry
  `namespace_tcp_retrans_segs` so that is visible. Host-side retransmissions
  cannot be observed per launch without root.
