// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// New-app-first scenarios (NEWFRONT-907): the waiting-for-runner banner on
// the issue detail. While the issue's active run is queued behind a pinned
// runner, the detail names the runner and the reason, and offers the three
// ways out — release the run to the pod once, change the issue's runner,
// or cancel the run. The old UI gains the surface with PDASHOSS01-299, so
// these run on apps/web_new only.
//
// The OSS backend does not yet compute `agent_status.active_run.dispatch_wait`
// (that is PDASHOSS01-297), so each scenario answers the issue-detail read
// through the browser and leaves every other call live. Rows: ISS-239,
// ISS-240.
import { test, expect, type Page } from "../fixtures";
import { fetchIssue, serverCreateIssueFull, serverCleanupIssueWithSession, signInSession } from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/** The waiting run each stubbed detail read answers with. */
interface WaitingStub {
  reason: string;
  runnerName: string;
  runsAhead?: number;
  runnerLastSeenAt?: string | null;
  releasable?: boolean;
  /** Set for read-only viewers: the issue reads as archived. */
  archived?: boolean;
  podId?: string;
  /** The run id the last answer carried, for endpoint assertions. */
  runId: string;
}

/**
 * Answer the issue-detail read with the live row plus a queued pinned
 * run, re-reading the live row each time so the rest of the page (state,
 * permissions, project) stays real. Mutating the stub between reloads
 * changes what the banner says.
 */
