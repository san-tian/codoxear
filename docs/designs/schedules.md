# Schedules

## Purpose

Schedules let Codoxear trigger future or recurring agent work from the daemon. They are persisted service state, not browser timers or skill-local cron jobs. The first implementation uses JSON files under the Codoxear app dir and a Rust daemon worker.

## Goals

- Owner Nova can create and manage schedules for existing sessions and for new-session templates.
- Shared-session pages can create and manage schedules only for sessions inside the current share set.
- Scheduled runs wait until the target agent turn finishes before they are considered complete.
- Schedules survive daemon restarts, but missed executions while the daemon is stopped are not replayed.
- The bundled `codoxear-session` skill can list, create, run, disable, delete, and mark schedules done through the same API.

## Target Modes

- `existing_session`: bind to a concrete `session_id` and send the rendered message to that session every run. Busy sessions use the schedule busy policy, defaulting to `enqueue`.
- `new_session_each_run`: create a fresh session from a saved session template for every run, then send the rendered initial message. Alias templates usually include `{datetime}` or `{run_number}`.
- `create_once_reuse`: create a session from the saved template the first time the schedule runs, persist `created_session_id`, and send future runs to that same session. If the reusable session is deleted, the schedule becomes `target_missing` and pauses until the user chooses a replacement or creates one.

Share pages expose only `existing_session` schedules whose target session is in that share set. Share pages do not expose new-session schedule modes.

## Time Rules

Supported rule kinds:

- `once`: one wall-clock date/time in a timezone.
- `daily`: one wall-clock time per day in a timezone.
- `weekly`: selected weekdays at one wall-clock time in a timezone.
- `monthly`: selected day-of-month at one wall-clock time in a timezone; months without that day are skipped.
- `interval`: every N minutes/hours/days, calculated from the previous completion time or the save time.

The first UI should show minute-level precision. The daemon worker ticks about every 15 seconds. If Codoxear is stopped through a due time, that occurrence is missed and not replayed after restart.

## Run Lifecycle

Run statuses:

- `queued`: an existing-session run entered the Codoxear queue and is waiting to be sent.
- `running`: the run has sent its message and is waiting for the agent reply.
- `completed`: Codoxear observed the first assistant final response or turn boundary after the recorded start cursor.
- `timeout`: the run exceeded its timeout.
- `marked_done`: a user manually marked the run done; the agent may still be running.
- `skipped_overlap`: a due time was skipped because the schedule already had an active `queued` or `running` run.
- `target_missing`: the bound session or reusable session no longer exists.

Existing-session runs record the live cursor before enqueue/send. If the session is busy and the message is enqueued, the run stays `queued`; once the queue item is gone, the worker starts waiting from that cursor for the next assistant final/turn-end. Timeout starts at the scheduled trigger time and includes queued time. If a queued run times out, Codoxear records the timeout but does not delete the queue item.

Only one active run per schedule is allowed. `Run now` follows the same non-reentrant rule and does not change the next scheduled run time.

## Timeout

Schedules default to a 240-minute run timeout. Users can set `timeout_minutes` to `null` for no timeout. A stuck no-timeout run can be marked done from the UI; this stops scheduler tracking but does not interrupt the agent session.

## Templates

Messages and aliases support simple variables:

- `{date}`
- `{time}`
- `{datetime}`
- `{run_number}`

Run history stores the rendered message, rendered alias, target mode, session template snapshot when relevant, schedule rule snapshot, session ids, status, timestamps, error text, and assistant preview. It does not store full transcripts.

## Storage

The first implementation stores:

- `schedules.json`: schedule definitions.
- `schedule_runs.json`: recent run history and active run records.

Each schedule keeps the recent 50 runs, while preserving at least the recent 10 failures.

## API Sketch

Owner routes:

- `GET /api/v1/schedules`
- `POST /api/v1/schedules`
- `POST /api/v1/schedules/:schedule_id`
- `DELETE /api/v1/schedules/:schedule_id`
- `POST /api/v1/schedules/:schedule_id/run_now`
- `POST /api/v1/schedules/:schedule_id/enable`
- `POST /api/v1/schedules/:schedule_id/disable`
- `POST /api/v1/schedules/:schedule_id/runs/:run_id/mark_done`

Stable public aliases exist under `/api/schedules...`.

Share routes:

- `GET /share/:share_id/schedules`
- `POST /share/:share_id/schedules`
- `POST /share/:share_id/schedules/:schedule_id`
- `DELETE /share/:share_id/schedules/:schedule_id`
- `POST /share/:share_id/schedules/:schedule_id/run_now`
- `POST /share/:share_id/schedules/:schedule_id/enable`
- `POST /share/:share_id/schedules/:schedule_id/disable`
- `POST /share/:share_id/schedules/:schedule_id/runs/:run_id/mark_done`

Share route handlers reject target modes other than `existing_session` and reject target sessions outside the share set.

## UI

Owner Nova adds a `Schedules` top action. It opens a drawer/modal defaulting to the selected session, with an `All schedules` tab. The create form uses target mode controls:

- `Use existing session`
- `New session each run`
- `Create once, then reuse`

Shared-session pages add a `Schedules` action that opens the same schedule management surface restricted to the shared session and `Use existing session`.

## Skill

The `codoxear-session` skill adds one `schedule.py` helper with subcommands:

- `list`
- `create`
- `run-now`
- `enable`
- `disable`
- `delete`
- `mark-done`

The helper sends structured JSON to the same owner schedule API. Complex new-session templates can be passed with `--template-json`.
