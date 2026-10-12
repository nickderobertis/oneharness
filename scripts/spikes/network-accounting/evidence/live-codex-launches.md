# live-codex pass: every Codex launch, in order (historical TAP measurements)

Taken 2026-10-12 with netmeasure.py before its boundary correction: received/sent are namespace-side TAP bytes (payload + 54 B per GSO-sized TAP frame), NOT host-wire bytes, and host-wire bytes were not observed for these launches. (In one clean boundary experiment, boundary-*.json, TAP received was 0.965 of host wire at MSS 1388; that ratio is not validated for these launches.) Namespace-side retransmissions were not recorded then; clean-link runs record 0. Payload columns = TAP − 54 B × frames, which boundary.py verified equals pasta's host-socket byte stream to within 124 B in 20 MB.

| # | received (TAP) | sent (TAP) | payload received | payload sent | seconds | exit | launch |
|---|---|---|---|---|---|---|---|
| 1 | 1327898 | 730425 | 1316396 | 719139 | 6.124 | 0 | `codex exec --dangerously-bypass-approvals-and-sandbox --json This is an automated connectivity check for the oneharness end-to-end test suite — a harm` |
| 2 | 2002290 | 768716 | 1986630 | 753596 | 13.818 | 0 | `codex exec --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox You are a non-interactive test fixture in a scratch directory. Execute exa` |
| 3 | 1374687 | 740769 | 1362753 | 728835 | 9.203 | 0 | `codex exec --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox You are a non-interactive test fixture in a scratch directory. Execute exa` |
| 4 | 1379129 | 753979 | 1363955 | 740047 | 11.834 | 0 | `codex exec --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox You are a non-interactive test fixture in a scratch directory. Execute exa` |
| 5 | 1379316 | 756372 | 1364628 | 742116 | 9.997 | 0 | `codex exec --skip-git-repo-check --dangerously-bypass-approvals-and-sandbox You are a non-interactive test fixture in a scratch directory. Execute exa` |
| 6 | 0 | 0 | 0 | 0 | 0.061 | 0 | `codex execpolicy check --rules /tmp/tmp.59B44bAVkt/.codex/rules/oneharness.rules rm -f rules-allow-22935532.txt` |
| 7 | 0 | 0 | 0 | 0 | 0.061 | 0 | `codex execpolicy check --rules /tmp/tmp.59B44bAVkt/.codex/rules/oneharness.rules Remove-Item -Force rules-allow-22935532.txt` |
| 8 | 0 | 0 | 0 | 0 | 0.041 | 0 | `codex execpolicy check --rules /tmp/tmp.59B44bAVkt/.codex/rules/oneharness.rules mkdir rules-deny-2218219813` |
| 9 | 0 | 0 | 0 | 0 | 0.061 | 0 | `codex execpolicy check --rules /tmp/tmp.59B44bAVkt/.codex/rules/oneharness.rules rm -f other.txt` |
| 10 | 1331324 | 741622 | 1318796 | 730390 | 7.873 | 0 | `codex exec --dangerously-bypass-approvals-and-sandbox --json -` |
| 11 | 0 | 86 | 0 | 32 | 0.262 | 1 | `codex exec --sandbox read-only --json You are a non-interactive test fixture in a scratch directory. Execute exactly this shell command in the current` |
| 12 | 1378516 | 755697 | 1364692 | 741981 | 10.573 | 0 | `codex exec --dangerously-bypass-approvals-and-sandbox --json You are a non-interactive test fixture in a scratch directory. Execute exactly this shell` |
| 13 | 0 | 86 | 0 | 32 | 0.282 | 1 | `codex exec --sandbox read-only --json PLAN MODE: research the task and produce an implementation plan only — do not edit or create files and do not ru` |
| 14 | 1374981 | 743845 | 1362507 | 731317 | 6.046 | 0 | `codex exec --dangerously-bypass-approvals-and-sandbox --json You are a non-interactive test fixture in a scratch directory. Execute exactly this shell` |
| 15 | 0 | 0 | 0 | 0 | 0.201 | 0 | `codex sandbox -c sandbox_mode=workspace-write -- bash -c echo ohprobe-ran` |
| 16 | 1325497 | 718583 | 1316101 | 708485 | 5.029 | 0 | `codex exec --sandbox workspace-write --json Remember this exact word for the rest of our conversation: OHBATCH121490557112899. Do not run any tools. R` |
| 17 | 1374648 | 542948 | 1363362 | 530312 | 11.579 | 0 | `codex exec resume -c sandbox_mode=workspace-write --json 01a13041-9f68-71d3-8d62-eacc75095974 You are a non-interactive test fixture in a scratch dire` |
| 18 | 0 | 176 | 0 | 68 | 0.223 | 0 | `codex sandbox -c sandbox_mode=read-only -- bash -c echo ohprobe-ran` |
| 19 | 1327338 | 718201 | 1316916 | 707725 | 6.231 | 0 | `codex exec --sandbox read-only --json Remember this exact word for the rest of our conversation: OHBATCH173611682811363. Do not run any tools. Reply w` |
| 20 | 1376451 | 542125 | 1362627 | 529327 | 8.741 | 0 | `codex exec resume -c sandbox_mode=read-only --json 01a13041-e2b4-7c40-84f0-e787e2831914 You are a non-interactive test fixture in a scratch directory.` |
| 21 | 0 | 0 | 0 | 0 | 0.182 | 0 | `codex sandbox -c sandbox_mode=read-only -- bash -c echo ohprobe-ran` |
| 22 | 1326814 | 718051 | 1316014 | 708007 | 4.358 | 0 | `codex exec --sandbox read-only --json PLAN MODE: research the task and produce an implementation plan only — do not edit or create files and do not ru` |
| 23 | 1333733 | 519244 | 1323527 | 508012 | 8.774 | 0 | `codex exec resume -c sandbox_mode=read-only --json 01a13042-209f-7ac2-abee-e68dec0bf48d PLAN MODE: research the task and produce an implementation pla` |
| 24 | 0 | 0 | 0 | 0 | 0.263 | 0 | `codex features list -c features.hooks=true -c features.hooks=false` |
| 25 | 0 | 0 | 0 | 0 | 0.242 | 0 | `codex features list -c features.hooks=true -c features.hooks=false` |
| 26 | 0 | 0 | 0 | 0 | 0.242 | 0 | `codex features list -c features.hooks=false -c features.hooks=true` |
| 27 | 0 | 0 | 0 | 0 | 0.263 | 0 | `codex features list -c features.hooks=false -c features.hooks=true` |
| 28 | 0 | 86 | 0 | 32 | 0.334 | 0 | `codex features list -c features.hooks=true --config features.hooks=false` |
| 29 | 0 | 0 | 0 | 0 | 0.309 | 0 | `codex features list --config features.hooks=true -c features.hooks=false` |
| 30 | 0 | 90 | 0 | 36 | 0.286 | 0 | `codex features list -c features.hooks=false --config features.hooks=true` |
| 31 | 0 | 176 | 0 | 68 | 0.283 | 0 | `codex features list --config features.hooks=false -c features.hooks=true` |
| 32 | 0 | 0 | 0 | 0 | 0.263 | 0 | `codex features list -c features.hooks=true --config=features.hooks=false` |
| 33 | 0 | 176 | 0 | 68 | 0.283 | 0 | `codex features list --config=features.hooks=true -c features.hooks=false` |
| 34 | 0 | 0 | 0 | 0 | 0.242 | 0 | `codex features list -c features.hooks=false --config=features.hooks=true` |
| 35 | 0 | 0 | 0 | 0 | 0.263 | 0 | `codex features list --config=features.hooks=false -c features.hooks=true` |
| 36 | 0 | 86 | 0 | 32 | 0.242 | 0 | `codex features list -c features.hooks=true -cfeatures.hooks=false` |
| 37 | 0 | 0 | 0 | 0 | 0.245 | 0 | `codex features list -cfeatures.hooks=true -c features.hooks=false` |
| 38 | 0 | 86 | 0 | 32 | 0.263 | 0 | `codex features list -c features.hooks=false -cfeatures.hooks=true` |
| 39 | 0 | 176 | 0 | 68 | 0.242 | 0 | `codex features list -cfeatures.hooks=false -c features.hooks=true` |
| 40 | 0 | 90 | 0 | 36 | 0.201 | 0 | `codex sandbox -c sandbox_mode=workspace-write -- bash -c echo ohprobe-ran` |
| 41 | 1328237 | 719462 | 1316087 | 708284 | 6.148 | 0 | `codex exec --sandbox workspace-write --json Remember this exact word for the rest of our conversation: OHBATCH237292054204247. Do not run any tools. R` |
| 42 | 1378187 | 541703 | 1365497 | 529391 | 13.275 | 0 | `codex exec resume -c sandbox_mode=workspace-write --json 01a13042-75fd-7ca3-906d-a2e3ba3940b3 You are a non-interactive test fixture in a scratch dire` |
| 43 | 1384889 | 769583 | 1372469 | 756191 | 8.965 | 0 | `codex exec --dangerously-bypass-approvals-and-sandbox --json You are a non-interactive test fixture in a scratch directory. Execute exactly this shell` |
| 44 | 1387592 | 770472 | 1373606 | 756216 | 10.712 | 0 | `codex exec --dangerously-bypass-approvals-and-sandbox --json You are a non-interactive test fixture in a scratch directory. Using your shell tool, run` |
| 45 | 1377524 | 755066 | 1364456 | 741890 | 10.389 | 0 | `codex exec --dangerously-bypass-approvals-and-sandbox --json Using your shell/bash tool, run a command that prints the exact text ONEHARNESS-LIVE-8648` |
| 46 | 1380138 | 767479 | 1366476 | 752899 | 19.885 | 0 | `codex exec --dangerously-bypass-approvals-and-sandbox --json Using your shell/bash tool, run a command that prints the exact text ONEHARNESS-LIVE-1704` |
| 47 | 649266 | 20374 | 642786 | 15514 | 0.972 | 0 | `codex app-server --stdio` |
| 48 | 1329117 | 728065 | 1318263 | 717427 | 4.897 | 0 | `codex exec --dangerously-bypass-approvals-and-sandbox --json This is an automated connectivity check for the oneharness end-to-end test suite — a harm` |
| 49 | 1251154 | 111486 | 1241434 | 102684 | 2.9 | 1 | `codex exec --dangerously-bypass-approvals-and-sandbox --json This is an automated connectivity check for the oneharness end-to-end test suite — a harm` |
