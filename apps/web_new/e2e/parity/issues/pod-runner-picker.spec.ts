// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-app-first scenarios (NEWFRONT-907): the pod-or-runner execution
// target picker (hybrid assignment). Each pod offers its squad row plus
// its runners, every runner names its status, owner and — when the server
// refuses it — the reason it cannot take this issue. A runner pick writes
// the pod, the runner and the local executor in one PATCH; a pod pick
// clears the runner; an agent pick clears both.
//
// The OSS backend lists runners without the eligibility answers
// (`assignable`, `not_assignable_code`, `is_busy`, `queue_depth`) and
// drops `assigned_runner_id` on write — both are PDASHOSS01-297 — so the
// scenarios answer those two calls through the browser and keep the rest
// live. Rows: ISS-241, ISS-242.
import { test, expect, type Page } from "../fixtures";
import {
  fetchIssue,
  getProject,
  parityProjectIdentifier,
  serverCleanupIssueWithSession,
  serverCleanupProject,
  serverCleanupRunner,
  serverCreateIssueFull,
  serverCreatePod,
  serverCreateProjectWithFlags,
  serverCreateRunnerV1,
  serverDeleteApiToken,
  serverMintApiToken,
  serverProjectPods,
  serverSetRunnerStatus,
  signInSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** Eligibility answers the stubbed runner read attaches, by runner name. */
type Eligibility = { assignable: boolean; code: string; busy?: boolean; queueDepth?: number };

/**
 * Answer the project runner read with eligibility answers the backend
 * does not compute yet. Every other field stays the live answer.
 */
async function stubRunnerEligibility(page: Page, answers: Record<string, Eligibility>): Promise<void> {
  await page.route("**/api/runners/?*", async (route) => {
    const response = await route.fetch();
    if (!response.ok()) {
      await route.fulfill({ response });
      return;
    }
    const rows = (await response.json()) as Record<string, unknown>[];
    const body = rows.map((row) => {
      const answer = answers[String(row["name"])];
      if (answer === undefined) return row;
      return {
        ...row,
        assignable: answer.assignable,
        not_assignable_code: answer.code,
        is_busy: answer.busy ?? row["status"] === "busy",
        queue_depth: answer.queueDepth ?? 0,
      };
    });
    await route.fulfill({ response, json: body });
  });
}

/**
 * Answer the issue detail with the target the last patch wrote, so a
 * follow-up read names the picked runner exactly as the server would.
 */
async function stubIssueTarget(
  page: Page,
  path: string,
  base: Record<string, unknown>,
  target: { podId: string | null; runnerId: string | null; executor: string | null }
): Promise<void> {
  await page.route(`**${path}`, async (route) => {
    // Only the detail read is answered here; the picker's own writes
    // (and any later stub that owns one) keep their own answer.
    if (route.request().method() !== "GET") {
      await route.fallback();
      return;
    }
    // The live row is read once before the stub is installed: a reload
    // cancels whatever read is in flight, so the answer must not depend on
    // a request the stub itself starts.
    const body = structuredClone(base);
    body["assigned_pod_id"] = target.podId;
    body["assigned_runner_id"] = target.runnerId;
    body["agent_executor"] = target.executor;
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(body),
    });
  });
}

/** Capture the issue PATCH bodies the picker writes. */
interface PatchBody {
  assigned_pod_id: string | null;
  assigned_runner_id: string | null;
  agent_executor: string;
}

/**
 * Capture the picker's writes, and let the caller move the stubbed read
 * to each accepted body. The request event fires before the server
 * answers, so the refetch that follows a save already reads the new
 * target instead of racing the scenario.
 */
function captureTargetPatches(page: Page, path: string, written: PatchBody[], apply: (body: PatchBody) => void): void {
  page.on("request", (request) => {
    if (request.method() !== "PATCH" || !request.url().endsWith(path)) return;
    const body = request.postDataJSON() as Partial<PatchBody>;
    const patch: PatchBody = {
      assigned_pod_id: body.assigned_pod_id ?? null,
      assigned_runner_id: body.assigned_runner_id ?? null,
      agent_executor: body.agent_executor ?? "",
    };
    written.push(patch);
    apply(patch);
  });
}

