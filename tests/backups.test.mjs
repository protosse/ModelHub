import assert from "node:assert/strict";
import test from "node:test";

import {
  groupBackupSnapshots,
  parseBackupStamp,
  snapshotDirFromPath,
} from "../src/lib/backups.ts";

test("groups backup files by agent and stamp", () => {
  const snapshots = groupBackupSnapshots([
    {
      agent: "pi",
      stamp: "20260729-010203-004",
      fileName: "settings.json",
      path: "/tmp/backups/pi/20260729-010203-004/settings.json",
    },
    {
      agent: "pi",
      stamp: "20260729-010203-004",
      fileName: "models.json",
      path: "/tmp/backups/pi/20260729-010203-004/models.json",
    },
    {
      agent: "claude",
      stamp: "20260728-010203-004",
      fileName: "settings.json",
      path: "/tmp/backups/claude/20260728-010203-004/settings.json",
    },
  ]);

  assert.equal(snapshots.length, 2);
  assert.deepEqual(
    snapshots[0].files.map((file) => file.fileName),
    ["models.json", "settings.json"],
  );
});

test("extracts snapshot directories from Windows and Unix paths", () => {
  assert.equal(snapshotDirFromPath("/a/b/settings.json"), "/a/b");
  assert.equal(
    snapshotDirFromPath("C:\\Users\\me\\backup\\settings.json"),
    "C:\\Users\\me\\backup",
  );
});

test("parses valid UTC stamps and rejects normalized invalid dates", () => {
  assert.equal(
    parseBackupStamp("20260729-010203-004")?.toISOString(),
    "2026-07-29T01:02:03.004Z",
  );
  assert.equal(parseBackupStamp("20260230-010203-004"), null);
  assert.equal(parseBackupStamp("20261301-010203-004"), null);
});
