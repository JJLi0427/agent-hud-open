/** A quota window, as far as a held reading needs it. */
interface HeldWindow {
  usedPercent: number | null;
  status: string;
  resetAt?: string | null;
}

/** Whether a window's reset has passed: its share has started over, so its
 * reading no longer says how much is used until the next read (Open #90;
 * vendors.rs `reset_passed`). */
export function resetPassed(
  q: { resetAt?: string | null },
  now: number,
): boolean {
  const reset = q.resetAt ? Date.parse(q.resetAt) : NaN;
  return reset <= now;
}

/** A window that reads live now: a number from the last read, and a reset
 * still ahead. */
export function liveWindow(q: HeldWindow, now: number): boolean {
  return q.usedPercent != null && q.status !== "stale" && !resetPassed(q, now);
}

/**
 * The tightest window of an agent's last reading while every window waits on
 * a failed read ("stale"), so a Simple mark keeps drawing it in the stale
 * grey instead of an empty ring. A window whose reset has passed holds
 * nothing, since its share has started over. Null while any window reads
 * live, or when none has a number (vendors.rs `held`).
 */
export function heldUsed(
  quotas: readonly HeldWindow[],
  now: number,
): number | null {
  if (quotas.some((q) => liveWindow(q, now))) return null;
  let held: number | null = null;
  for (const q of quotas) {
    if (q.usedPercent == null || !Number.isFinite(q.usedPercent)) continue;
    if (resetPassed(q, now)) continue;
    if (held == null || q.usedPercent > held) held = q.usedPercent;
  }
  return held;
}

/** The live window with the highest used percent; the first of equal
 * readings stays the tightest (vendors.rs `tightest`). */
export function tightestWindow<T extends HeldWindow>(
  quotas: readonly T[],
  now: number,
): T | undefined {
  return quotas
    .filter((q) => liveWindow(q, now))
    .reduce<T | undefined>(
      (a, b) => (!a || b.usedPercent! > a.usedPercent! ? b : a),
      undefined,
    );
}

/** What an agent's windows set on its mark: the tightest live window's share
 * and level, else the share its last reading holds, in the stale grey. */
export function windowReading<T extends HeldWindow>(
  quotas: readonly T[],
  now: number,
): { used: number | null; heldUsed: number | null; level: T["status"] } {
  const tightest = tightestWindow(quotas, now);
  return {
    used: tightest?.usedPercent ?? null,
    heldUsed: heldUsed(quotas, now),
    level: tightest?.status ?? "stale",
  };
}

/** An agent whose number and level follow the windows the panel shows: a row
 * the person hid there no longer sets them (Open #91). Its rings already draw
 * only those (`markWindows`). A mark whose every window was hidden has none to
 * write, so it writes its last 24 hours as one that reported no window does —
 * what the panel draws for the same block (QuotaSection.tsx), and what
 * `vendors.rs` `configured` writes for the menu bar. */
export function withShownWindows<T extends { quotas: readonly HeldWindow[] }>(
  vendor: T,
  shown: readonly HeldWindow[],
  now: number,
  usage: () => string | null,
): T & { usage?: string | null } {
  if (shown.length === vendor.quotas.length) return vendor;
  const reading = { ...vendor, ...windowReading(shown, now) };
  return shown.length ? reading : { ...reading, usage: usage() };
}
