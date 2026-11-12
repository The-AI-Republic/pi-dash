// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): user turns align right and assistant
// turns left with markdown; a streaming row with no content yet shows a
// typing placeholder; error turns render as tinted boxes; the list
// auto-scrolls to new messages and shows a placeholder when empty. The
// assistant-markdown and streaming-placeholder halves run at the
// transport contract with stubbed SSE frames (the seeded stack has no
// model backend), disclosed here. Row: AGT-047.
import { test, expect } from "../../fixtures";
import type { AssistantStreamFrame } from "../../drivers/parity-driver";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantPollTerminal,
  serverAssistantSend,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, PROVIDER_UNREACHABLE, expectBubbles } from "./support";

const ROWS = ["AGT-047"];

// The event contract types these as UUIDs (the server only ever emits UUIDs);
// a readable stub id would be rejected before it reached the view.
const STUB_TURN = "47000000-0000-4000-8000-000000000001";
const STUB_MD = "47000000-0000-4000-8000-000000000002";
const STUB_STREAM = "47000000-0000-4000-8000-000000000003";
const scrollId = (seq: number): string => `47000000-0000-4000-8000-${String(seq).padStart(12, "0")}`;

function doneMessage(id: string, role: string, content: string, seq: number): Record<string, unknown> {
  return {
    id,
    role,
    content,
    status: "completed",
    seq,
    turn_id: STUB_TURN,
    payload: {},
    created_at: "2026-01-01T00:00:00Z",
    completed_at: "2026-01-01T00:00:00Z",
  };
}

test(
  specTitle(ROWS, "roles align, markdown renders, errors tint, list follows the tail"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt47"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    await test.step("owner signs in", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
    });

    await test.step("empty transcript shows the placeholder", async () => {
      const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect
        .poll(() => driver.assistantEmptyState(), { timeout: 60_000 })
        .toBe("Ask about your issues, create work, or start a coding run.");
      expect(await driver.assistantBubbles()).toEqual([]);
    });

    await test.step("user and error rows align by role with error detail", async () => {
      const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantSend(workspaceSlug, thread.id, `style seed ${tag}`, harness.ownerSession);
      await serverAssistantPollTerminal(workspaceSlug, thread.id, harness.ownerSession);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      const bubbles = await expectBubbles(driver, (rows) => rows.length === 2);
      expect(bubbles).toEqual([
        { role: "user", text: `style seed ${tag}` },
        { role: "error", text: PROVIDER_UNREACHABLE },
      ]);
      // Error turns are tinted: the probe reads the rendered colour, so it
      // holds for either app's own danger token instead of one class spelling.
      const tint = await driver.assistantErrorTint(1);
      const rgb = /rgba?\((\d+),\s*(\d+),\s*(\d+)/.exec(tint);
      expect(rgb).not.toBeNull();
      const [red, green, blue] = [Number(rgb?.[1]), Number(rgb?.[2]), Number(rgb?.[3])];
      expect(red).toBeGreaterThan(green + 40);
      expect(red).toBeGreaterThan(blue + 40);
    });

    await test.step("assistant markdown renders (stubbed)", async () => {
      const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      const body = `# Styled ${tag}\n\n**bold** plus \`code\``;
      const reply: AssistantStreamFrame = {
        seq: 501,
        kind: "message_completed",
        message: STUB_MD,
        payload: { turn_id: STUB_TURN, message: doneMessage(STUB_MD, "assistant", body, 1) },
      };
      await driver.assistantStubStream(thread.id, [reply]);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expectBubbles(driver, (rows) => rows.length === 1 && rows[0]?.role === "assistant");
      const html = await driver.assistantBubbleHtml(0);
      expect(html).toContain("<h1");
      expect(html).toContain(`Styled ${tag}`);
      expect(html).toContain("<strong>");
      expect(html).toContain("<code>");
      await driver.assistantClearStreamStub(thread.id);
    });

    await test.step("streaming with no content shows the typing placeholder (stubbed)", async () => {
      const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      const open: AssistantStreamFrame = {
        seq: 502,
        kind: "message_created",
        message: STUB_STREAM,
        payload: {
          turn_id: STUB_TURN,
          message: {
            ...doneMessage(STUB_STREAM, "assistant", "", 1),
            status: "streaming",
            completed_at: null,
          },
        },
      };
      await driver.assistantStubStream(thread.id, [open]);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      const bubbles = await expectBubbles(driver, (rows) => rows.length === 1);
      expect(bubbles).toEqual([{ role: "assistant", text: "…" }]);
      await driver.assistantClearStreamStub(thread.id);
    });

    await test.step("the list auto-scrolls to new messages (stubbed)", async () => {
      const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      const frames: AssistantStreamFrame[] = [];
      for (let seq = 1; seq <= 40; seq++) {
        frames.push({
          seq: 600 + seq,
          kind: "message_completed",
          message: scrollId(seq),
          payload: {
            turn_id: STUB_TURN,
            message: doneMessage(scrollId(seq), seq % 2 === 1 ? "user" : "assistant", `Row ${seq}`, seq),
          },
        });
      }
      await driver.assistantStubStream(thread.id, frames);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expectBubbles(driver, (rows) => rows.length === 40);
      // Stop the reconnect storm before measuring: every replay refires
      // the smooth scroll, so the position only settles once the stub is
      // gone and the client sits on the quiet real stream.
      await driver.assistantClearStreamStub(thread.id);
      await expect.poll(() => driver.assistantIsScrolledToBottom(), { timeout: 30_000 }).toBe(true);
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
