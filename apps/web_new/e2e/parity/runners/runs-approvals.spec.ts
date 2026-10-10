// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios: runs, approvals, run dispatch, re-tick (NEWFRONT-180).
// The seeded stack enrolls no runners, pods, runs, approvals or tickers,
// so every scenario builds what it needs: plain runs through the direct
// web path, and states no web endpoint can produce (statuses, events,
// approvals, diagnostics, tool plans, ticker exhaustion, scheduler
// bindings) as scenario-owned ORM rows through the runsShell helpers —
// created per attempt with unique markers and deleted afterward, so a
// retry re-proves instead of colliding. Green on apps/web first.
// Rows: RUN-016, RUN-017, RUN-018, RUN-019, RUN-020, RUN-021, RUN-022,
// RUN-023, RUN-024, RUN-046, RUN-048.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  browserCookies,
  createIssue,
  createPod,
  createProjectViaApi,
  createState,
  deleteIssue,
  deletePod,
  deleteProjectViaApi,
  deleteState,
  patchIssue,
  projectFacts,
  projectStates,
  recentRuns,
  runsApprovalStatus,
  runsApprovalsList,
  runsCancel,
  runsCreateDirect,
  runsCreateFixtures,
  runsCreateRunner,
  runsCreateScheduler,
  runsDeleteRunner,
  runsDeleteRuns,
  runsDeleteScheduler,
  runsExhaustTicker,
  runsGet,
  runsIssueExecutor,
  runsListPage,
  runsLookupIds,
  runsProjectDefaultExecutor,
  runsReTick,
  runsSetStatus,
  runsStoredStatus,
  runsTickerBudget,
  signInAuthedSession,
  signInFreshUser,
  signInSession,
  uniqueSuffixForProjects,
  type AuthedSession,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

/**
 * Enter the app pre-authenticated via the credential endpoint plus cookie
 * injection (the layouts-area pattern), instead of paying the UI sign-in
 * cost per test. The login flow itself belongs to the AUTH rows; these
 * scenarios only need an authenticated browser.
 */
async function signIn(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const user = await signInFreshUser(seed.email, seed.password);
  await driver.openAuthenticated(`/${seed.workspaceSlug}/`, browserCookies(user));
  await expect.poll(() => driver.signedIn(), { timeout: 60_000 }).toBe(true);
}

/** A scenario-owned issue: `IDENT-seq`, id, name. */
interface OwnedIssue {
  seq: string;
  id: string;
  name: string;
}

/** Create a scenario-owned issue (callers delete it in `finally`). */
async function ownIssue(
  seed: ParitySeedFacts,
  session: string,
  name: string,
  projectId: string = seed.projectId
): Promise<OwnedIssue> {
  const { identifier } = await projectFacts(seed.workspaceSlug, projectId, session);
  const created = await createIssue(seed.workspaceSlug, projectId, session, name);
  return { seq: `${identifier}-${created.sequence_id}`, id: created.id, name };
}

/** Delete a scenario-owned issue. */
async function dropIssue(
  seed: ParitySeedFacts,
  session: string,
  id: string,
  projectId: string = seed.projectId
): Promise<void> {
  await deleteIssue(seed.workspaceSlug, projectId, id, session);
}

/** Create a scenario-owned pod (callers delete it after deleting its runs). */
async function ownPod(projectId: string, session: string, tag: string): Promise<{ id: string; name: string }> {
  const pod = await createPod(projectId, session, `parity-${tag}-${Date.now()}`);
  return { id: pod.id, name: pod.name };
}

/** Create a scenario-owned scratch project (callers delete it last). */
async function ownProject(authed: AuthedSession, slug: string, tag: string): Promise<string> {
  const suffix = uniqueSuffixForProjects().toUpperCase();
  return await createProjectViaApi(authed, slug, {
    name: `Parity ${tag} ${suffix}`,
    identifier: `R${suffix}`.slice(0, 10),
  });
}

/** Open an issue detail and wait for it to hydrate. */
async function openIssue(driver: ParityDriver, seed: ParitySeedFacts, issue: OwnedIssue): Promise<void> {
  await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
  await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);
  await expect.poll(() => driver.sidebarProperty("State"), { timeout: 30_000 }).not.toBeNull();
}

