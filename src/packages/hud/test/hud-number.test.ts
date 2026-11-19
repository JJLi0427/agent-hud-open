import { test } from "node:test";
import assert from "node:assert/strict";
import type { PanelLayout, Quota } from "@agent-hud/protocol";
import { windowReading } from "../src/held-reading.ts";
import { shownRows } from "../src/quota-rows.ts";
import { configuredVendors, meterLabel } from "../src/model.ts";

const now = Date.parse("2026-11-18T12:00:00Z");
const row = (window: string, usedPercent: number, status: string) =>
  ({
    id: `acct:${window}`,
    vendor: "antigravity",
    name: window,
    usedPercent,
    resetAt: "2026-11-18T17:00:00Z",
    status,
    account: "a@example.com",
    accountId: "acct",
  }) as Quota;
const quotas = [
  row("claude-5h", 100, "critical"),
  row("gemini-5h", 20, "normal"),
];
// What configuredVendors sets on a mark (model.ts `withShownWindows`).
const reading = (blocks: PanelLayout["blocks"]) => {
  const { used, heldUsed, level } = windowReading(
    shownRows(quotas, { blocks } as PanelLayout),
    now,
  );
  return [used, heldUsed, level];
};

test("the HUD number leaves out the windows the panel hides (Open #91)", () => {
  assert.deepEqual(reading({}), [100, null, "critical"]);
  assert.deepEqual(
    reading({ antigravity: { hiddenRows: ["acct:claude-5h"] } }),
    [20, null, "normal"],
  );
  assert.deepEqual(reading({ antigravity: { hiddenAccounts: ["acct"] } }), [
    null,
    null,
    "stale",
  ]);
});

test("a mark whose windows the layout all hid writes its last 24 hours", async () => {
  // Why this exists: OpenAI's one window is a monthly limit the panel's layout
  // hides, so the mark had no window to write and stood over "--" in the
  // Dynamic dock. The panel draws that block's last 24 hours (QuotaSection.tsx)
  // and so must the mark.
  const { tokensShort } = await import("@agent-hud/ui/format");
  void tokensShort;
  const snapshot = {
    language: "en",
    quotas: [
      {
        id: "codex:a:codex",
        vendor: "codex",
        name: "Monthly limit",
        usedPercent: 0,
        status: "normal",
        resetAt: null,
        accountId: "a",
        legacyWindowId: "a/codex",
      },
    ],
    billing: [],
    providers: [
      {
        id: "codex",
        detected: true,
        enabled: true,
        recent: { tokens: 5_700_000, costs: {}, unpricedTokens: 5_700_000 },
      },
    ],
    sessions: [],
    approvals: [],
    sourcePreferences: {},
  } as never;
  const label = (blocks: PanelLayout["blocks"]) => {
    const marks = configuredVendors(snapshot, {
      mode: "notch",
      panelLayout: { blocks },
    } as never);
    const openai = marks.find((mark) => mark.key === "openai");
    assert.ok(openai, "the mark is there");
    return meterLabel(openai, "consumed");
  };
  assert.equal(label({ codex: { hiddenRows: ["a/codex"] } }), "5.7M");
  assert.equal(label({}), "0%", "shown again, the window comes first");
});
