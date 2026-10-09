// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): servers a run drops surface as inline
// per-turn notice lines — one per server with a plain-language reason —
// never as toasts, and reload re-derives them from the replayed event.
// bug: NEWFRONT-197 — live SSE frames are corrupt server-side so the
// client drops every live event: notices never render mid-turn and
// appear only once a reload replays the clean persisted event. Intended
// is mid-turn inline notices. Tool calls and results render as activity
// rows with deep links; the whole-outage and mapped-reason phrasings are
// proven at the transport contract with stubbed SSE frames (the seeded
// stack's guard-off build cannot emit those codes), disclosed here.
// Row: AGT-046.
import { test, expect } from "../../fixtures";
import type { AssistantStreamFrame } from "../../drivers/parity-driver";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantMcpCreate,
  serverAssistantMcpDelete,
  serverAssistantMcpList,
  serverAssistantMessages,
  serverAssistantPollTerminal,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DEAD_MCP_URL, DUMMY_PROVIDER, expectBubbles, expectNoticeLines } from "./support";

const ROWS = ["AGT-046"];

function skippedFrame(seq: number, servers: { name: string; reason: string }[]): AssistantStreamFrame {
  return { seq, kind: "tool_servers_skipped", payload: { servers } };
}

test(
  specTitle(ROWS, "bug: NEWFRONT-197 notices surface on replay, never live; stubbed rows hold"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt46"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    const servers = await test.step("owner registers two dead tool servers", async () => {
      const first = await serverAssistantMcpCreate(harness.ownerSession, {
        name: `dead-a-${tag}`,
        url: DEAD_MCP_URL,
      });
      const second = await serverAssistantMcpCreate(harness.ownerSession, {
        name: `dead-b-${tag}`,
        url: DEAD_MCP_URL,
      });
      expect((await serverAssistantMcpList(harness.ownerSession)).map((row) => row.name)).toEqual([
        `dead-a-${tag}`,
        `dead-b-${tag}`,
      ]);
      return [first, second];
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
    });

    const expected = [
      `Tool server dead-a-${tag} was unavailable for this reply (RuntimeError).`,
      `Tool server dead-b-${tag} was unavailable for this reply (RuntimeError).`,
    ];
    await test.step("bug: dropped servers never notice mid-turn, nor toast", async () => {
      await driver.assistantFillDraft(`tools probe ${tag}`);
      await driver.assistantPressEnter();
      await serverAssistantPollTerminal(workspaceSlug, thread.id, harness.ownerSession);
      // The turn is terminal server-side and the rows rendered, yet no
      // live notice arrived: the skipped event was dropped with every
      // other live event (bug NEWFRONT-197).
      await expectBubbles(driver, (rows) => rows.length === 2);
      expect(await driver.assistantNoticeLines()).toEqual([]);
      expect(await driver.assistantLastToast()).toBeNull();
    });

    await test.step("reload re-derives the notices from the replay", async () => {
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      expect(await expectNoticeLines(driver, (lines) => lines.length === 2)).toEqual(expected);
      expect(await driver.assistantLastToast()).toBeNull();
      const messages = await serverAssistantMessages(workspaceSlug, thread.id, harness.ownerSession);
      // Notices are client-only: the transcript holds just the two rows.
      expect(messages.map((row) => row.role)).toEqual(["user", "error"]);
    });

    await test.step("mapped reasons render in plain language (stubbed)", async () => {
      const mapped = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantStubStream(mapped.id, [
        skippedFrame(301, [{ name: `closed-${tag}`, reason: "url_blocked" }]),
      ]);
      await driver.assistantOpenThread(workspaceSlug, mapped.id);
      expect(await expectNoticeLines(driver, (lines) => lines.length === 1)).toEqual([
        `Tool server closed-${tag} was unavailable for this reply (its URL is not allowed).`,
      ]);
      await driver.assistantClearStreamStub(mapped.id);
    });

    await test.step("a whole outage phrases as one capability line (stubbed)", async () => {
      const outage = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantStubStream(outage.id, [
        skippedFrame(302, [{ name: "all tool servers", reason: "toolsets_unavailable" }]),
      ]);
      await driver.assistantOpenThread(workspaceSlug, outage.id);
      expect(await expectNoticeLines(driver, (lines) => lines.length === 1)).toEqual([
        "Tool servers were unavailable for this reply (they could not be loaded).",
      ]);
      await driver.assistantClearStreamStub(outage.id);
    });

    await test.step("tool calls and results render with deep links (stubbed)", async () => {
      const tools = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      const call: AssistantStreamFrame = {
        seq: 401,
        kind: "tool_call",
        message: "stub-call1",
        payload: {
          turn_id: "stub-turn",
          message: {
            id: "stub-call1",
            role: "tool_call",
            content: `Searching ${tag}`,
            status: "completed",
            seq: 1,
            turn_id: "stub-turn",
            payload: {},
            created_at: "2026-01-01T00:00:00Z",
            completed_at: "2026-01-01T00:00:00Z",
          },
        },
      };
      const result: AssistantStreamFrame = {
        seq: 402,
        kind: "tool_result",
        message: "stub-result1",
        payload: {
          turn_id: "stub-turn",
          message: {
            id: "stub-result1",
            role: "tool_result",
            content: `Found ${tag}`,
            status: "completed",
            seq: 2,
            turn_id: "stub-turn",
            payload: { links: [{ url_path: `/${workspaceSlug}/assistant/${thread.id}`, label: "Open thread" }] },
            created_at: "2026-01-01T00:00:00Z",
            completed_at: "2026-01-01T00:00:00Z",
          },
        },
      };
      await driver.assistantStubStream(tools.id, [call, result]);
      await driver.assistantOpenThread(workspaceSlug, tools.id);
      await expectBubbles(driver, (rows) => rows.filter((row) => row.role === "tool").length === 2);
      expect(await driver.assistantToolActivities()).toEqual([
        { text: `Searching ${tag}`, links: [] },
        {
          text: `Found ${tag}`,
          links: [{ label: "Open thread", href: `/${workspaceSlug}/assistant/${thread.id}` }],
        },
      ]);
      await driver.assistantClearStreamStub(tools.id);
    });

    await test.step("registry and provider key are restored", async () => {
      await serverAssistantMcpDelete(servers[0]?.id ?? "", harness.ownerSession);
      await serverAssistantMcpDelete(servers[1]?.id ?? "", harness.ownerSession);
      expect(await serverAssistantMcpList(harness.ownerSession)).toEqual([]);
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
