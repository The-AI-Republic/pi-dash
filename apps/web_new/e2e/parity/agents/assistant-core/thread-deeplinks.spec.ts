// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): a per-thread deep link loads that
// transcript on a fresh visit; an unknown thread id renders the empty
// state without touching the sidebar; a tool-result deep link lands on
// its thread with that transcript. (Dashboard recents land likewise,
// proven in AGT-048.) Row: AGT-045.
import { randomUUID } from "node:crypto";
import { test, expect } from "../../fixtures";
import type { AssistantStreamFrame } from "../../drivers/parity-driver";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantMessages,
  serverAssistantPollTerminal,
  serverAssistantSend,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, expectBubbles, expectSidebarTitles } from "./support";

const ROWS = ["AGT-045"];

test(
  specTitle(ROWS, "thread links load transcripts, tool links land on threads"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt45"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    const first = await test.step("owner prepares a messaged thread", async () => {
      const thread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      await serverAssistantSend(workspaceSlug, thread.id, `deeplink seed ${tag}`, harness.ownerSession);
      await serverAssistantPollTerminal(workspaceSlug, thread.id, harness.ownerSession);
      return thread;
    });

    await test.step("owner signs in", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
    });

    await test.step("a deep link loads that transcript fresh", async () => {
      await driver.assistantOpenThread(workspaceSlug, first.id);
      const apiRows = await serverAssistantMessages(workspaceSlug, first.id, harness.ownerSession);
      const bubbles = await expectBubbles(driver, (rows) => rows.length === apiRows.length);
      expect(bubbles.map((row) => row.role)).toEqual(apiRows.map((row) => row.role));
      expect(bubbles[0]?.text).toBe(`deeplink seed ${tag}`);
    });

    await test.step("an unknown thread renders the empty state", async () => {
      await driver.assistantOpenThread(workspaceSlug, randomUUID());
      await expect
        .poll(() => driver.assistantEmptyState(), { timeout: 60_000 })
        .toBe("Ask about your issues, create work, or start a coding run.");
      expect(await driver.assistantBubbles()).toEqual([]);
      // The sidebar still lists the real threads.
      await expectSidebarTitles(driver, [`deeplink seed ${tag}`]);
    });

    await test.step("a tool-result link lands on its thread", async () => {
      const second = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      const tool: AssistantStreamFrame = {
        seq: 201,
        kind: "tool_result",
        message: "stub-tool1",
        payload: {
          turn_id: "stub-turn",
          message: {
            id: "stub-tool1",
            role: "tool_result",
            content: `Looked up ${tag}`,
            status: "completed",
            seq: 1,
            turn_id: "stub-turn",
            payload: {
              links: [{ url_path: `/${workspaceSlug}/assistant/${first.id}`, label: "Open thread" }],
            },
            created_at: "2026-01-01T00:00:00Z",
            completed_at: "2026-01-01T00:00:00Z",
          },
        },
      };
      await driver.assistantStubStream(second.id, [tool]);
      await driver.assistantOpenThread(workspaceSlug, second.id);
      await expectBubbles(driver, (rows) => rows.some((row) => row.role === "tool"));
      const activities = await driver.assistantToolActivities();
      expect(activities).toEqual([
        {
          text: `Looked up ${tag}`,
          links: [{ label: "Open thread", href: `/${workspaceSlug}/assistant/${first.id}` }],
        },
      ]);
      await driver.assistantClickToolLink(0, 0);
      await expect
        .poll(() => driver.assistantCurrentPath(), { timeout: 60_000 })
        .toBe(`/${workspaceSlug}/assistant/${first.id}`);
      await driver.assistantClearStreamStub(second.id);
      const bubbles = await expectBubbles(driver, (rows) => rows.length === 2);
      expect(bubbles[0]).toEqual({ role: "user", text: `deeplink seed ${tag}` });
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
