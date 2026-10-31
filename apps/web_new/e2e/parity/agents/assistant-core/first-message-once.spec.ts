// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): the landing hands the first message to
// the thread view as one-shot navigation state, so it is delivered
// exactly once — refresh and back-navigation never resend or duplicate
// it. Row: AGT-053.
import { test, expect } from "../../fixtures";
import {
  serverAssistantConfigDelete,
  serverAssistantConfigPut,
  serverAssistantMessages,
  serverAssistantPollTerminal,
  serverAssistantThreads,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { DUMMY_PROVIDER } from "./support";

const ROWS = ["AGT-053"];

test(
  specTitle(ROWS, "landing first message is delivered exactly once per thread"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt53"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    await test.step("owner signs in and opens the landing", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenLanding(workspaceSlug);
    });

    const firstMessage = `deliver once ${tag}`;
    const threadId = await test.step("start delivers the draft once", async () => {
      await driver.assistantFillDraft(firstMessage);
      await driver.assistantPressEnter();
      let path = "";
      await expect
        .poll(
          async () => {
            path = await driver.assistantCurrentPath();
            return path.startsWith(`/${workspaceSlug}/assistant/`) && path !== `/${workspaceSlug}/assistant`;
          },
          { timeout: 60_000 }
        )
        .toBe(true);
      const id = path.split("/").pop() ?? "";
      const messages = await serverAssistantPollTerminal(workspaceSlug, id, harness.ownerSession);
      expect(messages.filter((row) => row.role === "user").map((row) => row.content)).toEqual([firstMessage]);
      return id;
    });

    await test.step("refresh never resends", async () => {
      await driver.assistantOpenThread(workspaceSlug, threadId);
      // Let the transcript and a full poll cycle settle, then recount.
      await expect.poll(() => driver.assistantBubbles(), { timeout: 60_000 }).not.toEqual([]);
      const messages = await serverAssistantMessages(workspaceSlug, threadId, harness.ownerSession);
      expect(messages.filter((row) => row.role === "user").length).toBe(1);
      expect((await serverAssistantThreads(workspaceSlug, harness.ownerSession)).length).toBe(1);
    });

    await test.step("back-navigation creates no duplicate", async () => {
      // History: landing, then the thread entry whose handoff state was
      // replaced away (the same-URL refresh goto reloaded in place). Back
      // lands on the landing with nothing to resend.
      await driver.assistantGoBack();
      await expect.poll(() => driver.assistantCurrentPath(), { timeout: 60_000 }).toBe(`/${workspaceSlug}/assistant`);
      expect((await serverAssistantThreads(workspaceSlug, harness.ownerSession)).length).toBe(1);
      const messages = await serverAssistantMessages(workspaceSlug, threadId, harness.ownerSession);
      expect(messages.filter((row) => row.role === "user").length).toBe(1);
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
