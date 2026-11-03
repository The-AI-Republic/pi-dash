// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-188): the issue description editor offers
// the AI helper only when the instance reports an LLM key; a typed
// request posts the wire shape, renders the returned markup for review,
// and inserts it on confirmation; empty answers mark invalid; quota and
// other failures toast distinctly; closing discards the draft request.
// The generated answer runs at the transport contract (stubbed; the
// seeded stack configures no LLM backend), disclosed here — while the
// entry gate and the failure halves prove the honest stack. The gate
// reads once at app boot, so the hidden and shown halves run as two
// tests (fresh boot each). Row: AGT-063.
import { test, expect } from "../../fixtures";
import { serverCreateProject, serverInstanceLlmConfigured, serverWorkspaceGptRaw } from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { schedulerHarness } from "../support";
import { assistantToast } from "../assistant-core/support";

const ROWS = ["AGT-063"];

test(
  specTitle(ROWS, "keyless instance hides the issue editor helper entry"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt63h"));
    const { owner, workspaceSlug, tag } = harness;
    const projectId = await test.step("owner creates a project", async () =>
      serverCreateProject(
        workspaceSlug,
        harness.ownerSession,
        `Parity AGT63H ${tag}`,
        `AH${tag.slice(0, 3).toUpperCase()}`
      ));

    await test.step("honest stack configures no LLM backend", async () => {
      expect(await serverInstanceLlmConfigured()).toBe(false);
      const refused = await serverWorkspaceGptRaw(workspaceSlug, harness.ownerSession, {
        prompt: "",
        task: "anything",
      });
      expect(refused.status).toBe(400);
      expect((refused.payload as { error?: unknown }).error).toBe("LLM provider API key and model are required");
    });

    await test.step("helper entry hides with no instance key", async () => {
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.issueModalOpenCreate(workspaceSlug, projectId);
      expect(await driver.issueModalAiEntryVisible()).toBe(false);
    });
  }
);

test(
  specTitle(ROWS, "issue editor helper reviews, inserts, and fails distinctly"),
  { tag: specTags(ROWS) },
  async ({ driver }) => {
    // Modal setup alone costs ~3min on this stack; six helper halves follow.
    test.setTimeout(600_000);
    const harness = await test.step("fresh owner plus workspace", async () => schedulerHarness("parity-agt63"));
    const { owner, workspaceSlug, tag } = harness;
    const projectId = await test.step("owner creates a project", async () =>
      serverCreateProject(
        workspaceSlug,
        harness.ownerSession,
        `Parity AGT63 ${tag}`,
        `AG${tag.slice(0, 3).toUpperCase()}`
      ));

    await test.step("owner signs in under a configured instance", async () => {
      await driver.assistantStubInstanceLlm(true);
      await driver.rulesEnsureSignedIn(owner.email, owner.password, workspaceSlug);
      await driver.issueModalOpenCreate(workspaceSlug, projectId);
      expect(await driver.issueModalAiEntryVisible()).toBe(true);
    });

    await test.step("typed request posts the wire shape and reviews", async () => {
      await driver.assistantStubGptAnswer({
        response: `canned ${tag}`,
        response_html: `<p>canned ${tag}</p>`,
      });
      await driver.issueModalAiOpen();
      await driver.issueModalAiFillTask(`summarize ${tag}`);
      await driver.issueModalAiGenerate();
      await expect.poll(() => driver.issueModalAiResponse(), { timeout: 60_000 }).toContain(`canned ${tag}`);
      // The caller passes no prompt: the draft stays out of the request.
      expect(await driver.assistantGptRequests()).toEqual([{ prompt: "", task: `summarize ${tag}` }]);
    });

    await test.step("confirmation inserts the answer into the draft", async () => {
      await driver.issueModalAiUseResponse();
      await expect.poll(() => driver.issueModalDescriptionText(), { timeout: 30_000 }).toContain(`canned ${tag}`);
    });

    await test.step("closing discards the draft request", async () => {
      await driver.issueModalAiOpen();
      await driver.issueModalAiFillTask(`abandoned ${tag}`);
      await driver.issueModalAiClose();
      await driver.issueModalAiOpen();
      await driver.issueModalAiGenerate();
      expect(await assistantToast(driver, "Error!")).toBe("Please enter some task to get AI assistance.");
      expect((await driver.assistantGptRequests()).length).toBe(1);
      await expect.poll(() => driver.assistantLastToast(), { timeout: 30_000 }).toBeNull();
      await driver.issueModalAiClose();
    });

    await test.step("empty answers mark invalid", async () => {
      await driver.assistantStubGptAnswer({ response: "", response_html: "" });
      await driver.issueModalAiOpen();
      await driver.issueModalAiFillTask(`summarize ${tag}`);
      await driver.issueModalAiGenerate();
      await expect.poll(() => driver.issueModalAiInvalidVisible(), { timeout: 60_000 }).toBe(true);
      expect((await driver.assistantGptRequests()).length).toBe(2);
      await driver.issueModalAiClose();
    });

    await test.step("quota failures toast the quota line", async () => {
      await driver.assistantFailGptAnswer(429, {});
      await driver.issueModalAiOpen();
      await driver.issueModalAiFillTask(`summarize ${tag}`);
      await driver.issueModalAiGenerate();
      expect(await assistantToast(driver, "Error!")).toBe(
        "You have reached the maximum number of requests of 50 requests per month per user."
      );
      expect((await driver.assistantGptRequests()).length).toBe(3);
      await expect.poll(() => driver.assistantLastToast(), { timeout: 30_000 }).toBeNull();
      await driver.issueModalAiClose();
    });

    await test.step("backend failures toast the backend line", async () => {
      await driver.assistantClearGptStubs();
      await driver.issueModalAiOpen();
      await driver.issueModalAiFillTask(`summarize ${tag}`);
      await driver.issueModalAiGenerate();
      expect(await assistantToast(driver, "Error!")).toBe("LLM provider API key and model are required");
      await driver.issueModalAiClose();
      await driver.assistantClearInstanceStub();
    });
  }
);
