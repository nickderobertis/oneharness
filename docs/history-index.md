# The dated history index

This is the one declaration of how a oneharness history store is indexed. The
README's history section and `AGENTS.md` point here rather than restating it,
and the two field tables below are test-pinned to the serialized entry types
(`oneharness_core::domain::history_index::{RunIndexEntry, EventIndexEntry}`), so
a field added, removed or renamed in either place fails the suite until the
other follows.

The goal it serves: recording a run costs the same on day one and on day five
hundred, and history stays complete and searchable. So recording never reads
the index or walks the session tree, and only a verb a person or program names
explicitly ever reads the whole store. **No history is ever deleted by the
index**: nothing renames, rewrites, truncates or deletes a segment.

## Layout

```
<history_dir>/
  <project-slug>/<session>.jsonl      # session files — unchanged, authoritative
  .index.d/
    runs-YYYY-MM-DD.ndjson            # one entry per closing run line
    events-YYYY-MM-DD.ndjson          # one entry per event line
  .index.jsonl                        # legacy: an older core's index
  .event-index.jsonl                  # legacy: an older core's event index
  .index.lock                         # legacy: an older core's lock
```

Dates are UTC. The segment extension is deliberately **not** `.jsonl`: every
released core treats any subdirectory of the store as a project and any
`*.jsonl` in it as a session it reads — and that `history clear --all-projects
--yes` deletes. The legacy `.index.jsonl`, `.event-index.jsonl` and
`.index.lock` stay where they are; this core never writes, renames, truncates
or deletes them.

## Entries

One JSON object per line. Every entry carries `schema_version` (`1.0`) and
`kind`. No field's size depends on the prompt, output or event content an entry
points at — an entry is a pointer, and the session file is the record.

**Run entry.** `kind: "run"`, one per closing run line.

| field | meaning |
| --- | --- |
| `schema_version` | the index entry version, `1.0` |
| `kind` | `run` |
| `history_id` | the run's history id (UUIDv7) |
| `session_path` | the session file, relative to the store: `<project-slug>/<session>.jsonl` |
| `session` | the session id — the session file's stem |
| `name` | the session's name |
| `project_slug` | the project directory's slug — the session file's parent directory |
| `harness_id` | the run's whole harness id (`claude-code`, `claude-code:work`) |
| `labels` | the session's labels; omitted when empty |
| `recorded_at` | RFC 3339 UTC, when the closing line was written (the record's `timestamp`) |
| `offset` / `length` | optional hint: the byte offset and length (newline included) of the line in the session file |

**Event entry.** `kind: "event"`, one per event line.

| field | meaning |
| --- | --- |
| `schema_version` | the index entry version, `1.0` |
| `kind` | `event` |
| `run_id` | the id of the run the event belongs to |
| `event_index` | the event's `index` within its run |
| `session_path` | the session file, relative to the store |
| `project_slug` | the project directory's slug |
| `harness_id` | the run's whole harness id |
| `labels` | the session's labels (`{}` when none) |
| `offset` / `length` | optional hint, as on a run entry |

A reader checks an `offset`/`length` hint against what it reads there (a whole
line that parses as the line the entry names) and, on a mismatch, answers by
reading that one session file instead. A `session_path` that is not exactly one
project directory and one `.jsonl` file under it is refused, never opened.

## Rotation

An entry is appended to the segment named for the UTC date of its id's
timestamp — `history_id` for a run, `run_id` for an event — so a run's events
and its closing line share a date, and a run begun before midnight UTC that
closes after it still closes in its start date's segment. A date's first append
creates its segment. An id carrying no timestamp (a legacy UUIDv5, assigned to a
migrated 0.1 record) is placed by `history reindex` under the date its session
id embeds, else its record's `timestamp`; such an id is reachable by id only
with `--all-time`.

Nothing ever renames, rewrites, truncates or deletes a segment — `history
clear`, `history migrate` and `history reindex` included.

## Writing

Recording a run — `oneharness run --history`, and every in-process library run
through `io::run` — appends each session line to its own session file, then one
entry to its date's segment:

- one append-mode write of one complete line;
- the writer reads at most one trailing byte of that segment: when an
  interrupted writer left the segment without its final newline, the entry goes
  out behind one, so the torn tail is a line of its own a reader skips and never
  swallows the next entry;
- no duplicate check, no `.index.lock`, and no lock of its own at all;
- opening a writer reads no index and walks no directory beyond creating its
  own project directory, and a recording run holds nothing in memory whose size
  depends on the store.

Concurrent writers to one segment are each fully and exactly recorded, because
each entry is one write on a file opened for append.

## Readers

Each reader reads only the segments named here; none falls back to another
segment or to the session tree. A segment that cannot be read fails the read
with an error naming its path; a line that does not parse is skipped; an entry
whose session file is gone is skipped at read (one `stat`).

| reader | reads |
| --- | --- |
| id lookup — `find_record_by_id`, `history show <uuid>` | the one runs segment for the id's date, then that entry's session file. A miss is not-found, naming `history reindex` and `--all-time`; there is no fallback read of the legacy index |
| all-time id lookup — `find_record_by_id_in(…, HistoryWindow::AllTime)`, `history show <uuid> --all-time` | every runs segment, then `.index.jsonl` streamed line by line (opened read-only, memory bounded by one line), stopping at the first entry with the id and opening the session file it names. It writes, renames and rebuilds nothing, and runs only when `--all-time` is named |
| pointer-path read — `read_pointers`, `read_session` / `read_session_display` on a pointer's `history_file`, `history show <session-id> --project <dir>` | no index: the session file is opened by name, so every run whose session file exists stays readable, whatever its age |
| listing — `list_sessions`, `history list`, `history show <name>`, `history show --last` | the segments dated inside a `HistoryWindow`: by default the last 7 UTC days, today included; `--days N` the last N UTC days, today included; `--since YYYY-MM-DD` from that date on; `--all-time` every segment plus the legacy index files — never the session tree. For each session listed it reads the one line of its file that names its project |
| watch — `HistoryWatcher::open`, `history watch` | from its cursor's date on (after the cursor, in that date's segment), or with no `--after` from the beginning of the current UTC day's segment (`--days N` starts at the beginning of the last N UTC days, `--since` / `--all-time` earlier). Each poll lists `.index.d/` once and tails by byte offset every segment dated on or after its start — an earlier-dated segment that still receives a closing run line after a later one exists included — plus the day before its start from that segment's size at open. While the legacy index files exist it tails them from their size at open, never reading their earlier bytes (from the beginning under `--all-time`), so what an older core appends is still followed. Records are de-duplicated by id; its memory grows only with the records it has emitted |
| session by id — `find_session_path` | with a project slug, that one file by name; without one, the segments for the date the session id embeds |

