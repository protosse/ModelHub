import assert from "node:assert/strict";
import test from "node:test";

import {
  outcomeAfterCancelledRun,
  shouldKeepInMemoryOutcome,
} from "../src/lib/testResultPolicy.ts";

test("a newer in-memory test outcome wins while persistence is queued", () => {
  assert.equal(
    shouldKeepInMemoryOutcome(
      "2026-08-04T12:00:01.000Z",
      "2026-08-04T12:00:00.000Z",
    ),
    true,
  );
  assert.equal(
    shouldKeepInMemoryOutcome(
      "2026-08-04T12:00:00.000Z",
      "2026-08-04T12:00:01.000Z",
    ),
    false,
  );
});

test("a cancelled first run restores the absence of a finished outcome", () => {
  assert.equal(outcomeAfterCancelledRun(null), null);

  const previous = { ok: true, testedAt: "2026-08-04T12:00:00.000Z" };
  assert.equal(outcomeAfterCancelledRun(previous), previous);
});
