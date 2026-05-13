import assert from "node:assert/strict";
import { isAwaitingAssistantReply } from "../../frontend/src/lib/session-status.ts";

const userEvent = { id: "u1", kind: "user", ts: 10, title: "User", body: "stop now", meta: "" };

assert.equal(isAwaitingAssistantReply([userEvent], 0), true);
assert.equal(
  isAwaitingAssistantReply(
    [
      userEvent,
      { id: "err1", kind: "tool_result", ts: 11, title: "remote", body: "remote error", meta: "error", toolResultIsError: true },
    ],
    0,
  ),
  false,
);
assert.equal(
  isAwaitingAssistantReply(
    [
      userEvent,
      { id: "ext1", kind: "extension", ts: 11, title: "Remote", body: "aborted", meta: "", status: "aborted" },
    ],
    0,
  ),
  false,
);
assert.equal(isAwaitingAssistantReply([userEvent], 0, 12), false);