## Verbs that read everything

None of these runs implicitly: no writer, lookup or watch calls any of them.

- **`history reindex`** streams every session file with bounded memory and
  appends one entry for each run or event line its date's segment lacks. It is
  idempotent (a second run adds nothing and leaves every segment byte-identical),
  keeps every existing segment's bytes as that segment's leading bytes, reports
  what it added per segment, and names each file it could not read while
  indexing every readable one. It never writes, renames, truncates or deletes a
  legacy index file or a session file. It is how a store written by an older
  core, or session files copied in from another store, become findable by id and
  by date. Its memory does not grow with the store, nor with how many sessions
  one UTC day holds: candidates are spilled to scratch space per segment, then
  each segment is reconciled against the keys it holds by an external sort,
  whose chunks and merge fan-in are fixed. New entries are appended in key
  order.
- **`--all-time`** on `history list`, `history show` and `history watch` reads
  every segment and the legacy index files. The SDKs take the window as one
  `HistoryWindow` value — `{"recent": {"days": N}}`, `{"since": "YYYY-MM-DD"}`
  or `"allTime"` — rendered as `--days`, `--since` or `--all-time`.
- **`history migrate`** rewrites legacy session files to the 1.0 line format. It
  no longer rebuilds `.index.jsonl` and writes no segment; `history reindex`
  indexes what it rewrote.
- **`history clear`** deletes session files, and only under `--yes` — never a
  segment or a legacy index file.

## An older core sharing the directory

Every released `oneharness-core` up to the one in oneharness 0.19.1 keeps its own
index. Sharing a store with this core, it:

- keeps its own reconcile of `.index.jsonl` on every open, at its old cost;
- never reads, writes or deletes a segment: it sees `.index.d/` as a project
  holding no sessions (no `*.jsonl` is in it), so its `clear` removes nothing
  there and cannot remove the non-empty directory;
- sees runs this core recorded only through its own tree walk, which appends
  them to `.index.jsonl` — so its watcher misses them between reconciles.

This core's own walks (`reindex`, `migrate`, `clear`) skip `.index.d/`, and its
watcher tails `.index.jsonl` from its size at open, de-duplicating what an older
core re-appends there by id.

## Library surface

`oneharness_core::io::history`:

- `HistoryWindow` — `Recent { days }`, `Since(UtcDate)`, `AllTime`; the default
  is `Recent { days: 7 }`.
- `list_sessions(dir, project_slug, window)`.
- `reindex(dir) -> Result<HistoryReindexReport, OneharnessError>`.
- `find_record_by_id_in(dir, id, window)` beside `find_record_by_id(dir, id)`,
  and `HistoryWatcher::open_in(…, window)` beside `open` / `open_session`.
- `find_record_by_id`, `find_session_path`, `HistoryWatcher::open`,
  `HistoryWriter::open`, `read_pointers`, `read_session` and
  `read_session_display` keep their signatures.
