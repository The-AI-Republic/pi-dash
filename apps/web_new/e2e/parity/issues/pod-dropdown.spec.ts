// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-122): pod / execution-target dropdown — the
// "Runs on" row lists pods with a DEFAULT tag plus cloud/managed agent
// rows that disable with a reason when unavailable; picking a pod pins
// the executor and the pod on the server.
// Rows: ISS-218 (pod / execution-target dropdown), ISS-117 (the create
// modal starts on the project's default pod, NEWPIDASH-23; it names the
// project-default agent while sending nothing, NEWPIDASH-45).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  runsProjectDefaultExecutor,
  serverCreateIssueFull,
  serverCreatePod,
  serverCreateProjectWithFlags,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverIssue,
  serverIssues,
  serverPatchProject,
  serverProjectPods,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-218"], "runs-on dropdown lists pods and agents, pins pod on pick"),
  { tag: specTags(["ISS-218"]) },
  async ({ driver, seed, page }) => {
    const tag = `NF122 pods ${Date.now()}`;
    const session = await signInSession(seed.email, seed.password);
    // Fresh projects auto-create one default pod; the scenario adds a tier
    // pod so the picker has two pods plus the agent rows.
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("N122"),
      {},
      session
    );
    const tierSuffix = `Tier${Date.now().toString(36)}`;
    const tier = await serverCreatePod(projectId, tierSuffix, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `${tag} issue`, session);
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("fresh issues arrive pinned to the default pod", async () => {
        // The server pins every new issue to the project's default pod on
        // creation (there is no unpinned state while a default pod exists).
        expect(await driver.propertyRowPresent("Runs on")).toBe(true);
        const pods = await serverProjectPods(projectId, session);
        const auto = pods.find((p) => p.isDefault);
        expect(auto).toBeDefined();
        expect(pods.some((p) => p.id === tier.id)).toBe(true);
        const detail = await serverIssue(seed.workspaceSlug, projectId, issue.id, session);
        expect(detail.assigned_pod_id).toBe(auto?.id);
        await expect.poll(() => driver.propertyValueText("Runs on"), { timeout: 15_000 }).toContain(auto?.name ?? "");
      });

      await test.step("the picker lists agents (disabled, with reasons) and pods", async () => {
        await driver.propertyOpenPicker("Runs on");
        expect(await driver.pickerHasSearch()).toBe(true);
        const options = await driver.pickerOptionTexts();
        expect(options.some((o) => o.includes("Pi Dash Cloud Agent"))).toBe(true);
        expect(options.some((o) => o.includes("Pi Dash Agent (desktop)"))).toBe(true);
        expect(options.some((o) => o.includes(tier.name) && !/default/i.test(o))).toBe(true);
        const defaults = await serverProjectPods(projectId, session);
        const auto = defaults.find((p) => p.isDefault);
        expect(auto).toBeDefined();
        // The old app folds the default tag into the option text; the
        // rebuild tags it with a visible DEFAULT pill that the option-text
        // read skips by design (aria-hidden trailing), so the new leg pins
        // the pill itself (spec §10 gate alignment).
        if (driver.target === "web_new") {
          const autoOption = page.getByRole("listbox").getByRole("option", { name: auto?.name ?? "" });
          await expect(autoOption.getByText("DEFAULT")).toBeVisible();
        } else {
          expect(options.some((o) => o.includes(auto?.name ?? "") && /default/i.test(o))).toBe(true);
        }
        // Neither agent is offered on this stack, so both rows disable with
        // a human-readable reason rendered inline.
        expect(await driver.pickerOptionDisabled("Pi Dash Cloud Agent")).toBe(true);
        expect(await driver.pickerOptionDisabled("Pi Dash Agent (desktop)")).toBe(true);
        // The old app folds the unavailability reason into the option text;
        // the rebuild renders it as trailing pill text the option-text read
        // skips by design, so the new leg reads the whole option (spec §10
        // gate alignment).
        if (driver.target === "web_new") {
          const cloudOption = page.getByRole("listbox").getByRole("option", { name: "Pi Dash Cloud Agent" });
          const cloudText = ((await cloudOption.innerText()) ?? "").trim();
          expect(cloudText.length).toBeGreaterThan("Pi Dash Cloud Agent".length);
        } else {
          expect(options.some((o) => /cloud agent/i.test(o) && o.length > "Pi Dash Cloud Agent".length)).toBe(true);
        }
        await driver.pickerClickOutside();
      });

      await test.step("search narrows to the tier pod", async () => {
        await driver.propertyOpenPicker("Runs on");
        await driver.pickerSearch(tierSuffix);
        const options = await driver.pickerOptionTexts();
        expect(options.some((o) => o.includes(tier.name))).toBe(true);
        expect(options.some((o) => /cloud agent/i.test(o))).toBe(false);
        await driver.pickerPressEscape();
        await driver.pickerClickOutside();
      });

      await test.step("picking a pod pins the pod and the local executor", async () => {
        await driver.propertyOpenPicker("Runs on");
        await driver.pickerPick(tier.name);
        await expect
          .poll(async () => (await serverIssue(seed.workspaceSlug, projectId, issue.id, session)).assigned_pod_id, {
            timeout: 15_000,
          })
          .toBe(tier.id);
        const detail = await serverIssue(seed.workspaceSlug, projectId, issue.id, session);
        expect(detail.agent_executor).toBe("local_runner");
        await expect.poll(() => driver.propertyValueText("Runs on"), { timeout: 15_000 }).toContain(tier.name);
      });
    } finally {
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-117", "ISS-218"], "create modal starts on the default pod and stores it"),
  { tag: specTags(["ISS-117", "ISS-218"]) },
  async ({ driver, seed }) => {
    const tag = `NP23 create pod ${Date.now()}`;
    const name = `${tag} issue`;
    const session = await signInSession(seed.email, seed.password);
    // A fresh project carries exactly one auto-created default pod and the
    // local executor default; a second, non-default pod proves the modal
    // picks the default rather than the first row it lists.
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("NP23"),
      {},
      session
    );
    await serverCreatePod(projectId, `Tier${Date.now().toString(36)}`, session);
    let createdId: string | null = null;
    try {
      const pods = await serverProjectPods(projectId, session);
      const auto = pods.find((p) => p.isDefault);
      expect(auto).toBeDefined();
      await driver.openEntry();
      await driver.signInWithPasswordRetry(seed.email, seed.password);
      await driver.openProjectIssuesSettled(seed.workspaceSlug, projectId);
      await driver.dismissWelcomeDialog();

      await test.step("the untouched form already names the default pod", async () => {
        await driver.openCreateModal();
        await expect.poll(() => driver.modalTextContains(auto?.name ?? "<none>"), { timeout: 15_000 }).toBe(true);
      });

      await test.step("a title-only create stores the pod and the local executor", async () => {
        await driver.fillCreateTitle(name);
        await driver.submitCreateModal();
        await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
        await expect
          .poll(
            async () => (await serverIssues(seed.workspaceSlug, projectId, session)).find((i) => i.name === name)?.id,
            { timeout: 30_000 }
          )
          .toBeTruthy();
        const row = (await serverIssues(seed.workspaceSlug, projectId, session)).find((i) => i.name === name);
        createdId = row?.id ?? null;
        const detail = await serverIssue(seed.workspaceSlug, projectId, createdId ?? "", session);
        expect(detail.assigned_pod_id).toBe(auto?.id);
        expect(detail.agent_executor).toBe("local_runner");
      });
    } finally {
      if (createdId !== null) await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, createdId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-117", "ISS-218"], "create modal names the project-default agent and sends nothing"),
  { tag: specTags(["ISS-117", "ISS-218"]) },
  async ({ driver, seed }) => {
    const tag = `NP45 create agent ${Date.now()}`;
    const name = `${tag} issue`;
    const session = await signInSession(seed.email, seed.password);
    // A cloud default needs a configured cloud agent, which this stack
    // has not got (the ISS-218 leg pins both agent rows disabled), so
    // the scenario defaults to the managed agent instead; the trigger
    // names either default the same way.
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `${tag} project`,
      parityProjectIdentifier("NP45"),
      {},
      session
    );
    let createdId: string | null = null;
    try {
      await serverPatchProject(seed.workspaceSlug, projectId, session, { default_agent_executor: "managed_runner" });
      expect(await runsProjectDefaultExecutor(seed.workspaceSlug, projectId, session)).toBe("managed_runner");
      await driver.openEntry();
      await driver.signInWithPasswordRetry(seed.email, seed.password);
      await driver.openProjectIssuesSettled(seed.workspaceSlug, projectId);
      await driver.dismissWelcomeDialog();

      await test.step("the untouched form already names the default agent", async () => {
        await driver.openCreateModal();
        await expect.poll(() => driver.modalTextContains("Pi Dash Agent (desktop)"), { timeout: 15_000 }).toBe(true);
      });

      await test.step("a title-only create sends no target; the row inherits the default", async () => {
        await driver.fillCreateTitle(name);
        await driver.submitCreateModal();
        await expect.poll(() => driver.createModalOpen(), { timeout: 30_000 }).toBe(false);
        await expect
          .poll(
            async () => (await serverIssues(seed.workspaceSlug, projectId, session)).find((i) => i.name === name)?.id,
            { timeout: 30_000 }
          )
          .toBeTruthy();
        const row = (await serverIssues(seed.workspaceSlug, projectId, session)).find((i) => i.name === name);
        createdId = row?.id ?? null;
        const detail = await serverIssue(seed.workspaceSlug, projectId, createdId ?? "", session);
        // The client sends no execution target (verified on the oracle by
        // intercepting the POST: assigned_pod_id null, no agent_executor
        // key). The server pins the default pod at write (Issue.save) while
        // the NULL executor inherits the managed project default — the same
        // row on both legs.
        const pods = await serverProjectPods(projectId, session);
        expect(detail.assigned_pod_id).toBe(pods.find((p) => p.isDefault)?.id);
        expect(detail.agent_executor).toBeNull();
      });
    } finally {
      if (createdId !== null) await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, createdId, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