/**
 * Plant one enrolled runner on the project's default pod under the seed
 * owner. Enrolling through the v1 endpoint sets `enrolled_at` but leaves
 * the row offline until a session connects, so every plant then names the
 * status it wants.
 */
async function plantRunner(
  seed: { workspaceSlug: string },
  token: string,
  identifier: string,
  name: string,
  status: "online" | "busy" | "offline" | "revoked"
): Promise<{ id: string; name: string }> {
  // The v1 create endpoint resolves the project by its identifier, not
  // its UUID, the way `pidash runner add --project` does.
  const planted = await serverCreateRunnerV1(token, {
    project: identifier,
    workspaceSlug: seed.workspaceSlug,
    name,
  });
  await serverSetRunnerStatus(planted.runnerId, status);
  return { id: planted.runnerId, name: planted.runnerName };
}

test(
  specTitle(["ISS-241"], "pod-or-runner picker names status, owner and eligibility"),
  { tag: specTags(["ISS-241"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "the pod-or-runner picker pins the web_new UI only");
    const session = await signInSession(seed.email, seed.password);
    const identifier = parityProjectIdentifier("N907");
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `NF907 picker ${Date.now()}`,
      identifier,
      {},
      session
    );
    const tier = await serverCreatePod(projectId, `Tier${Date.now().toString(36)}`, session);
    const issue = await serverCreateIssueFull(
      seed.workspaceSlug,
      projectId,
      `NF907 picker issue ${Date.now()}`,
      session
    );
    const detailPath = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${issue.id}/`;
    const pods = await serverProjectPods(projectId, session);
    const auto = pods.find((pod) => pod.isDefault);
    expect(auto).toBeDefined();
    // The seeded runners live on the seed project's pod, so this project
    // plants its own squad: two free, one busy, one the caller may not claim.
    const token = await serverMintApiToken(session, `nf907-picker-${Date.now()}`);
    const online = await plantRunner(seed, token.token, identifier, "nf907-online", "online");
    const busy = await plantRunner(seed, token.token, identifier, "nf907-busy", "busy");
    const offline = await plantRunner(seed, token.token, identifier, "nf907-offline", "offline");
    const revoked = await plantRunner(seed, token.token, identifier, "nf907-revoked", "revoked");
    const planted = [online, busy, offline, revoked];
    const writtenTargets: PatchBody[] = [];
    const target = {
      podId: auto?.id ?? null,
      runnerId: null as string | null,
      executor: "local_runner" as string | null,
    };
    const base = await fetchIssue(seed.workspaceSlug, projectId, issue.id, session);
    try {
      captureTargetPatches(driver.page, detailPath, writtenTargets, (patch) => {
        target.podId = patch.assigned_pod_id;
        target.runnerId = patch.assigned_runner_id;
        target.executor = patch.agent_executor;
      });
      await stubIssueTarget(driver.page, detailPath, base, target);
      await stubRunnerEligibility(driver.page, {
        [online.name]: { assignable: true, code: "" },
        [busy.name]: { assignable: true, code: "", busy: true, queueDepth: 2 },
        [offline.name]: { assignable: false, code: "runner_owner_not_on_issue" },
        [revoked.name]: { assignable: false, code: "runner_revoked" },
      });
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);

      await test.step("each pod offers its squad row and its runners", async () => {
        await driver.propertyOpenPicker("Runs on");
        const options = await driver.pickerOptionTexts();
        // A pod's squad row reads as its name; the default pod's row
        // paints a badge after it that only sighted readers see.
        const squadRow = `Any runner in ${auto?.name}`;
        expect(options.some((text) => text.startsWith(squadRow))).toBe(true);
        expect(options.some((text) => text.includes(tier.name))).toBe(true);
        // The default pod's squad row comes first, its runners follow, and
        // the second pod sits after them.
        const squad = options.findIndex((text) => text.startsWith(squadRow));
        const tierRow = options.findIndex((text) => text.includes(tier.name));
        const onlineRow = options.findIndex((text) => text.includes(online.name));
        expect(squad).toBeGreaterThanOrEqual(0);
        expect(onlineRow).toBeGreaterThan(squad);
        expect(onlineRow).toBeLessThan(tierRow);
      });

      await test.step("a runner names its status and its owner", async () => {
        const options = await driver.pickerOptionTexts();
        expect(options.some((text) => new RegExp(`${online.name} · online · idle · Parity Oracle`).test(text))).toBe(
          true
        );
        expect(options.some((text) => new RegExp(`${busy.name} · busy · 2 waiting · Parity Oracle`).test(text))).toBe(
          true
        );
        expect(options.some((text) => new RegExp(`${offline.name} · offline · Parity Oracle`).test(text))).toBe(true);
      });

      await test.step("an ineligible runner disables with the coded reason", async () => {
        expect(await driver.pickerOptionDisabled(offline.name)).toBe(true);
        expect(await driver.pickerOptionDisabled(revoked.name)).toBe(true);
        expect(await driver.pickerOptionDisabled(online.name)).toBe(false);
        // The reason trails the row rather than joining its label.
        expect((await driver.pickerOptionReason(offline.name)) ?? "").toContain(
          "Add the runner's owner as an assignee"
        );
        expect(await driver.pickerOptionReason(revoked.name)).toBe("This runner was revoked.");
        expect(await driver.pickerOptionReason(online.name)).toBeNull();
        await driver.pickerClickOutside();
      });

      await test.step("a busy runner warns without blocking the pick", async () => {
        await driver.propertyOpenPicker("Runs on");
        await driver.pickerPick(busy.name);
        await expect.poll(() => driver.propertyValueText("Runs on"), { timeout: 30_000 }).toContain(busy.name);
        expect(await driver.propertyInlineError("Runs on")).toBeNull();
        await expect
          .poll(() => driver.propertyNote("Runs on"), { timeout: 15_000 })
          .toBe(`${busy.name} runs one issue at a time. This issue will wait behind 2 others.`);
      });

      await test.step("the trigger names the runner and its pod after a reload", async () => {
        const written = writtenTargets[0];
        expect(written?.assigned_runner_id).not.toBeNull();
        await driver.page.reload();
        await expect.poll(() => driver.propertyValueText("Runs on"), { timeout: 60_000 }).toContain(busy.name);
        expect(await driver.propertyValueText("Runs on")).toContain(auto?.name ?? "");
        // Nothing is waiting yet, so the trigger carries no badge.
        expect(await driver.propertyValueText("Runs on")).not.toContain(" ahead");
      });

      await test.step("a queued run puts the waiting badge on the trigger", async () => {
        // The badge reads the active run, not the selection: a queued run
        // behind the pinned busy runner reads "2 ahead" beside the trigger.
        // The OSS serializer drops `dispatch_wait` (PDASHOSS01-297), so the
        // detail read answers with it through the browser.
        const queued = structuredClone(base) as Record<string, unknown>;
        queued["assigned_pod_id"] = target.podId;
        queued["assigned_runner_id"] = busy.id;
        queued["agent_executor"] = "local_runner";
        queued["agent_status"] = {
          ticker: null,
          run_count: 1,
          active_run: {
            id: crypto.randomUUID(),
            status: "queued",
            executor_kind: "local_runner",
            queue_position: null,
            runner: null,
            runner_name: null,
            pinned_runner: busy.id,
            pinned_runner_name: busy.name,
            pin_kind: "dedicated",
            dispatch_wait: {
              reason: "runner_busy",
              pin_kind: "dedicated",
              runner: busy.id,
              runner_name: busy.name,
              queued_at: "2026-10-25T10:00:00Z",
              runner_last_seen_at: null,
              runs_ahead: 2,
              releasable: true,
            },
            created_at: "2026-10-25T10:00:00Z",
            assigned_at: null,
            started_at: null,
            ended_at: null,
            done_payload: null,
            error: "",
            error_code: "",
            error_diagnostic: null,
            llm_model: "",
            input_tokens: null,
            output_tokens: null,
            total_tokens: null,
            live_state: null,
          },
          latest_run: null,
        };
        await driver.page.route(`**${detailPath}`, async (route) => {
          if (route.request().method() !== "GET") {
            await route.fallback();
            return;
          }
          await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(queued) });
        });
        await driver.page.reload();
        await expect.poll(() => driver.propertyRowPresent("Runs on"), { timeout: 60_000 }).toBe(true);
        await expect.poll(() => driver.propertyValueText("Runs on"), { timeout: 30_000 }).toContain("2 ahead");
        const trigger = await driver.propertyValueText("Runs on");
        expect(trigger).toContain(busy.name);
        expect(trigger).toContain(auto?.name ?? "");
      });
    } finally {
      await driver.page.unrouteAll({ behavior: "wait" }).catch(() => {});
      for (const runner of planted) await serverCleanupRunner(runner.id);
      await serverDeleteApiToken(token.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);

test(
  specTitle(["ISS-242"], "execution-target patch writes the runner pin with in-place validation"),
  { tag: specTags(["ISS-242"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "the pod-or-runner picker pins the web_new UI only");
    const session = await signInSession(seed.email, seed.password);
    const identifier = parityProjectIdentifier("N907");
    const projectId = await serverCreateProjectWithFlags(
      seed.workspaceSlug,
      `NF907 exec ${Date.now()}`,
      identifier,
      {},
      session
    );
    const tier = await serverCreatePod(projectId, `Tier${Date.now().toString(36)}`, session);
    const issue = await serverCreateIssueFull(seed.workspaceSlug, projectId, `NF907 exec issue ${Date.now()}`, session);
    const detailPath = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/issues/${issue.id}/`;
    const projectPath = `/api/workspaces/${seed.workspaceSlug}/projects/${projectId}/`;
    const pods = await serverProjectPods(projectId, session);
    const auto = pods.find((pod) => pod.isDefault);
    expect(auto).toBeDefined();
    const token = await serverMintApiToken(session, `nf907-exec-${Date.now()}`);
    const online = await plantRunner(seed, token.token, identifier, "nf907-pin-online", "online");
    // The stored execution target the stubbed detail read answers with;
    // each pick moves it to what the server would persist.
    const target = {
      podId: auto?.id ?? null,
      runnerId: null as string | null,
      executor: "local_runner" as string | null,
    };
    const written: PatchBody[] = [];
    const base = await fetchIssue(seed.workspaceSlug, projectId, issue.id, session);
    const projectBase = await getProject(seed.workspaceSlug, projectId, session);
    try {
      await stubRunnerEligibility(driver.page, { [online.name]: { assignable: true, code: "" } });
      await stubIssueTarget(driver.page, detailPath, base, target);
      // The last step answers its own write with a rejection, so its body
      // must not move the stored target.
      let acceptWrites = true;
      captureTargetPatches(driver.page, detailPath, written, (patch) => {
        if (!acceptWrites) return;
        target.podId = patch.assigned_pod_id;
        target.runnerId = patch.assigned_runner_id;
        target.executor = patch.agent_executor;
      });
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, projectId, issue.id);
      // The sidebar hydrates after the detail paints.
      await expect.poll(() => driver.propertyRowPresent("Runs on"), { timeout: 60_000 }).toBe(true);

      await test.step("a runner pick writes the pod, the runner and the executor", async () => {
        await driver.propertyOpenPicker("Runs on");
        await driver.pickerPick(online.name);
        await expect.poll(() => written.length, { timeout: 30_000 }).toBe(1);
        expect(written[0]).toEqual({
          assigned_pod_id: auto?.id ?? null,
          assigned_runner_id: online.id,
          agent_executor: "local_runner",
        });
        await expect.poll(() => driver.propertyValueText("Runs on"), { timeout: 30_000 }).toContain(online.name);
        expect(await driver.propertyValueText("Runs on")).toContain(auto?.name ?? "");
      });

      await test.step("reloading keeps the runner and its pod", async () => {
        await driver.page.reload();
        await expect.poll(() => driver.propertyValueText("Runs on"), { timeout: 60_000 }).toContain(online.name);
        expect(await driver.propertyValueText("Runs on")).toContain(auto?.name ?? "");
      });

      await test.step("a pod pick clears the runner", async () => {
        await driver.propertyOpenPicker("Runs on");
        await driver.pickerPick(tier.name);
        await expect.poll(() => written.length, { timeout: 30_000 }).toBe(2);
        expect(written[1]).toEqual({
          assigned_pod_id: tier.id,
          assigned_runner_id: null,
          agent_executor: "local_runner",
        });
        await expect.poll(() => driver.propertyValueText("Runs on"), { timeout: 30_000 }).toContain(tier.name);
        expect(await driver.propertyValueText("Runs on")).not.toContain(online.name);
      });

      await test.step("an agent pick clears the pod and the runner", async () => {
        // The OSS server offers no available agent row, so this scenario
        // answers the project read with the cloud row on and accepts the
        // write: the mapping is what ISS-242 proves here.
        await driver.page.route(`**${projectPath}`, async (route) => {
          // Read once up front: the reload below must not race a fetch the
          // stub starts on its own.
          const body = structuredClone(projectBase);
          const options = Array.isArray(body["agent_executor_options"]) ? body["agent_executor_options"] : [];
          body["agent_executor_options"] = (options as Record<string, unknown>[]).map((option) =>
            option["kind"] === "cloud_agent" ? { ...option, available: true, reason_code: "" } : option
          );
          await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
        });
        // The OSS server refuses an unavailable agent on write too, so the
        // pick is accepted here and the stored read answers with it.
        await driver.page.route(`**${detailPath}`, async (route) => {
          if (route.request().method() !== "PATCH") {
            await route.fallback();
            return;
          }
          await route.fulfill({ status: 204 });
        });
        await driver.page.reload();
        await expect.poll(() => driver.propertyRowPresent("Runs on"), { timeout: 60_000 }).toBe(true);
        await driver.propertyOpenPicker("Runs on");
        expect(await driver.pickerOptionDisabled("Pi Dash Cloud Agent")).toBe(false);
        await driver.pickerPick("Pi Dash Cloud Agent");
        await expect.poll(() => written.length, { timeout: 30_000 }).toBe(3);
        expect(written[2]).toEqual({ assigned_pod_id: null, assigned_runner_id: null, agent_executor: "cloud_agent" });
        await expect
          .poll(() => driver.propertyValueText("Runs on"), { timeout: 30_000 })
          .toContain("Pi Dash Cloud Agent");
      });

      await test.step("a rejected write shows the API message in place", async () => {
        acceptWrites = false;
        const message = "That runner is already dedicated to another issue.";
        await driver.page.unroute(`**${projectPath}`).catch(() => {});
        await driver.page.unroute(`**${detailPath}`).catch(() => {});
        await stubIssueTarget(driver.page, detailPath, base, target);
        await driver.page.reload();
        await expect.poll(() => driver.propertyRowPresent("Runs on"), { timeout: 60_000 }).toBe(true);
        await driver.page.route(`**${detailPath}`, async (route) => {
          if (route.request().method() !== "PATCH") {
            await route.fallback();
            return;
          }
          await route.fulfill({
            status: 400,
            contentType: "application/json",
            body: JSON.stringify({ assigned_runner_id: [message] }),
          });
        });
        await driver.propertyOpenPicker("Runs on");
        await driver.pickerPick(online.name);
        await expect.poll(() => driver.propertyInlineError("Runs on"), { timeout: 30_000 }).toBe(message);
        // A field rejection stays next to the trigger instead of toasting.
        expect(await driver.toastText()).toBeNull();
        // The optimistic value rolled back to the stored target.
        await expect
          .poll(() => driver.propertyValueText("Runs on"), { timeout: 30_000 })
          .toContain("Pi Dash Cloud Agent");
      });
    } finally {
      await driver.page.unrouteAll({ behavior: "wait" }).catch(() => {});
      await serverCleanupRunner(online.id);
      await serverDeleteApiToken(token.id, session);
      await serverCleanupIssueWithSession(seed.workspaceSlug, projectId, issue.id, session);
      await serverCleanupProject(seed.workspaceSlug, projectId, session);
    }
  }
);
