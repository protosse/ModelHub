export function shouldKeepInMemoryOutcome(
  inMemoryTestedAt: string | null | undefined,
  diskTestedAt: string,
): boolean {
  return typeof inMemoryTestedAt === "string" && inMemoryTestedAt > diskTestedAt;
}

export function outcomeAfterCancelledRun<T>(previous: T | null): T | null {
  return previous;
}
