// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-188): a full chat flow — landing, thread,
// send, stream, sidebar navigation — never calls the desktop-gated
// profile/token endpoints, and a web session is refused at both with the
// desktop-only code. Row: AGT-059 (network-level negative row).
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantPollTerminal,
  serverAssistantThreadCreate,
  serverDesktopRuntimeAgentProfileRefusal,
  serverDesktopRuntimeAgentTokenRefusal,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER, expectBubbles } from "../assistant-core/support";

const ROWS = ["AGT-059"];

test(
  specTitle(ROWS, "web chat never calls the desktop-gated profile/token endpoints"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt59"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    const thread = await test.step("owner prepares an empty thread", async () =>
      serverAssistantThreadCreate(workspaceSlug, harness.ownerSession));

    await test.step("owner signs in and opens the landing under the watch", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantStartDesktopCallWatch();
      await driver.assistantOpenLanding(workspaceSlug);
      await expect.poll(() => driver.assistantLandingGreeting(), { timeout: 60_000 }).not.toBeNull();
    });

    await test.step("thread send, stream and sidebar navigation stay clean", async () => {
      await driver.assistantOpenThread(workspaceSlug, thread.id);
      await expect.poll(() => driver.assistantEmptyState(), { timeout: 60_000 }).not.toBeNull();
      await driver.assistantFillDraft(`desktop watch ${tag}`);
      await driver.assistantPressEnter();
      await expectBubbles(driver, (rows) => rows.length === 2);
      await serverAssistantPollTerminal(workspaceSlug, thread.id, harness.ownerSession);
      await driver.assistantOpenLanding(workspaceSlug);
      await expect.poll(() => driver.assistantLandingGreeting(), { timeout: 60_000 }).not.toBeNull();
      await driver.assistantClickSidebarThread(0);
      await expect
        .poll(() => driver.assistantCurrentPath(), { timeout: 60_000 })
        .toBe(`/${workspaceSlug}/assistant/${thread.id}`);
      expect(await driver.assistantDesktopCallsObserved()).toEqual([]);
      await driver.assistantStopDesktopCallWatch();
    });

    await test.step("web sessions are refused at both endpoints", async () => {
      const profile = await serverDesktopRuntimeAgentProfileRefusal(harness.ownerSession);
      expect(profile.status).toBe(403);
      expect(profile.error).toBe("desktop_session_required");
      const token = await serverDesktopRuntimeAgentTokenRefusal(harness.ownerSession);
      expect(token.status).toBe(403);
      expect(token.error).toBe("desktop_session_required");
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
