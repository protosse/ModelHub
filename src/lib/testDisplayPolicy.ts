export type TestDisplaySource = "single" | "multi" | "batch" | "last" | "none";

export function pickTestDisplaySource(input: {
  readonly singleRunning: boolean;
  readonly multiRunning: boolean;
  readonly batchRunning: boolean;
  readonly hasLast: boolean;
}): TestDisplaySource {
  if (input.singleRunning) return "single";
  if (input.multiRunning) return "multi";
  if (input.batchRunning) return "batch";
  if (input.hasLast) return "last";
  return "none";
}
