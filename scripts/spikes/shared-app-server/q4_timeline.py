#!/usr/bin/env python3
"""Q4 elapsed times for the `killed-client --no-other --reap` run, from its own
evidence: results/killed-client-reap.json (monotonic offsets and heartbeat line
counts sampled at each event) and results/killed-client-reap.hb.log (one epoch
timestamp per 100 ms heartbeat line, pid 176694).

The client's death is anchored to the newest heartbeat line present when the
first post-kill sample read the log (±0.1 s, the heartbeat period); the
interrupt is that anchor plus the run's own kill->interrupt offset, cross-
checked against the line count read 0.02 s after the interrupt."""
import json
import sys
from pathlib import Path

here = Path(__file__).parent / "results"
run = json.loads((here / "killed-client-reap.json").read_text())["observed"]
stamps = [float(x) for x in (here / "killed-client-reap.hb.log").read_text().split() if x.replace(".", "").isdigit()]
s0, i0 = run["after_kill_samples"][0], run["after_interrupt_samples"][0]
death = stamps[s0["hb_lines"] - 1]
interrupt = death + run["interrupt_sent_s_after_kill"]
last = stamps[-1]
reap_after_last = -run["marker_reap"]["last_line_s_after_reap"]
out = {
    "heartbeat_pid": run["heartbeat_pid"],
    "anchor_precision_s": 0.1,
    "interrupt_crosscheck_s": round(stamps[i0["hb_lines"] - 1] - interrupt, 3),
    "death_to_interrupt_s": run["interrupt_sent_s_after_kill"],
    "death_to_turn_end_s": run["turn_end_s_after_kill"],
    "interrupt_to_turn_end_s": run["turn_end_s_after_interrupt"],
    "turn_completed_status": run["turn_completed_notification_status"],
    "interrupt_to_unsubscribe_and_reap": "harness waits: 30 s observation, 5 s, thread/unsubscribe, 5 s, then marker SIGTERM",
    "death_to_last_heartbeat_s": round(last - death, 2),
    "interrupt_to_last_heartbeat_s": round(last - interrupt, 2),
    "reap_to_last_heartbeat_s": round(-reap_after_last, 3),
    "heartbeat_lines_after_death": len(stamps) - s0["hb_lines"],
    "heartbeat_lines_after_interrupt": len(stamps) - i0["hb_lines"],
    "process_alive_after_interrupt": run["heartbeat_pid_alive_after_interrupt"],
    "process_alive_5s_after_unsubscribe": run["heartbeat_pid_alive_5s_after_unsubscribe"],
    "marker_pids_left_after_reap": run["marker_reap"]["marker_pids_left"],
}
json.dump(out, sys.stdout, indent=2)
print()
