// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): tool-server registry management lives
// in settings, never in chat — the skipped-server notice links nowhere
// and the chat surface links to no settings registry — while the
// registry CRUD stays live server-side. The settings chrome itself is
// rowed under profile/settings. Row: AGT-060.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantMcpCreate,
  serverAssistantMcpDelete,
  serverAssistantMcpList,
  serverAssistantMcpPatch,
  serverAssistantPollTerminal,
  serverAssistantSttDelete,
  serverAssistantSttPut,
  serverAssistantThreadCreate,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DEAD_MCP_URL, DUMMY_PROVIDER, DUMMY_STT, expectBubbles, expectNoticeLines } from "../assistant-core/support";

const ROWS = ["AGT-060"];

test(
  specTitle(ROWS, "chat never manages tool servers; the notice links nowhere"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt60"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores dummy provider and speech keys", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
      // Fully keyed, so the dictation hint (rowed under AGT-051) cannot
      // contribute a settings link to the negative below.
      await serverAssistantSttPut(harness.ownerSession, DUMMY_STT);
    });

    const server = await test.step("owner registers a dead tool server", async () => {
      const row = await serverAssistantMcpCreate(harness.ownerSession, {
        name: `dead-${tag}`,
        url: DEAD_MCP_URL,
      });
      expect((await serverAssistantMcpList(harness.ownerSession)).map((entry) => entry.name)).toEqual([`dead-${tag}`]);
      return row;
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and sends past the dead server", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
      await driver.assistantFillDraft(`registry probe ${tag}`);
      await driver.assistantPressEnter();
      await expectBubbles(driver, (rows) => rows.length === 2);
      await serverAssistantPollTerminal(workspaceSlug, thread.id, harness.ownerSession);
    });

    await test.step("skipped notice renders linkless on replay", async () => {
      // Live SSE frames never land (bug NEWFRONT-197, rowed under
      // AGT-046); the notice re-derives from the replayed event.
      await driver.reloadPage();
      const lines = await expectNoticeLines(driver, (rows) => rows.length === 1);
      expect(lines[0]).toContain(`dead-${tag}`);
      expect(await driver.assistantSkippedNoticeActions()).toEqual([]);
    });

    await test.step("chat links to no settings registry", async () => {
      expect(await driver.assistantChatSettingsLinks()).toEqual([]);
      await driver.assistantOpenLanding(workspaceSlug);
      await expect.poll(() => driver.assistantLandingGreeting(), { timeout: 60_000 }).not.toBeNull();
      expect(await driver.assistantChatSettingsLinks()).toEqual([]);
    });

    await test.step("registry rename, toggle and remove stay live server-side", async () => {
      const renamed = await serverAssistantMcpPatch(server.id, harness.ownerSession, { name: `renamed-${tag}` });
      expect(renamed.name).toBe(`renamed-${tag}`);
      const toggled = await serverAssistantMcpPatch(server.id, harness.ownerSession, { is_enabled: false });
      expect(toggled.is_enabled).toBe(false);
      expect((await serverAssistantMcpList(harness.ownerSession)).map((entry) => entry.name)).toEqual([
        `renamed-${tag}`,
      ]);
      await serverAssistantMcpDelete(server.id, harness.ownerSession);
      expect(await serverAssistantMcpList(harness.ownerSession)).toEqual([]);
    });

    await test.step("both keys are restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
      await serverAssistantSttDelete(harness.ownerSession);
    });
  }
);
