import { test } from "node:test";
import assert from "node:assert/strict";
import type { Quota } from "@agent-hud/protocol";
import {
  blockHasReading,
  inRowOrder,
  quotaRowHidden,
  quotaRowKey,
  withRowShown,
} from "../src/quota-rows.ts";

const account = "account:aaa";
const other = "account:bbb";
const row = (window: string, accountId = account): Quota =>
  ({
    id: `claude:${accountId}:${window}`,
    vendor: "claude",
    name: window,
    usedPercent: 0,
    status: "normal",
    accountId,
    legacyWindowId: `${accountId}/${window}`,
  }) as Quota;

const session = row("claude-session");
const weekly = row("claude-weekly");
const fable = row("claude-weekly-fable");
const otherFable = row("claude-weekly-fable", other);

test("a layout writes a row by its Swift window id", () => {
  assert.equal(quotaRowKey(fable), `${account}/claude-weekly-fable`);
});

test("a row is hidden by its id, its window id, or its window alone", () => {
  assert.ok(quotaRowHidden(fable, [fable.id]));
  assert.ok(quotaRowHidden(fable, [`${account}/claude-weekly-fable`]));
  assert.ok(quotaRowHidden(fable, ["claude-weekly-fable"]));
  assert.ok(!quotaRowHidden(weekly, ["claude-weekly-fable"]));
});

test("rows follow a layout's order whichever key names them", () => {
  const ordered = inRowOrder(
    [session, weekly, fable],
    [fable.id, `${account}/claude-weekly`],
  );
  assert.deepEqual(
    ordered.map((r) => r.name),
    ["claude-weekly-fable", "claude-weekly", "claude-session"],
  );
});

test("showing a row hidden by its window alone keeps the other account's row hidden", () => {
  const hidden = withRowShown(["claude-weekly-fable", session.id], fable, [
    session,
    fable,
    otherFable,
  ]);
  assert.deepEqual(hidden, [session.id, `${other}/claude-weekly-fable`]);
  assert.ok(!quotaRowHidden(fable, hidden));
  assert.ok(quotaRowHidden(otherFable, hidden));
});

test("a block whose rows its layout all hid has no reading to draw", () => {
  // Codex's one window, hidden in the panel's layout: the block has a
  // reading, but none it may show, so it draws its last 24 hours instead of
  // a heading over nothing.
  const layout = {
    hidden: false,
    hiddenRows: [quotaRowKey(session)],
    rows: [quotaRowKey(session)],
  };
  assert.equal(blockHasReading([session], undefined), true);
  assert.equal(blockHasReading([session], { hidden: false }), true);
  assert.equal(blockHasReading([session], layout), false);
  // Another account's window is not the row that key hid.
  assert.equal(blockHasReading([otherFable], layout), true);
});