test(
  specTitle(["RUN-016"], "paginated runs list: deep-link, pager end-states, history-replace, page preservation"),
  { tag: specTags(["RUN-016"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const authed = await signInAuthedSession(seed.email, seed.password);
    // A scratch project keeps the page math exact: these 31 rows are the
    // only runs in this scope (30 per page, so pages 1 and 2).
    const marker = `parity-page-${Date.now()}`;
    const projectId = await ownProject(authed, seed.workspaceSlug, "runs");
    try {
      const pod = await ownPod(projectId, session, "runs");
      const runIds = await runsCreateFixtures(
        seed.workspaceSlug,
        seed.email,
        pod.id,
        Array.from({ length: 31 }, (_, index) => ({ prompt: `${marker} ${index}` }))
      );
      try {
        await test.step("?page= deep-link lands on page 2 with end-state pager", async () => {
          await driver.openRunsListAtPage(seed.workspaceSlug, 2, projectId);
          const pager = await driver.runsPager();
          expect(pager?.label).toBe("Page 2 of 2");
          expect(pager?.prevDisabled).toBe(false);
          expect(pager?.nextDisabled).toBe(true);
          const rows = await driver.runsRows();
          expect(rows).toHaveLength(1);
          const api = await runsListPage(session, { page: 2, projectId });
          expect(api.total_count).toBe(31);
          expect(api.total_pages).toBe(2);
          expect(rows[0]?.prompt).toBe(api.results[0]?.["prompt"]);
        });
        await test.step("selecting a row preserves the page query", async () => {
          await driver.runsSelectRun(marker);
          expect(await driver.runDetailState()).toBe("loaded");
          expect(await driver.currentPath()).toContain("page=2");
          const header = await driver.runDetailHeader();
          const api = await runsListPage(session, { page: 2, projectId });
          expect(header?.id).toBe(api.results[0]?.["id"]);
        });
        await test.step("pager turns rewrite the URL (no history push) and drop ?page= on page 1", async () => {
          await driver.openRunsListAtPage(seed.workspaceSlug, 2, projectId);
          const depth = await driver.runsHistoryLength();
          await driver.runsPrevPage();
          expect(await driver.runsHistoryLength()).toBe(depth);
          expect(await driver.currentPath()).not.toContain("page=");
          const first = await driver.runsPager();
          expect(first?.label).toBe("Page 1 of 2");
          expect(first?.prevDisabled).toBe(true);
          expect(first?.nextDisabled).toBe(false);
          await driver.runsNextPage();
          expect(await driver.runsHistoryLength()).toBe(depth);
          expect(await driver.currentPath()).toContain("page=2");
        });
        await test.step("newest-first order matches the API", async () => {
          await driver.openRunsList(seed.workspaceSlug, projectId);
          const rows = await driver.runsRows();
          expect(rows).toHaveLength(30);
          const api = await runsListPage(session, { page: 1, projectId });
          expect(rows.map((row) => row.prompt)).toEqual(api.results.map((row) => row["prompt"]));
        });
      } finally {
        await runsDeleteRuns(runIds);
        await deletePod(pod.id, session);
      }
    } finally {
      await deleteProjectViaApi(authed, seed.workspaceSlug, projectId);
    }
  }
);

test(
  specTitle(["RUN-016"], "runs list empty state and no cross-scope row bleed"),
  { tag: specTags(["RUN-016"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const authed = await signInAuthedSession(seed.email, seed.password);
    const marker = `parity-scope-${Date.now()}`;
    const projectA = await ownProject(authed, seed.workspaceSlug, "scopea");
    const projectB = await ownProject(authed, seed.workspaceSlug, "scopeb");
    try {
      const podA = await ownPod(projectA, session, "scopea");
      const runIds = await runsCreateFixtures(seed.workspaceSlug, seed.email, podA.id, [
        { prompt: `${marker} one` },
        { prompt: `${marker} two` },
      ]);
      try {
        await test.step("empty scope shows the empty state with no pager", async () => {
          await driver.openRunsList(seed.workspaceSlug, projectB);
          expect(await driver.runsEmptyVisible()).toBe(true);
          expect(await driver.runsPager()).toBeNull();
          expect(await driver.runsRows()).toHaveLength(0);
          const api = await runsListPage(session, { projectId: projectB });
          expect(api.total_count).toBe(0);
        });
        await test.step("rows show in their own scope and the aggregate, never in a sibling scope", async () => {
          await driver.openRunsList(seed.workspaceSlug, projectA);
          const scoped = await driver.runsRows();
          expect(scoped.filter((row) => row.prompt.includes(marker))).toHaveLength(2);
          await driver.openRunsList(seed.workspaceSlug);
          const aggregate = await driver.runsRows();
          expect(aggregate.some((row) => row.prompt.includes(marker))).toBe(true);
          await driver.openRunsList(seed.workspaceSlug, projectB);
          const sibling = await driver.runsRows();
          expect(sibling.some((row) => row.prompt.includes(marker))).toBe(false);
        });
      } finally {
        await runsDeleteRuns(runIds);
        await deletePod(podA.id, session);
      }
    } finally {
      await deleteProjectViaApi(authed, seed.workspaceSlug, projectA);
      await deleteProjectViaApi(authed, seed.workspaceSlug, projectB);
    }
  }
);

test(
  specTitle(["RUN-017", "RUN-018"], "run detail fields, scheduler link, events, and poll-until-terminal"),
  { tag: specTags(["RUN-017", "RUN-018"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const marker = `parity-detail-${Date.now()}`;
    const pod = await ownPod(seed.projectId, session, "detail");
    const scheduler = await runsCreateScheduler(
      seed.workspaceSlug,
      seed.projectId,
      seed.email,
      `parity-sched-${Date.now()}`,
      `Parity sched ${Date.now()}`
    );
    const narrativeOne = `${marker} narrative one`;
    const narrativeTwo = `${marker} narrative two`;
    const runIds = await runsCreateFixtures(seed.workspaceSlug, seed.email, pod.id, [
      {
        prompt: `${marker} full prompt text`,
        status: "running",
        scheduler_binding: scheduler.bindingId,
        events: [
          { kind: "run/started", payload: {} },
          { kind: "agent/message", payload: { text: narrativeOne } },
          { kind: "agent/message", payload: { text: narrativeTwo } },
        ],
      },
      { prompt: `${marker} terminal`, status: "completed" },
    ]);
    const liveId = runIds[0] as string;
    const terminalId = runIds[1] as string;
    try {
      await test.step("detail shows id, status, executor, full prompt, scheduler link", async () => {
        await driver.openRunDetail(seed.workspaceSlug, liveId);
        const header = await driver.runDetailHeader();
        expect(header?.id).toBe(liveId);
        expect(header?.statusLabel).toBe("running");
        expect(header?.executor).toBe("Local Runner");
        expect(await driver.runDetailPrompt()).toBe(`${marker} full prompt text`);
        const link = await driver.runDetailScheduler();
        expect(link?.name).toBe(scheduler.schedulerName);
        expect(link?.href).toContain(scheduler.bindingId);
        const stored = await runsGet(liveId, session, true);
        expect(stored["status"]).toBe("running");
        expect(stored["executor_kind"]).toBe("local_runner");
      });
      await test.step("Events render sequenced with narratives inline and metadata rows bare", async () => {
        const events = await driver.runEvents();
        expect(events.map((event) => event.kind)).toEqual(["run/started", "agent/message", "agent/message"]);
        expect(events.map((event) => event.seq)).toEqual(["1", "2", "3"]);
        expect(events[0]?.narrative).toBeNull();
        expect(events[1]?.narrative).toBe(narrativeOne);
        expect(events[2]?.narrative).toBe(narrativeTwo);
      });
      await test.step("Detail re-polls while non-terminal and stops at terminal", async () => {
        await driver.openRunDetail(seed.workspaceSlug, liveId);
        expect(await driver.runDetailPollCount(liveId, 9_000)).toBeGreaterThanOrEqual(2);
        expect(await runsSetStatus(liveId, "completed")).toBe("completed");
        // Let the in-flight poll learn the terminal status, then prove silence.
        await new Promise((resolve) => setTimeout(resolve, 7_000));
        expect(await driver.runDetailPollCount(liveId, 8_000)).toBe(0);
        expect((await runsGet(liveId, session))["status"]).toBe("completed");
        await driver.openRunDetail(seed.workspaceSlug, terminalId);
        expect(await driver.runDetailPollCount(terminalId, 8_000)).toBe(0);
      });
      await test.step("Runs without a binding show no scheduler link", async () => {
        await driver.openRunDetail(seed.workspaceSlug, terminalId);
        expect(await driver.runDetailScheduler()).toBeNull();
      });
    } finally {
      await runsDeleteRuns(runIds);
      await runsDeleteScheduler(scheduler.schedulerId);
      await deletePod(pod.id, session);
    }
  }
);

test(
  specTitle(["RUN-017"], "run detail pane states: none selected, loading, unavailable"),
  { tag: specTags(["RUN-017"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const marker = `parity-states-${Date.now()}`;
    const pod = await ownPod(seed.projectId, session, "states");
    const runIds = await runsCreateFixtures(seed.workspaceSlug, seed.email, pod.id, [{ prompt: marker }]);
    try {
      await driver.openRunsList(seed.workspaceSlug);
      expect(await driver.runDetailState()).toBe("none");
      expect(await driver.runDetailLoadingObserved(seed.workspaceSlug, runIds[0] as string)).toBe(true);
      await driver.openRunDetail(seed.workspaceSlug, "00000000-0000-0000-0000-000000000000");
      expect(await driver.runDetailState()).toBe("unavailable");
    } finally {
      await runsDeleteRuns(runIds);
      await deletePod(pod.id, session);
    }
  }
);

test(
  specTitle(["RUN-019"], "run failure diagnostics and result payload"),
  { tag: specTags(["RUN-019"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const marker = `parity-diag-${Date.now()}`;
    const pod = await ownPod(seed.projectId, session, "diag");
    const authError = `${marker} first line: authentication_failed for agent CLI\nsecond line`;
    const cloudSummary = `${marker} cloud summary`;
    const runIds = await runsCreateFixtures(seed.workspaceSlug, seed.email, pod.id, [
      { prompt: `${marker} auth failure`, status: "failed", error: authError },
      { prompt: `${marker} plain failure`, status: "failed", error: `${marker} plain boom` },
      {
        prompt: `${marker} cloud done`,
        status: "completed",
        executor: "cloud_agent",
        done_payload: { summary: cloudSummary, answer: 42 },
      },
      {
        prompt: `${marker} local done`,
        status: "completed",
        done_payload: { answer: 7 },
      },
    ]);
    try {
      await test.step("Classified error shows source, kind, summary, action plus raw text", async () => {
        await driver.openRunDetail(seed.workspaceSlug, runIds[0] as string);
        const error = await driver.runDetailError();
        expect(error?.raw).toBe(authError);
        expect(error?.source).toBe("Agent CLI");
        expect(error?.kind).toBe("Authentication");
        expect(error?.summary).toBe(`${marker} first line: authentication_failed for agent CLI`);
        expect(error?.action).toMatch(/Re-authenticate/);
        const stored = await runsGet(runIds[0] as string, session);
        const diagnostic = stored["error_diagnostic"] as { kind?: unknown } | null;
        expect(diagnostic?.kind).toBe("agent_authentication");
      });
      await test.step("Unclassified error falls back to Unknown with no action", async () => {
        await driver.openRunDetail(seed.workspaceSlug, runIds[1] as string);
        const error = await driver.runDetailError();
        expect(error?.raw).toBe(`${marker} plain boom`);
        expect(error?.source).toBe("Unknown");
        expect(error?.kind).toBe("Unknown");
        expect(error?.summary).toBe(`${marker} plain boom`);
        expect(error?.action).toBeNull();
      });
      await test.step("Cloud result surfaces its summary above the raw payload; local shows raw only", async () => {
        await driver.openRunDetail(seed.workspaceSlug, runIds[2] as string);
        const cloud = await driver.runDetailResult();
        expect(cloud?.summary).toBe(cloudSummary);
        expect(JSON.parse(cloud?.raw ?? "{}")["answer"]).toBe(42);
        await driver.openRunDetail(seed.workspaceSlug, runIds[3] as string);
        const local = await driver.runDetailResult();
        expect(local?.summary).toBeNull();
        expect(JSON.parse(local?.raw ?? "{}")["answer"]).toBe(7);
      });
    } finally {
      await runsDeleteRuns(runIds);
      await deletePod(pod.id, session);
    }
  }
);

test(
  specTitle(["RUN-020"], "cloud tool-plan panel shows for cloud runs only"),
  { tag: specTags(["RUN-020"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const marker = `parity-cloud-${Date.now()}`;
    const pod = await ownPod(seed.projectId, session, "cloud");
    const runIds = await runsCreateFixtures(seed.workspaceSlug, seed.email, pod.id, [
      {
        prompt: `${marker} cloud`,
        status: "running",
        executor: "cloud_agent",
        tool_plan: { tools: ["parity-tool-a", "parity-tool-b"] },
        tool_calls: [{ tool_name: "parity-call-1", risk: "write", status: "succeeded" }],
      },
      {
        prompt: `${marker} local`,
        status: "running",
        tool_plan: { tools: ["parity-ignored"] },
      },
    ]);
    try {
      await driver.openRunDetail(seed.workspaceSlug, runIds[0] as string);
      const panel = await driver.runCloudPanel();
      expect(panel?.tools).toEqual(["parity-tool-a", "parity-tool-b"]);
      expect(panel?.calls).toEqual([{ tool: "parity-call-1", risk: "write", status: "succeeded" }]);
      const stored = await runsGet(runIds[0] as string, session);
      expect((stored["tool_plan"] as { tools?: unknown })?.tools).toEqual(["parity-tool-a", "parity-tool-b"]);
      await driver.openRunDetail(seed.workspaceSlug, runIds[1] as string);
      expect(await driver.runCloudPanel()).toBeNull();
    } finally {
      await runsDeleteRuns(runIds);
      await deletePod(pod.id, session);
    }
  }
);

test(
  specTitle(["RUN-021"], "cancel an in-flight run from the detail header"),
  { tag: specTags(["RUN-021"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const marker = `parity-cancel-${Date.now()}`;
    const pod = await ownPod(seed.projectId, session, "cancel");
    const { workspaceId } = await runsLookupIds(seed.workspaceSlug, seed.email);
    const created = await runsCreateDirect(session, {
      workspace: workspaceId,
      prompt: `${marker} queued`,
      pod: pod.id,
    });
    const createdId = created["id"] as string;
    const terminalIds = await runsCreateFixtures(seed.workspaceSlug, seed.email, pod.id, [
      { prompt: `${marker} completed`, status: "completed" },
      { prompt: `${marker} cancel requested`, status: "cancel_requested" },
    ]);
    try {
      await test.step("Cancel control confirms and the run lands cancelled", async () => {
        expect(created["status"]).toBe("queued");
        await driver.openRunDetail(seed.workspaceSlug, createdId);
        expect(await driver.runCancelAvailable()).toBe(true);
        await driver.runCancelOpen();
        const dialog = await driver.runCancelDialog();
        expect(dialog?.title).toBe("Cancel run?");
        expect(dialog?.body).toMatch(/stop/i);
        await driver.runCancelConfirm();
        await expect
          .poll(async () => (await driver.runDetailHeader())?.statusLabel, { timeout: 30_000 })
          .toBe("cancelled");
        expect(await driver.runCancelAvailable()).toBe(false);
        expect(await runsStoredStatus(createdId)).toBe("cancelled");
        const page = await runsListPage(session, {});
        expect(page.results.find((row) => row["id"] === createdId)?.["status"]).toBe("cancelled");
      });
      await test.step("Terminal and cancel-requested runs offer no cancel; the server rejects terminal cancels", async () => {
        await driver.openRunDetail(seed.workspaceSlug, terminalIds[0] as string);
        expect(await driver.runCancelAvailable()).toBe(false);
        await driver.openRunDetail(seed.workspaceSlug, terminalIds[1] as string);
        expect(await driver.runCancelAvailable()).toBe(false);
        await expect(runsCancel(terminalIds[0] as string, session)).rejects.toThrow(/409/);
        expect(await runsStoredStatus(terminalIds[0] as string)).toBe("completed");
      });
    } finally {
      await runsDeleteRuns([createdId, ...terminalIds]);
      await deletePod(pod.id, session);
    }
  }
);

test(
  specTitle(["RUN-022"], "run status badge map plus unknown-status neutral fallback"),
  { tag: specTags(["RUN-022"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const marker = `parity-badge-${Date.now()}`;
    const pod = await ownPod(seed.projectId, session, "badge");
    // The full badge map, plus a backend status the badge component does
    // not know: it must render its raw value, not crash.
    const expected: Array<[string, string]> = [
      ["queued", "queued"],
      ["assigned", "assigned"],
      ["running", "running"],
      ["cancel_requested", "cancellation requested"],
      ["awaiting_approval", "awaiting approval"],
      ["awaiting_reauth", "awaiting reauth"],
      ["paused_awaiting_input", "paused awaiting input"],
      ["blocked", "blocked"],
      ["completed", "completed"],
      ["failed", "failed"],
      ["cancelled", "cancelled"],
      ["refused", "refused"],
      ["waiting_for_worktree", "waiting_for_worktree"],
    ];
    const runIds = await runsCreateFixtures(
      seed.workspaceSlug,
      seed.email,
      pod.id,
      expected.map(([status]) => ({ prompt: `${marker} ${status}`, status }))
    );
    try {
      await driver.openRunsList(seed.workspaceSlug);
      const rows = (await driver.runsRows()).filter((row) => row.prompt.includes(marker));
      expect(rows).toHaveLength(expected.length);
      for (const [status, label] of expected) {
        const row = rows.find((candidate) => candidate.prompt === `${marker} ${status}`);
        expect(row?.statusLabel, `badge for ${status}`).toBe(label);
      }
      const unknownId = runIds[expected.length - 1] as string;
      await driver.openRunDetail(seed.workspaceSlug, unknownId);
      expect((await driver.runDetailHeader())?.statusLabel).toBe("waiting_for_worktree");
      expect((await runsGet(unknownId, session))["status"]).toBe("waiting_for_worktree");
    } finally {
      await runsDeleteRuns(runIds);
      await deletePod(pod.id, session);
    }
  }
);

test(
  specTitle(["RUN-023"], "pending-approval queue cards, empty state, and scope"),
  { tag: specTags(["RUN-023"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const authed = await signInAuthedSession(seed.email, seed.password);
    const marker = `parity-queue-${Date.now()}`;
    const pod = await ownPod(seed.projectId, session, "queue");
    const emptyProject = await ownProject(authed, seed.workspaceSlug, "queue");
    const kinds: Array<[string, string]> = [
      ["command_execution", "The runner wants to run a shell command"],
      ["file_change", "The runner wants to modify a file"],
      ["network_access", "The runner wants to make a network call"],
      ["other", "The runner is requesting approval"],
    ];
    const runIds = await runsCreateFixtures(
      seed.workspaceSlug,
      seed.email,
      pod.id,
      kinds.map(([kind], index) => ({
        prompt: `${marker} run ${kind}`,
        status: "awaiting_approval",
        approvals: [
          {
            kind,
            payload: { marker: `${marker} ${kind}`, index },
            reason: index < 2 ? `${marker} reason ${kind}` : "",
            expires_in_seconds: index === 0 ? 3600 : null,
          },
        ],
      }))
    );
    try {
      await test.step("Cards show run, kind label, reason, expiry, and payload", async () => {
        await driver.openApprovals(seed.workspaceSlug);
        const cards = (await driver.approvalsCards()).filter((card) => card.payload.includes(marker));
        expect(cards).toHaveLength(4);
        const pending = await runsApprovalsList(session);
        for (const [kind, label] of kinds) {
          const card = cards.find((candidate) => candidate.payload.includes(`${marker} ${kind}`));
          expect(card?.kindLabel, `kind label for ${kind}`).toBe(label);
          const approval = pending.find((row) => JSON.stringify(row["payload"] ?? {}).includes(`${marker} ${kind}`));
          expect(approval, `server row for ${kind}`).toBeTruthy();
          expect(card?.header).toContain(approval?.["agent_run"] as string);
          if (kind === "command_execution" || kind === "file_change") {
            expect(card?.reason).toBe(`${marker} reason ${kind}`);
          } else {
            expect(card?.reason).toBeNull();
          }
          if (kind === "command_execution") {
            expect(card?.expiry).toMatch(/expires /);
          } else {
            expect(card?.expiry).toBeNull();
          }
        }
      });
      await test.step("Empty and foreign scopes show no cards", async () => {
        await driver.openApprovals(seed.workspaceSlug, emptyProject);
        expect(await driver.approvalsEmptyVisible()).toBe(true);
        expect(await driver.approvalsCards()).toHaveLength(0);
        expect(await runsApprovalsList(session, emptyProject)).toHaveLength(0);
      });
    } finally {
      await runsDeleteRuns(runIds);
      await deletePod(pod.id, session);
      await deleteProjectViaApi(authed, seed.workspaceSlug, emptyProject);
    }
  }
);

test(
  specTitle(["RUN-024"], "bug: NEWFRONT-297 accept-once and decline both 500, so the cards stay pending"),
  { tag: specTags(["RUN-024"]) },
  async ({ driver, seed }) => {
    // Every decide POST 500s (the endpoint's select_for_update spans the
    // nullable runner join, which PostgreSQL rejects), so both decisions
    // toast "Failed to record decision", the cards stay, and the approvals
    // stay pending. Intended: the card drops off, the decision persists with
    // source web and the decider, and the run resumes to running.
    // Follow-up: NEWFRONT-297.
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const marker = `parity-decide-${Date.now()}`;
    const pod = await ownPod(seed.projectId, session, "decide");
    const runIds = await runsCreateFixtures(seed.workspaceSlug, seed.email, pod.id, [
      {
        prompt: `${marker} accept`,
        status: "awaiting_approval",
        approvals: [{ kind: "command_execution", payload: { marker: `${marker} accept` } }],
      },
      {
        prompt: `${marker} decline`,
        status: "awaiting_approval",
        approvals: [{ kind: "file_change", payload: { marker: `${marker} decline` } }],
      },
    ]);
    // The refusal toast is transient while the decide helper waits out the
    // full detach window for the staying card, so each decision watches for
    // its toast across the click instead of polling after it.
    const decideToast = async (text: string, decision: "accept" | "decline"): Promise<string | null> => {
      let toast: string | null = null;
      const watch = (async () => {
        const deadline = Date.now() + 30_000;
        for (;;) {
          const current = await driver.lastToast();
          if (current !== null) {
            toast = current;
            return;
          }
          if (Date.now() > deadline) return;
          await new Promise((resolve) => setTimeout(resolve, 500));
        }
      })();
      await driver.approvalsDecide(text, decision);
      await watch;
      return toast;
    };
    try {
      const pending = await runsApprovalsList(session);
      const approvalFor = (text: string): string => {
        const row = pending.find((candidate) => JSON.stringify(candidate["payload"] ?? {}).includes(text));
        if (typeof row?.["id"] !== "string") throw new Error(`[parity] no pending approval for ${text}.`);
        return row["id"];
      };
      const acceptId = approvalFor(`${marker} accept`);
      const declineId = approvalFor(`${marker} decline`);
      await driver.openApprovals(seed.workspaceSlug);
      await test.step("Accept-once 500s: toast, card stays, approval stays pending", async () => {
        expect(await decideToast(`${marker} accept`, "accept")).toMatch(/failed to record decision/i);
        const cards = await driver.approvalsCards();
        expect(cards.some((card) => card.payload.includes(`${marker} accept`))).toBe(true);
        expect((await runsApprovalStatus(acceptId)).status).toBe("pending");
        expect(await runsStoredStatus(runIds[0] as string)).toBe("awaiting_approval");
      });
      await test.step("Decline 500s: toast, card stays, approval stays pending", async () => {
        expect(await decideToast(`${marker} decline`, "decline")).toMatch(/failed to record decision/i);
        const cards = await driver.approvalsCards();
        expect(cards.some((card) => card.payload.includes(`${marker} decline`))).toBe(true);
        expect((await runsApprovalStatus(declineId)).status).toBe("pending");
      });
    } finally {
      await runsDeleteRuns(runIds);
      await deletePod(pod.id, session);
    }
  }
);

test(
  specTitle(["RUN-024"], "bug: NEWFRONT-189 accept-for-session is rejected by the decide endpoint"),
  { tag: specTags(["RUN-024"]) },
  async ({ driver, seed }) => {
    // The card offers Accept for session and the client sends it, but the
    // decide endpoint only accepts accept/decline, so the decision 400s and
    // the card stays with an error toast. Intended: a session-scoped accept
    // persists and the card drops off like the other decisions.
    // Follow-up: NEWFRONT-189.
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const marker = `parity-session-${Date.now()}`;
    const pod = await ownPod(seed.projectId, session, "session");
    const runIds = await runsCreateFixtures(seed.workspaceSlug, seed.email, pod.id, [
      {
        prompt: `${marker} run`,
        status: "awaiting_approval",
        approvals: [{ kind: "network_access", payload: { marker } }],
      },
    ]);
    try {
      const pending = await runsApprovalsList(session);
      const row = pending.find((candidate) => JSON.stringify(candidate["payload"] ?? {}).includes(marker));
      if (typeof row?.["id"] !== "string") throw new Error("[parity] no pending approval for the marker.");
      await driver.openApprovals(seed.workspaceSlug);
      // The refusal toast is transient (auto-dismisses in seconds) while the
      // decide helper waits out the full detach window for the staying card,
      // so watch for the toast across the click instead of polling after it.
      let refusalToast: string | null = null;
      const watch = (async () => {
        const deadline = Date.now() + 30_000;
        for (;;) {
          const toast = await driver.lastToast();
          if (toast !== null) {
            refusalToast = toast;
            return;
          }
          if (Date.now() > deadline) return;
          await new Promise((resolve) => setTimeout(resolve, 500));
        }
      })();
      await driver.approvalsDecide(marker, "accept_for_session");
      await watch;
      const cards = await driver.approvalsCards();
      expect(cards.some((card) => card.payload.includes(marker))).toBe(true);
      expect(refusalToast).toMatch(/failed to record decision/i);
      expect((await runsApprovalStatus(row["id"])).status).toBe("pending");
    } finally {
      await runsDeleteRuns(runIds);
      await deletePod(pod.id, session);
    }
  }
);

test(
  specTitle(["RUN-046"], "Run-AI dispatch leaves the executor selection untouched and prepares nothing locally"),
  { tag: specTags(["RUN-046"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle dispatch ${Date.now()}`);
    try {
      // A fresh issue inherits the project default (no pin); pin it so the
      // read-only assertion has a value that must survive the dispatch.
      expect(await runsIssueExecutor(seed.workspaceSlug, seed.projectId, issue.id, session)).toBeNull();
      await patchIssue(seed.workspaceSlug, seed.projectId, issue.id, session, { agent_executor: "local_runner" });
      const pinBefore = await runsIssueExecutor(seed.workspaceSlug, seed.projectId, issue.id, session);
      expect(pinBefore).toBe("local_runner");
      const defaultBefore = await runsProjectDefaultExecutor(seed.workspaceSlug, seed.projectId, session);
      await openIssue(driver, seed, issue);
      // No runners are enrolled, so dispatch deterministically reports
      // failure (the graceful-failure path); the executor assertions below
      // are what this row owns.
      const dispatch = await driver.issueRunAiDispatch();
      expect(dispatch.toast).toMatch(/failed to start agent run/i);
      expect(await runsIssueExecutor(seed.workspaceSlug, seed.projectId, issue.id, session)).toBe(pinBefore);
      expect(await runsProjectDefaultExecutor(seed.workspaceSlug, seed.projectId, session)).toBe(defaultBefore);
      // On a plain browser the desktop-managed preparation path makes no
      // calls: no local agent, enrollment, or machine endpoints are hit.
      const prepHits = dispatch.requestUrls.filter((url) =>
        /agent-profile|desktop-enroll|managed_|dev-machines|desktop/i.test(url)
      );
      expect(prepHits).toEqual([]);
      expect(dispatch.requestUrls.some((url) => url.includes("/api/runners/runs/"))).toBe(true);
      expect((await recentRuns(session)).filter((row) => row["work_item"] === issue.id)).toHaveLength(0);
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);

test(
  specTitle(["RUN-048"], "re-tick grants a fresh budget and starts a run"),
  { tag: specTags(["RUN-048"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle retick ${Date.now()}`);
    // The seed project carries only the Todo state, so the ticking bucket is
    // scenario-owned — and it must be the registered "In Progress" name, not
    // just the started group, for the scheduler to treat it as ticking.
    // Reuse a leaked one if a previous attempt never cleaned up.
    const bucket = (await projectStates(seed.workspaceSlug, seed.projectId, session)).find(
      (state) => state.group === "started" && state.name === "In Progress"
    );
    let ticking = bucket ?? null;
    let ownBucket = false;
    if (ticking === null) {
      ticking = await createState(seed.workspaceSlug, seed.projectId, session, "In Progress", "started");
      ownBucket = true;
    }
    // A grant must dispatch a run, and dispatch preflight needs a registered
    // runner in the issue's pod — an offline enrollment in the default pod
    // is honest capacity (the run waits visibly in queued).
    const runnerId = await runsCreateRunner(
      seed.workspaceSlug,
      seed.email,
      seed.projectId,
      `parity-retick-${Date.now()}`
    );
    try {
      await patchIssue(seed.workspaceSlug, seed.projectId, issue.id, session, { state: ticking.id });
      // Ticker rows are created lazily; exhausting first both creates and
      // spends it, so the pre-grant read below always has a row.
      await runsExhaustTicker(issue.id);
      const before = await runsTickerBudget(issue.id);
      await openIssue(driver, seed, issue);
      expect(await driver.issueReTickVisible()).toBe(true);
      const toast = await driver.issueReTickClick();
      expect(toast).toMatch(/budget/i);
      const after = await runsTickerBudget(issue.id);
      expect(after.granted).toBeGreaterThan(before.granted);
      expect(after.cap).toBeGreaterThan(before.cap);
      // The grant fires a run now (or the issue already runs one from
      // entering the bucket); either way a run is attached to the issue.
      expect((await recentRuns(session)).filter((row) => row["work_item"] === issue.id).length).toBeGreaterThanOrEqual(
        1
      );
    } finally {
      const leftovers = (await recentRuns(session))
        .filter((row) => row["work_item"] === issue.id)
        .map((row) => row["id"] as string);
      await runsDeleteRuns(leftovers);
      await dropIssue(seed, session, issue.id);
      if (ownBucket) await deleteState(seed.workspaceSlug, seed.projectId, ticking.id, session);
      await runsDeleteRunner(runnerId);
    }
  }
);

test(
  specTitle(["RUN-048"], "re-tick refuses with a machine-readable reason when nothing is owed"),
  { tag: specTags(["RUN-048"]) },
  async ({ driver, seed }) => {
    await signIn(driver, seed);
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssue(seed, session, `Oracle retick refuse ${Date.now()}`);
    try {
      // A fresh issue is neither ticking nor exhausted, so no control shows.
      await openIssue(driver, seed, issue);
      expect(await driver.issueReTickVisible()).toBe(false);
      // The API answers granted:false with a machine-readable reason rather
      // than an error — the caller surfaces it instead of failing.
      const result = await runsReTick(issue.id, session);
      expect(result["granted"]).toBe(false);
      expect(typeof result["reason"]).toBe("string");
      expect(["no_ticker", "not_ticking_state", "budget_not_exhausted"]).toContain(result["reason"]);
    } finally {
      await dropIssue(seed, session, issue.id);
    }
  }
);
