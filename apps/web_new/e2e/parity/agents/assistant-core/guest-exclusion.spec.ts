// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): guests are excluded from the assistant
// everywhere — no dashboard card, no sidebar rows, and every workspace
// assistant endpoint refuses them (403 on REST, 404 on the event stream)
// — while members proceed normally. A keyed guest still cannot start:
// the create refusal surfaces as a notice. Row: AGT-041.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigPut,
  serverAssistantConfigDelete,
  serverAssistantEventsStatus,
  serverAssistantSendRaw,
  serverAssistantThreadCreate,
  serverAssistantThreadCreateRaw,
  serverAssistantThreadsRaw,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness, seatGuest } from "../support";
import { DUMMY_PROVIDER, assistantToast } from "./support";

const ROWS = ["AGT-041"];

test(
  specTitle(ROWS, "guests see no assistant surfaces and every endpoint refuses them"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt41"));
    const { workspaceSlug } = harness;
    const guest = await test.step("seat a workspace guest", async () => seatGuest(harness));

    await test.step("guest sees no dashboard card", async () => {
      await driver.rulesEnsureSignedIn(guest.email, guest.password, workspaceSlug);
      await driver.assistantOpenHome(workspaceSlug);
      // The card gate rides on the loaded workspace, so settle on the
      // greeting (rendered for every signed-in user once home hydrates)
      // before asserting absence — a bare count would also pass on an
      // unhydrated page.
      await expect.poll(() => driver.homeGreetingHeading(), { timeout: 60_000 }).not.toBeNull();
      expect(await driver.assistantCardVisible()).toBe(false);
    });

    await test.step("guest sidebar stays empty with no placeholder", async () => {
      // The thread list 403s, so the sidebar data stays undefined: neither
      // rows nor the empty placeholder render. The spy proves the refused
      // fetch completed before the absence is asserted.
      await driver.assistantStartApiSpy();
      await driver.assistantOpenLanding(workspaceSlug);
      await expect
        .poll(async () => (await driver.assistantApiCounts()).threadList, { timeout: 60_000 })
        .toBeGreaterThanOrEqual(1);
      await driver.assistantStopApiSpy();
      expect(await driver.assistantSidebarThreads()).toEqual([]);
      expect(await driver.assistantSidebarEmptyVisible()).toBe(false);
    });

    await test.step("assistant REST refuses the guest", async () => {
      const list = await serverAssistantThreadsRaw(workspaceSlug, guest.session);
      expect(list.status).toBe(403);
      expect((list.payload as { error?: unknown }).error).toBe("role_not_allowed");
      const create = await serverAssistantThreadCreateRaw(workspaceSlug, guest.session);
      expect(create.status).toBe(403);
      const ownerThread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      const send = await serverAssistantSendRaw(workspaceSlug, ownerThread.id, "hello", guest.session);
      expect(send.status).toBe(403);
    });

    await test.step("the event stream hides from the guest", async () => {
      const ownerThread = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      expect(await serverAssistantEventsStatus(workspaceSlug, ownerThread.id, guest.session)).toBe(404);
      expect(await serverAssistantEventsStatus(workspaceSlug, ownerThread.id, harness.ownerSession)).toBe(200);
    });

    await test.step("a keyed guest start still fails with a notice", async () => {
      await serverAssistantConfigPut(guest.session, DUMMY_PROVIDER);
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(guest.email, guest.password, workspaceSlug);
      await driver.assistantOpenLanding(workspaceSlug);
      await expect.poll(() => driver.assistantLandingComposerVisible(), { timeout: 60_000 }).toBe(true);
      await driver.assistantFillDraft("guest hello");
      await driver.assistantClickSend();
      const message = await assistantToast(driver, "Unable to start chat");
      expect(message).toContain("The assistant is available to workspace members.");
      expect(await driver.assistantCurrentPath()).toBe(`/${workspaceSlug}/assistant`);
      await serverAssistantConfigDelete(guest.session);
    });

    await test.step("members proceed normally", async () => {
      await driver.resetSession();
      await driver.rulesEnsureSignedIn(harness.owner.email, harness.owner.password, workspaceSlug);
      await driver.assistantOpenHome(workspaceSlug);
      await expect.poll(() => driver.assistantCardVisible(), { timeout: 60_000 }).toBe(true);
    });
  }
);
