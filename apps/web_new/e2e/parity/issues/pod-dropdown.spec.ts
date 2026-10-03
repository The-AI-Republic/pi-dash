// Copyright (c) Pi Dash contributors. All rights reserved.
// License for this tree is pending the H-license decision (NEWFRONT-2,
// owner: human). This placeholder grants no license and must be replaced
// with the final header text by F-11 (NEWFRONT-22).
// Oracle scenarios (NEWFRONT-122): pod / execution-target dropdown — the
// "Runs on" row lists pods with a DEFAULT tag plus cloud/managed agent
// rows that disable with a reason when unavailable; picking a pod pins
// the executor and the pod on the server.
// Rows: ISS-218 (pod / execution-target dropdown).
import { test, expect } from "../fixtures";
import {
  parityProjectIdentifier,
  serverCreateIssueFull,
  serverCreatePod,
  serverCreateProjectWithFlags,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverIssue,
  serverProjectPods,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

test(
  specTitle(["ISS-218"], "runs-on dropdown lists pods and agents, pins pod on pick"),
  { tag: specTags(["ISS-218"]) },
  async ({ driver, seed }) => {
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
        expect(options.some((o) => o.includes(auto?.name ?? "") && /default/i.test(o))).toBe(true);
        // Neither agent is offered on this stack, so both rows disable with
        // a human-readable reason rendered inline.
        expect(await driver.pickerOptionDisabled("Pi Dash Cloud Agent")).toBe(true);
        expect(await driver.pickerOptionDisabled("Pi Dash Agent (desktop)")).toBe(true);
        expect(options.some((o) => /cloud agent/i.test(o) && o.length > "Pi Dash Cloud Agent".length)).toBe(true);
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