async function stubWaitingDetail(
  page: Page,
  path: string,
  base: Record<string, unknown>,
  stub: WaitingStub
): Promise<void> {
  await page.route(`**${path}`, async (route) => {
    // Only the detail read is answered here; every other method keeps
    // the live server's answer.
    if (route.request().method() !== "GET") {
      await route.fallback();
      return;
    }
    // The live row is read once before the stub is installed: a reload
    // cancels whatever read is in flight, so the answer must not depend on
    // a request the stub itself starts.
    const body = structuredClone(base);
    // One stable run id across refetches: the banner's actions must all
    // address the same run, and the scenario asserts that path.
    if (stub.runId === "") stub.runId = crypto.randomUUID();
    const runId = stub.runId;
    const runnerName = stub.runnerName;
    body["assigned_pod_id"] = stub.podId ?? body["assigned_pod_id"] ?? null;
    body["archived_at"] = stub.archived === true ? "2026-10-20" : (body["archived_at"] ?? null);
    body["agent_status"] = {
      ticker: null,
      run_count: 1,
      active_run: {
        id: runId,
        status: "queued",
        executor_kind: "local_runner",
        queue_position: null,
        runner: null,
        runner_name: null,
        pinned_runner: "5b0f1cc0-3f5a-4a43-9a1f-1f6f5b7d0a11",
        pinned_runner_name: runnerName,
        pin_kind: "dedicated",
        dispatch_wait: {
          reason: stub.reason,
          pin_kind: "dedicated",
          runner: "5b0f1cc0-3f5a-4a43-9a1f-1f6f5b7d0a11",
          runner_name: runnerName,
          queued_at: "2026-10-25T10:00:00Z",
          runner_last_seen_at: stub.runnerLastSeenAt ?? null,
          runs_ahead: stub.runsAhead ?? 0,
          releasable: stub.releasable ?? true,
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
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify(body),
    });
  });
}

/**
 * The banner's release control label. It names the issue's pod, so the
 * read waits out the generic `the pod` fallback the button carries until
 * the pod list lands.
 */
async function releaseLabel(driver: { waitingBannerActions(): Promise<string[]> }): Promise<string> {
  let label = "";
  await expect
    .poll(
      async () => {
        label = (await driver.waitingBannerActions()).find((text) => /once$/.test(text)) ?? "";
        // The generic fallback reads `... in the pod once`; anything else
        // names the pod the issue is actually on.
        return label !== "" && label !== "Run on any runner in the pod once";
      },
      { timeout: 30_000 }
    )
    .toBe(true);
  return label;
}

test(
  specTitle(["ISS-239"], "waiting banner names the runner and the reason"),
  { tag: specTags(["ISS-239"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "the waiting banner pins the web_new UI only");
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(
      seed.workspaceSlug,
      seed.projectId,
      `Waiting banner ${Date.now()}`,
      session
    );
    const detailPath = `/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/issues/${issue.id}/`;
    const stub: WaitingStub = {
      reason: "runner_offline",
      runnerName: "parity-runner-offline",
      runnerLastSeenAt: new Date(Date.now() - 2 * 60 * 60 * 1000).toISOString(),
      runId: "",
    };
    const base = await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
    try {
      await stubWaitingDetail(driver.page, detailPath, base, stub);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);

      await test.step("an offline runner names itself and its last heartbeat", async () => {
        await expect.poll(() => driver.waitingBannerPresent(), { timeout: 60_000 }).toBe(true);
        const waiting = await driver.waitingBannerText();
        expect(waiting).toBe("Waiting for runner parity-runner-offline — offline, last seen 2 h ago.");
      });

      await test.step("a busy runner names the runs ahead", async () => {
        stub.reason = "runner_busy";
        stub.runsAhead = 2;
        await driver.page.reload();
        await expect
          .poll(() => driver.waitingBannerText(), { timeout: 60_000 })
          .toBe("Waiting for runner parity-runner-offline — busy, 2 runs ahead.");
      });

      await test.step("a refused runner reads plainly", async () => {
        stub.reason = "runner_not_permitted";
        stub.runsAhead = 0;
        await driver.page.reload();
        await expect
          .poll(() => driver.waitingBannerText(), { timeout: 60_000 })
          .toBe("Waiting for runner parity-runner-offline — the runner can't take this issue.");
      });

      await test.step("a dispatching run reads as dispatching", async () => {
        stub.reason = "dispatching";
        await driver.page.reload();
        await expect
          .poll(() => driver.waitingBannerText(), { timeout: 60_000 })
          .toBe("Waiting for runner parity-runner-offline — dispatching.");
      });

      await test.step("read-only viewers read the banner with inert actions", async () => {
        stub.archived = true;
        await driver.page.reload();
        await expect.poll(() => driver.waitingBannerPresent(), { timeout: 60_000 }).toBe(true);
        // The sidebar names its pod before the banner claims read-only, so a
        // still-loading panel cannot pass as inert.
        await expect.poll(() => driver.propertyValueText("Runs on"), { timeout: 30_000 }).toContain("PAR_pod_1");
        const actions = await driver.waitingBannerActions();
        expect(actions).toHaveLength(3);
        for (const label of actions) {
          expect(await driver.waitingBannerActionDisabled(label), label).toBe(true);
        }
        expect(await driver.propertyPickerDisabled("State")).toBe(true);
      });

      await test.step("a run that is not waiting shows no banner", async () => {
        await driver.page.unrouteAll({ behavior: "wait" }).catch(() => {});
        await driver.page.reload();
        await expect.poll(() => driver.waitingBannerPresent(), { timeout: 60_000 }).toBe(false);
        expect(await driver.waitingBannerText()).toBeNull();
      });
    } finally {
      await driver.page.unrouteAll({ behavior: "wait" }).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);

test(
  specTitle(["ISS-240"], "waiting-run actions: release once, change runner, cancel"),
  { tag: specTags(["ISS-240"]) },
  async ({ driver, seed }, testInfo) => {
    test.skip(testInfo.project.name !== "new", "the waiting banner pins the web_new UI only");
    const session = await signInSession(seed.email, seed.password);
    const issue = await serverCreateIssueFull(
      seed.workspaceSlug,
      seed.projectId,
      `Waiting actions ${Date.now()}`,
      session
    );
    const detailPath = `/api/workspaces/${seed.workspaceSlug}/projects/${seed.projectId}/issues/${issue.id}/`;
    const stub: WaitingStub = {
      reason: "runner_busy",
      runnerName: "parity-runner-busy",
      runsAhead: 1,
      releasable: true,
      runId: "",
    };
    const issueWrites: string[] = [];
    const base = await fetchIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
    try {
      await stubWaitingDetail(driver.page, detailPath, base, stub);
      driver.page.on("request", (request) => {
        if (request.method() === "PATCH" && request.url().endsWith(detailPath)) issueWrites.push(request.url());
      });
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, seed.projectId, issue.id);
      await expect.poll(() => driver.waitingBannerPresent(), { timeout: 60_000 }).toBe(true);

      await test.step("the banner offers release, change runner and cancel", async () => {
        const actions = await driver.waitingBannerActions();
        expect(actions).toHaveLength(3);
        expect(actions[0]).toMatch(/^Run on any runner in .* once$/);
        expect(actions).toContain("Change runner");
        expect(actions).toContain("Cancel run");
      });

      await test.step("an unreleasable run disables release alone", async () => {
        stub.releasable = false;
        await driver.page.reload();
        await expect.poll(() => driver.waitingBannerPresent(), { timeout: 60_000 }).toBe(true);
        // Membership and the pod list settle after the first paint; wait for
        // the writable actions to live before reading the release gate.
        await expect.poll(() => driver.waitingBannerActionDisabled("Cancel run"), { timeout: 30_000 }).toBe(false);
        expect(await driver.waitingBannerActionDisabled("Change runner")).toBe(false);
        expect(await driver.waitingBannerActionDisabled(await releaseLabel(driver))).toBe(true);
      });

      await test.step("release runs this run once and leaves the issue untouched", async () => {
        stub.releasable = true;
        const releases: string[] = [];
        await driver.page.route("**/api/runners/runs/*/release-pin/", async (route) => {
          releases.push(new URL(route.request().url()).pathname);
          await route.fulfill({
            status: 200,
            contentType: "application/json",
            body: JSON.stringify({ id: stub.runId, status: "queued", executor_kind: "local_runner" }),
          });
        });
        await driver.page.reload();
        await expect.poll(() => driver.waitingBannerPresent(), { timeout: 60_000 }).toBe(true);
        const runId = stub.runId;
        await driver.waitingBannerAction(await releaseLabel(driver));
        await expect.poll(() => releases.length, { timeout: 30_000 }).toBe(1);
        expect(releases[0]).toBe(`/api/runners/runs/${runId}/release-pin/`);
        // The release rides the run, never the issue's execution target.
        expect(issueWrites).toEqual([]);
      });

      await test.step("change runner opens the execution-target picker", async () => {
        await driver.waitingBannerAction("Change runner");
        await expect.poll(() => driver.pickerOpen(), { timeout: 30_000 }).toBe(true);
        const options = await driver.pickerOptionTexts();
        expect(options.some((text) => text.includes("Any runner in"))).toBe(true);
        expect(options.some((text) => text.includes("Pi Dash Cloud Agent"))).toBe(true);
        await driver.pickerPressEscape();
        await driver.pickerClickOutside();
      });

      await test.step("cancel stops the run", async () => {
        const cancels: string[] = [];
        await driver.page.route("**/api/runners/runs/*/cancel/", async (route) => {
          cancels.push(new URL(route.request().url()).pathname);
          await route.fulfill({
            status: 200,
            contentType: "application/json",
            body: JSON.stringify({ id: stub.runId, status: "cancelled", executor_kind: "local_runner" }),
          });
        });
        const runId = stub.runId;
        await driver.waitingBannerAction("Cancel run");
        await expect.poll(() => cancels.length, { timeout: 30_000 }).toBe(1);
        expect(cancels[0]).toBe(`/api/runners/runs/${runId}/cancel/`);
      });

      await test.step("failed actions toast and the run stays waiting", async () => {
        const failures: string[] = [];
        await driver.page.route("**/api/runners/runs/*/release-pin/", async (route) => {
          failures.push("release");
          await route.fulfill({
            status: 409,
            contentType: "application/json",
            body: JSON.stringify({ detail: "The run already moved on." }),
          });
        });
        await driver.page.route("**/api/runners/runs/*/cancel/", async (route) => {
          failures.push("cancel");
          await route.fulfill({
            status: 404,
            contentType: "application/json",
            body: JSON.stringify({ detail: "No such run." }),
          });
        });
        await driver.page.reload();
        await expect.poll(() => driver.waitingBannerPresent(), { timeout: 60_000 }).toBe(true);
        await driver.waitingBannerAction(await releaseLabel(driver));
        await expect.poll(() => driver.toastText(), { timeout: 30_000 }).toContain("Couldn't release the run");
        expect(await driver.waitingBannerPresent()).toBe(true);
        await driver.waitingBannerAction("Cancel run");
        await expect.poll(() => driver.toastText(), { timeout: 30_000 }).toContain("Couldn't cancel the run");
        expect(await driver.waitingBannerPresent()).toBe(true);
        expect(failures).toEqual(["release", "cancel"]);
      });
    } finally {
      await driver.page.unrouteAll({ behavior: "wait" }).catch(() => {});
      await serverCleanupIssueWithSession(seed.workspaceSlug, seed.projectId, issue.id, session);
    }
  }
);
