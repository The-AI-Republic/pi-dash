// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenario (NEWFRONT-187): the transcript read pages by cursor
// with an explicit limit, and the thread view renders every row exactly
// once — streamed events and fetched pages merge without duplicates.
// (The client's beyond-100 multi-fetch loop is unprovable here: sends
// throttle at 30/hour per user while the loop needs 50 sends in one
// thread, so that half stays a Gap in the row.) Row: AGT-055.
import { test, expect } from "../../fixtures";
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
import { DUMMY_PROVIDER, expectBubbles } from "./support";

const ROWS = ["AGT-055"];
const SENDS = 6;

test(
  specTitle(ROWS, "transcript pages by cursor and renders every row once"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt55"));
    const { owner, workspaceSlug, tag } = harness;

    await test.step("owner stores a dummy provider key", async () => {
      await serverAssistantConfigPut(harness.ownerSession, DUMMY_PROVIDER);
    });

    const thread = await test.step(`owner seeds ${SENDS} exchanges`, async () => {
      const roomy = await serverAssistantThreadCreate(workspaceSlug, harness.ownerSession);
      for (let turn = 1; turn <= SENDS; turn++) {
        await serverAssistantSend(workspaceSlug, roomy.id, `page seed ${tag} ${turn}`, harness.ownerSession);
        await serverAssistantPollTerminal(workspaceSlug, roomy.id, harness.ownerSession);
      }
      return roomy;
    });

    await test.step("cursor pages slice the transcript", async () => {
      const session = harness.ownerSession;
      const first = await serverAssistantMessages(workspaceSlug, thread.id, session, 0, 5);
      expect(first.map((row) => row.seq)).toEqual([1, 2, 3, 4, 5]);
      const second = await serverAssistantMessages(workspaceSlug, thread.id, session, 5, 5);
      expect(second.map((row) => row.seq)).toEqual([6, 7, 8, 9, 10]);
      const rest = await serverAssistantMessages(workspaceSlug, thread.id, session, 10, 5);
      expect(rest.map((row) => row.seq)).toEqual([11, 12]);
      const pair = await serverAssistantMessages(workspaceSlug, thread.id, session, 0, 2);
      expect(pair.map((row) => row.seq)).toEqual([1, 2]);
      expect(pair[0]?.content).toBe(`page seed ${tag} 1`);
    });

    await test.step("owner signs in and opens the thread", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.assistantOpenThread(workspaceSlug, thread.id);
    });

    await test.step("every row renders exactly once in order", async () => {
      const apiRows = await serverAssistantMessages(workspaceSlug, thread.id, harness.ownerSession, 0, 200);
      expect(apiRows.length).toBe(SENDS * 2);
      const bubbles = await expectBubbles(driver, (rows) => rows.length === apiRows.length);
      expect(bubbles.map((row) => row.role)).toEqual(apiRows.map((row) => row.role));
      expect(bubbles[0]).toEqual({ role: "user", text: `page seed ${tag} 1` });
      // Streamed events and fetched pages meet in one keyed merge: equal
      // counts with no unknown rows means no duplicates either way.
      expect(bubbles.every((row) => row.role !== "unknown")).toBe(true);
    });

    await test.step("provider key is restored to absent", async () => {
      await serverAssistantConfigDelete(harness.ownerSession);
    });
  }
);
