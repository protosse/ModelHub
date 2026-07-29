import assert from "node:assert/strict";
import test from "node:test";

import { pickTestDisplaySource } from "../src/lib/testDisplayPolicy.ts";

test("a completed multi session does not shadow a newer batch result", () => {
  assert.equal(
    pickTestDisplaySource({
      singleRunning: false,
      multiRunning: false,
      batchRunning: false,
      hasLast: true,
    }),
    "last",
  );
});

test("a running session still overrides the shared last result", () => {
  assert.equal(
    pickTestDisplaySource({
      singleRunning: false,
      multiRunning: true,
      batchRunning: false,
      hasLast: true,
    }),
    "multi",
  );
});
