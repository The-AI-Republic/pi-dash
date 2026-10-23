// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-207): desktop agent-runtime web-observable
// sides — automatic enrollment plus the status notice, the Run-press
// readiness gate, the missing-binary repair notice, server refusal reasons,
// the built-in chat contact, direct local chat, per-session approval modes,
// inline approval prompts, on-machine history, sign-out teardown, and live
// engine-setup refresh. Rows: DESK-001–DESK-010, DESK-026.
//
// The oracle runs on apps/web, where these capabilities are absent by
// design: every scenario pins the web-observable side (no enrollment
// traffic, no notice, an inert gate, reason codes over web-reachable APIs,
// the cloud chat path) and the server state a browser session observes.
// The RUN-033/034/035/036/044/045/047 precedent
// (runners/desktop-runtime.spec.ts) already covers the shared absence
// halves; these scenarios assert the DESK-specific angles — the notice
// lifecycle across project open and leave, direct navigation to the
// built-in chat id, the warm/cancel/close REST verbs, queue-side approval
// decisions, focus-triggered refresh quietness, and history kept across
// sign-out — rather than copying it.
//
// Fixtures: runners are planted through the Django shell (the web API
// offers no create-runner endpoint); online runners get a live session row
// so the outbox treats them as connected. Chat sessions, messages and the
// queue reads go through the app's own REST endpoints; pending chat
// approvals are planted through the shell, standing in for the daemon
// write-back, and multi-turn transcripts settle each turn through the same
// service call the daemon's completion upstream uses (turn bookkeeping
// only — no transcript content is invented). Scenario-owned issues go
// through the app's own issue endpoints. The scratch stack runs no live
// runner daemon.
import { test, expect } from "../../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../../drivers/parity-driver";
import {
  signInSession,
  projectFacts,
  createIssue,
  deleteIssue,
  recentRuns,
  runsDeleteRuns,
  serverProjectIdentifier,
  serverSetTourCompleted,
  sessionBrowserCookies,
  sessionValid,
  serverDesktopRuntimeWorkspaceId,
  serverDesktopRuntimePlantRunner,
  serverDesktopRuntimeCleanupRunner,
  serverDesktopRuntimeCreateChatSession,
  serverDesktopRuntimeListChatSessions,
  serverDesktopRuntimeChatMessages,
  serverDesktopRuntimeSendChatMessage,
  serverDesktopRuntimePlantChatApproval,
  serverDesktopRuntimeListChatApprovals,
  serverDesktopRuntimeMarkSessionDesktop,
  serverDeskRuntimeProjectExecutorOptions,
  serverDeskRuntimePinIssueExecutorStatus,
  serverDeskRuntimeIssueExecutor,
  serverDeskRuntimeWarmChatSession,
  serverDeskRuntimeCancelChatSession,
  serverDeskRuntimeCloseChatSession,
  serverDeskRuntimeChatSessionDetail,
  serverDeskRuntimeSettleChatTurn,
  serverDeskRuntimeDecideChatApproval,
  type DesktopRuntimeRunner,
  type DesktopRuntimeRunnerStatus,
} from "../../helpers/api";
import { specTags, specTitle } from "../../helpers/tags";
import { desktopChatSessions, desktopEmit, desktopInvocations, installDesktopMock } from "../tauri-mock";

/** Synthetic chat id the desktop build serves over local IPC. */
const BUILTIN_CHAT_ID = "pidash-builtin";

let deskSerial = 0;

function deskTag(prefix: string): string {
  deskSerial += 1;
  return `dk207 ${prefix} ${Date.now()} ${deskSerial}`;
}

async function plantRunner(
  seed: ParitySeedFacts,
  prefix: string,
  status: DesktopRuntimeRunnerStatus
): Promise<DesktopRuntimeRunner> {
  return serverDesktopRuntimePlantRunner({
    ownerEmail: seed.email,
    workspaceSlug: seed.workspaceSlug,
    projectId: seed.projectId,
    name: deskTag(prefix),
    status,
  });
}

/** Sequence plus title of the first seed issue, resolved live. */
async function seedIssueSeq(seed: ParitySeedFacts): Promise<{ seq: string; name: string }> {
  const session = await signInSession(seed.email, seed.password);
  const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
  const name = seed.issueNames[0];
  if (!name) throw new Error("[parity] seed facts carry no issue names.");
  return { seq: `${identifier}-1`, name };
}

/**
 * Open the first seed issue's detail page signed in. A project route, so
 * the desktop enrollment effect would fire there too (it resolves the
 * project from the browse identifier) — while the shared issues-list
 * opener's boot check never converges on this checkout (its strict
 * landmark read always misses) and burns its full deadline per call.
 */
async function openSignedInIssue(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const issue = await seedIssueSeq(seed);
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
  await expect.poll(() => driver.issueDetailTitle(), { timeout: 60_000 }).toBe(issue.name);
}

async function openSignedInChat(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  runnerId: string,
  sessionId?: string
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.desktopRuntimeOpenChat(seed.workspaceSlug, runnerId, sessionId);
}

/** Request URLs that would carry enrollment or engine-setup traffic. */
function enrollmentTraffic(urls: string[]): string[] {
  return urls.filter((url) => url.includes("/ai-assistant/") || url.includes("/desktop-enroll/"));
}

async function ownIssueId(
  seed: ParitySeedFacts,
  session: string,
  name: string
): Promise<{ id: string; seq: string; name: string }> {
  const { identifier } = await projectFacts(seed.workspaceSlug, seed.projectId, session);
  const created = await createIssue(seed.workspaceSlug, seed.projectId, session, name);
  return { id: created.id, seq: `${identifier}-${created.sequence_id}`, name };
}

test(
  specTitle(["DESK-001"], "project open enrolls nothing and never shows a status notice"),
  { tag: specTags(["DESK-001"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk001Positive(driver, seed);
      return;
    }
    const issue = await seedIssueSeq(seed);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.desktopRuntimeStartRequestSpy();
    try {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 60_000 }).toBe(issue.name);

      await test.step("no notice appears while the desktop would poll", async () => {
        expect(await driver.desktopRuntimeRuntimeBannerVisible()).toBe(false);
        // The desktop re-reads enrollment on a short cadence after opening
        // a project; sit through one window and confirm nothing mounts.
        await driver.page.waitForTimeout(6_000);
        expect(await driver.desktopRuntimeRuntimeBannerVisible()).toBe(false);
      });

      await test.step("leaving the project stays quiet", async () => {
        await driver.desktopRuntimeOpenRunners(seed.workspaceSlug);
        expect(await driver.desktopRuntimeRuntimeBannerVisible()).toBe(false);
        expect(enrollmentTraffic(await driver.desktopRuntimeSpyUrls())).toEqual([]);
      });
    } finally {
      await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
    }

    await test.step("no enrollment cache is kept client-side", async () => {
      const keys = await driver.desktopRuntimeStorageKeys();
      expect(keys.session.filter((key) => key.startsWith("pidash-managed-workspaces:"))).toEqual([]);
      expect(keys.local.filter((key) => key.startsWith("pidash-managed-workspaces:"))).toEqual([]);
    });
  }
);

test(
  specTitle(["DESK-002"], "Run press leaves the local agent alone and the selection untouched"),
  { tag: specTags(["DESK-002"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk002Positive(driver, seed);
      return;
    }
    const session = await signInSession(seed.email, seed.password);
    const issue = await ownIssueId(seed, session, `Oracle desk run ${Date.now()}`);
    try {
      const before = await serverDeskRuntimeIssueExecutor(seed.workspaceSlug, seed.projectId, issue.id, session);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.issueDetailTitle(), { timeout: 120_000 }).toBe(issue.name);

      await driver.desktopRuntimeStartRequestSpy();
      try {
        await test.step("the press dispatches through the cloud path with no local traffic", async () => {
          // The seed enrolls runners, so dispatch may start a run or fail
          // gracefully depending on capacity — either way the gate half
          // pinned here is that the press goes through the cloud path and
          // never touches the local-enrollment path. The failure toast
          // itself is pinned by ISS-192.
          const dispatch = await driver.issueRunAiDispatch();
          const attached = (await recentRuns(session)).filter((row) => row["work_item"] === issue.id);
          const started = driver.target === "web_new" ? /run started/i : /agent run started/i;
          const failed = driver.target === "web_new" ? /couldn't start the run/i : /failed to start agent run/i;
          if (dispatch.toast !== null && started.test(dispatch.toast)) {
            expect(attached).toHaveLength(1);
          } else {
            // Failure toast only, with no row count: dispatch can answer
            // 500 after the run row is already persisted (observed on a
            // cold stack — the row exists, the retry then answers 201),
            // so a count here would pin server internals, not the gate.
            // Cleanup below removes any such orphan.
            expect(dispatch.toast).toMatch(failed);
          }
          expect(dispatch.requestUrls.some((url) => url.includes("/api/runners/runs/"))).toBe(true);
          expect(enrollmentTraffic(await driver.desktopRuntimeSpyUrls())).toEqual([]);
        });
      } finally {
        await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
      }

      await test.step("the executor selection is left untouched", async () => {
        expect(await serverDeskRuntimeIssueExecutor(seed.workspaceSlug, seed.projectId, issue.id, session)).toEqual(
          before
        );
        const keys = await driver.desktopRuntimeStorageKeys();
        expect(keys.session.filter((key) => key.startsWith("pidash-managed-workspaces:"))).toEqual([]);
      });
    } finally {
      // deleteIssue nulls work_item instead of cascading, so attached runs
      // from either path are captured and deleted first — or each attempt
      // leaks rows.
      const leftovers = (await recentRuns(session))
        .filter((row) => row["work_item"] === issue.id)
        .map((row) => row["id"] as string);
      await runsDeleteRuns(leftovers);
      await deleteIssue(seed.workspaceSlug, seed.projectId, issue.id, session).catch(() => undefined);
    }
  }
);

test(
  specTitle(["DESK-003"], "no repair notice on web project pages"),
  { tag: specTags(["DESK-003"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk003Positive(driver, seed);
      return;
    }
    await openSignedInIssue(driver, seed);

    await test.step("the pill slot stays empty and names no repair", async () => {
      expect(await driver.desktopRuntimeRuntimeBannerVisible()).toBe(false);
      expect((await driver.deskRuntimePageText()).toLowerCase()).not.toContain("repair");
    });
  }
);

test(
  specTitle(["DESK-004"], "server names its refusal reason while the picker shows no built-in entry"),
  { tag: specTags(["DESK-004"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk004Positive(driver, seed);
      return;
    }
    const session = await signInSession(seed.email, seed.password);

    await test.step("project options carry the managed reason code", async () => {
      const options = await serverDeskRuntimeProjectExecutorOptions(seed.workspaceSlug, seed.projectId, session);
      // The scratch instance leaves the managed switch off, so the first
      // gate wins deterministically.
      expect(options).toEqual(
        expect.arrayContaining([{ kind: "managed_runner", available: false, reasonCode: "managed_runner_disabled" }])
      );
    });

    await test.step("pinning the desktop target is refused with its reason", async () => {
      const issue = await ownIssueId(seed, session, `Oracle desk pin ${Date.now()}`);
      try {
        const outcome = await serverDeskRuntimePinIssueExecutorStatus(
          seed.workspaceSlug,
          seed.projectId,
          issue.id,
          "managed_runner",
          session
        );
        expect(outcome.status).toBe(400);
        expect(outcome.body).toContain("agent_executor");
      } finally {
        await deleteIssue(seed.workspaceSlug, seed.projectId, issue.id, session).catch(() => undefined);
      }
    });

    await test.step("the chat picker still offers no built-in entry", async () => {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.desktopRuntimeOpenRunners(seed.workspaceSlug);
      const links = await driver.desktopRuntimeRailChatLinks();
      expect(links.map((link) => link.href).join("\n")).not.toContain(BUILTIN_CHAT_ID);
    });
  }
);

test(
  specTitle(["DESK-005"], "the built-in contact never appears in a browser session"),
  { tag: specTags(["DESK-005"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk005Positive(driver, seed);
      return;
    }
    const runner = await plantRunner(seed, "rail", "online");
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.desktopRuntimeOpenRunners(seed.workspaceSlug);

      await test.step("the rail lists cloud runners with no built-in entry", async () => {
        await expect
          .poll(() => driver.desktopRuntimeRailChatLinks().then((links) => links.map((link) => link.name)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining([runner.name]));
        const links = await driver.desktopRuntimeRailChatLinks();
        expect(links.map((link) => link.href).join("\n")).not.toContain(BUILTIN_CHAT_ID);
        expect(await driver.desktopRuntimeRailSectionHeaders()).not.toContain("Built-in");
      });

      await test.step("the built-in chat address renders no local-mode UI", async () => {
        await driver.desktopRuntimeOpenChat(seed.workspaceSlug, BUILTIN_CHAT_ID);
        expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
        expect(await driver.desktopRuntimeApprovalModeVisible()).toBe(false);
        expect(await driver.desktopRuntimeApprovalPromptVisible()).toBe(false);
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["DESK-006"], "local chat verbs all run over REST with no native bridge"),
  { tag: specTags(["DESK-006"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk006Positive(driver, seed);
      return;
    }
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "restchat", "online");
    try {
      const chat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);

      await test.step("warm, send, cancel and close answer over REST", async () => {
        await expect(serverDeskRuntimeWarmChatSession(chat.id, session)).resolves.toMatchObject({
          status: 202,
        });
        await serverDesktopRuntimeSendChatMessage(chat.id, "rest turn over the cloud relay", session);
        // Warming mid-turn is a no-op rather than an error: the turn the
        // send parked is still the active one.
        const rewarm = await serverDeskRuntimeWarmChatSession(chat.id, session);
        expect(rewarm.status).toBe(200);
        expect(JSON.parse(rewarm.body) as { skipped?: unknown }).toMatchObject({
          skipped: "chat_turn_active",
        });
        await expect(serverDeskRuntimeCancelChatSession(chat.id, session)).resolves.toMatchObject({
          status: 200,
        });
        const idle = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
        await expect(serverDeskRuntimeCloseChatSession(idle.id, session)).resolves.toMatchObject({
          status: 200,
        });
        await expect(serverDeskRuntimeChatSessionDetail(idle.id, session)).resolves.toMatchObject({
          status: "closed",
          closeRequested: false,
        });
      });

      await test.step("the transcript renders in the browser with no native bridge", async () => {
        await openSignedInChat(driver, seed, runner.id, chat.id);
        expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
        await expect
          .poll(() => driver.desktopRuntimeChatBubbles().then((bubbles) => bubbles.map((bubble) => bubble.text)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining(["rest turn over the cloud relay"]));
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["DESK-007"], "no per-session approval mode anywhere on web"),
  { tag: specTags(["DESK-007"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk007Positive(driver, seed);
      return;
    }
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "nomode", "online");
    try {
      const first = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await serverDesktopRuntimeSendChatMessage(first.id, "first session turn", session);
      // A second session with its own message: the desktop would track a
      // mode per session, so both must stay mode-less here.
      const second = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await serverDesktopRuntimeSendChatMessage(second.id, "second session turn", session);

      await test.step("neither session shows a mode control", async () => {
        await openSignedInChat(driver, seed, runner.id, first.id);
        await expect
          .poll(() => driver.desktopRuntimeChatBubbles().then((bubbles) => bubbles.map((bubble) => bubble.text)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining(["first session turn"]));
        expect(await driver.desktopRuntimeApprovalModeVisible()).toBe(false);
        await driver.desktopRuntimeOpenChat(seed.workspaceSlug, runner.id, second.id);
        await expect
          .poll(() => driver.desktopRuntimeChatBubbles().then((bubbles) => bubbles.map((bubble) => bubble.text)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining(["second session turn"]));
        expect(await driver.desktopRuntimeApprovalModeVisible()).toBe(false);
      });

      await test.step("a reload keeps no per-runner mode choice", async () => {
        await driver.page.reload();
        await expect
          .poll(() => driver.desktopRuntimeChatBubbles().then((bubbles) => bubbles.map((bubble) => bubble.text)), {
            timeout: 30_000,
          })
          .toEqual(expect.arrayContaining(["second session turn"]));
        expect(await driver.desktopRuntimeApprovalModeVisible()).toBe(false);
        const keys = await driver.desktopRuntimeStorageKeys();
        expect(keys.local.filter((key) => key.startsWith("pidash:chat-approval-mode:"))).toEqual([]);
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["DESK-008"], "chat approvals queue for decision instead of prompting inline"),
  { tag: specTags(["DESK-008"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk008Positive(driver, seed);
      return;
    }
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "queue", "online");
    try {
      const chat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await serverDesktopRuntimeSendChatMessage(chat.id, "turn with queued approvals", session);
      const kinds = ["command_execution", "file_change", "network_access"];
      const planted: string[] = [];
      for (const kind of kinds) {
        planted.push(
          await serverDesktopRuntimePlantChatApproval({
            sessionId: chat.id,
            localApprovalId: `parity-207-${kind}-${Date.now()}`,
            kind,
            reason: "parity probe",
            payload: { command: "parity probe", cwd: "/tmp" },
          })
        );
      }

      await test.step("every kind lands in the queue with nothing inline", async () => {
        const queued = await serverDesktopRuntimeListChatApprovals(wsId, session);
        for (const id of planted) {
          expect(queued).toEqual(expect.arrayContaining([expect.objectContaining({ id, status: "pending" })]));
        }
        await openSignedInChat(driver, seed, runner.id, chat.id);
        await expect
          .poll(() => driver.desktopRuntimeChatBubbles().then((bubbles) => bubbles.map((bubble) => bubble.text)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining(["turn with queued approvals"]));
        expect(await driver.desktopRuntimeApprovalPromptVisible()).toBe(false);
      });

      await test.step("deciding through the queue resolves the request", async () => {
        const decided = await serverDeskRuntimeDecideChatApproval(planted[0] ?? "", "accept", session);
        expect(decided).toMatchObject({ status: "accepted", decisionSource: "web" });
        const queued = await serverDesktopRuntimeListChatApprovals(wsId, session);
        expect(queued.map((row) => row.id)).not.toContain(planted[0]);
        expect(await driver.desktopRuntimeApprovalPromptVisible()).toBe(false);
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["DESK-009"], "chat history lives server-side with nothing kept locally"),
  { tag: specTags(["DESK-009"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk009Positive(driver, seed);
      return;
    }
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "history", "online");
    try {
      const chat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      const turns = ["history turn one", "history turn two", "history turn three"];

      await test.step("turns persist server-side in order", async () => {
        // The server admits one turn at a time; each turn is settled the
        // way the daemon's completion upstream would before the next send.
        for (const [index, turn] of turns.entries()) {
          if (index > 0) await serverDeskRuntimeSettleChatTurn(chat.id);
          await serverDesktopRuntimeSendChatMessage(chat.id, turn, session);
        }
        const stored = await serverDesktopRuntimeChatMessages(chat.id, session);
        expect(stored.map((message) => message.content)).toEqual(turns);
        expect(stored.map((message) => message.seq)).toEqual([...stored.map((message) => message.seq)].sort());
        expect(await serverDesktopRuntimeListChatSessions(wsId, runner.id, session)).toEqual(
          expect.arrayContaining([expect.objectContaining({ id: chat.id })])
        );
      });

      await test.step("the transcript renders with no local history store", async () => {
        await openSignedInChat(driver, seed, runner.id, chat.id);
        await expect
          .poll(() => driver.desktopRuntimeChatBubbles().then((bubbles) => bubbles.map((bubble) => bubble.text)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining(turns));
        const keys = await driver.desktopRuntimeStorageKeys();
        expect(keys.local.filter((key) => key.toLowerCase().includes("chat-history"))).toEqual([]);
        expect(keys.session.filter((key) => key.toLowerCase().includes("chat-history"))).toEqual([]);
        const databases = await driver.deskRuntimeIndexedDatabaseNames();
        expect(databases.filter((name) => name.toLowerCase().includes("chat"))).toEqual([]);
        expect(databases.filter((name) => name.toLowerCase().includes("history"))).toEqual([]);
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["DESK-010"], "sign-out keeps server history and tears down nothing local"),
  { tag: specTags(["DESK-010"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk010Positive(driver, seed);
      return;
    }
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "keep", "online");
    try {
      const chat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await serverDesktopRuntimeSendChatMessage(chat.id, "turn kept across sign-out", session);

      await openSignedInIssue(driver, seed);
      await driver.desktopRuntimeStartRequestSpy();
      try {
        await test.step("sign-out ends the session with no enrollment calls", async () => {
          await driver.signOutViaAccountMenu();
          expect(await driver.isSignedOut()).toBe(true);
          const enrolls = (await driver.desktopRuntimeSpyUrls()).filter((url) => url.includes("/desktop-enroll/"));
          expect(enrolls).toEqual([]);
          const keys = await driver.desktopRuntimeStorageKeys();
          expect(keys.session.filter((key) => key.startsWith("pidash-managed-workspaces:"))).toEqual([]);
        });
      } finally {
        await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
      }

      await test.step("the transcript survives the round trip", async () => {
        await driver.openEntry();
        await driver.signInWithPassword(seed.email, seed.password);
        const stored = await serverDesktopRuntimeChatMessages(chat.id, await signInSession(seed.email, seed.password));
        expect(stored).toEqual([expect.objectContaining({ content: "turn kept across sign-out" })]);
        await driver.desktopRuntimeOpenChat(seed.workspaceSlug, runner.id, chat.id);
        await expect
          .poll(() => driver.desktopRuntimeChatBubbles().then((bubbles) => bubbles.map((bubble) => bubble.text)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining(["turn kept across sign-out"]));
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["DESK-026"], "engine-setup refresh stays quiet on web"),
  { tag: specTags(["DESK-026"]) },
  async ({ driver, seed }) => {
    if (driver.target === "web_new") {
      await desk026Positive(driver, seed);
      return;
    }
    await openSignedInIssue(driver, seed);

    await driver.desktopRuntimeStartRequestSpy();
    try {
      await test.step("returning focus issues no engine-setup traffic", async () => {
        // The desktop re-reads the engine profile on window focus; on web
        // the same signal must reach no desktop endpoint.
        await driver.deskRuntimeDispatchWindowFocus();
        await driver.page.waitForTimeout(2_000);
        expect(enrollmentTraffic(await driver.desktopRuntimeSpyUrls())).toEqual([]);
        expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
      });
    } finally {
      await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
    }

    await test.step("no engine credential residue is kept", async () => {
      const keys = await driver.desktopRuntimeStorageKeys();
      expect(keys.session.filter((key) => key.startsWith("pidash-managed-workspaces:"))).toEqual([]);
      expect(keys.local.filter((key) => key.startsWith("pidash-managed-workspaces:"))).toEqual([]);
    });
  }
);

// ---------------------------------------------------------------------------
// Desktop-positive legs (NEWFRONT-595). The oracle above pins the web side
// (no enrollment, no chat, no daemon); these legs run only on web_new in a
// desktop-mode run and prove the supervisor lifecycle, the run-press gate,
// the repaired-missing notice, the refusal vocabulary, the built-in rail
// contact, local chat over the mocked bridge, per-session modes, inline
// approvals, on-machine history, the sign-out teardown and the focus
// refresh. The scratch stack ships the CE model seam, which never offers
// the desktop a model lane: no enrollment, warm, send, engine-config or
// credential step can run behind it. Legs assert the honest fallback the
// page owns (profile consulted, host checked, refusal named, teardown
// and direct-bridge history verbs working); the inventory row records
// the seam and the cloud-only remainder.
// ---------------------------------------------------------------------------

const RUNTIME_POLL = { timeout: 60_000 };
const NEW_DAMAGED_TEXT = "The desktop app's bundled agent is damaged. Reinstall the app to repair it.";
const NEW_NO_MODEL_TEXT = "No model is configured for AI runs yet. Set one up in the AI settings.";

/** Install the mock bridge, then plant a signed-in session for desktop legs. */
async function enterRuntimeSignedIn(
  driver: ParityDriver,
  email: string,
  password: string,
  mock: Parameters<typeof installDesktopMock>[1] = {},
  desktopMarked = true
): Promise<string> {
  await installDesktopMock(driver.page, mock);
  const session = await signInSession(email, password);
  await serverSetTourCompleted(session, true);
  if (desktopMarked) await serverDesktopRuntimeMarkSessionDesktop(session);
  await driver.page.context().addCookies(sessionBrowserCookies(session));
  return session;
}

/** Own issue with its desktop work-item key. */
async function runtimeIssue(
  seed: ParitySeedFacts,
  session: string,
  name: string
): Promise<{ id: string; seq: string }> {
  const identifier = await serverProjectIdentifier(seed.workspaceSlug, seed.projectId, session);
  const created = await createIssue(seed.workspaceSlug, seed.projectId, session, name);
  return { id: created.id, seq: `${identifier}-${created.sequence_id}` };
}

function enrollPosts(urls: string[]): string[] {
  return urls.filter((url) => url.includes("/desktop-enroll/"));
}

/** The chat session id the page currently shows, from its address. */
function shownChatSession(driver: ParityDriver): string | null {
  try {
    return new URL(driver.page.url()).searchParams.get("sessionId");
  } catch {
    return null;
  }
}

/**
 * One built-in session on screen and selected into the address. Opening
 * the chat auto-creates a session through warm-up without selecting it,
 * and a "New chat" click races that create (the control is busy while it
 * runs), so legs wait for the auto-created row and click it instead of
 * pressing New chat into an empty state.
 */
async function ensureChatSelected(driver: ParityDriver): Promise<string> {
  await expect
    .poll(async () => (await desktopChatSessions(driver.page)).length, RUNTIME_POLL)
    .toBeGreaterThanOrEqual(1);
  const current = shownChatSession(driver);
  if (current !== null) return current;
  const history = driver.page.getByTestId("runner-chat-history");
  await history.getByText("Mocked chat", { exact: true }).first().click();
  await expect.poll(() => shownChatSession(driver), RUNTIME_POLL).not.toBeNull();
  return shownChatSession(driver) as string;
}

async function sendChatMessage(driver: ParityDriver, text: string): Promise<void> {
  await driver.page.getByRole("textbox", { name: "Message" }).fill(text);
  await driver.page.getByRole("button", { name: "Send message", exact: true }).click();
}

/**
 * Send against the refused engine: the refusal names the missing model,
 * the draft is restored beside it, the user turn reaches the on-machine
 * store but no send does — and the stored turn renders once the
 * transcript refetches (a failed send invalidates nothing, so reload).
 */
async function sendChatMessageStored(driver: ParityDriver, text: string): Promise<void> {
  await sendChatMessage(driver, text);
  await expect.poll(() => driver.pageText(), RUNTIME_POLL).toContain(NEW_NO_MODEL_TEXT);
  await expect
    .poll(() => driver.page.getByRole("textbox", { name: "Message" }).inputValue(), RUNTIME_POLL)
    .toContain(text);
  await expect
    .poll(async () => (await desktopInvocations(driver.page, "chat_append_event")).length, RUNTIME_POLL)
    .toBeGreaterThanOrEqual(1);
  expect(await desktopInvocations(driver.page, "chat_send")).toEqual([]);
  await driver.page.reload();
  await expect
    .poll(async () => (await driver.desktopRuntimeChatBubbles()).map((bubble) => bubble.text).join("\n"), RUNTIME_POLL)
    .toContain(text);
}

async function desk001Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await enterRuntimeSignedIn(driver, seed.email, seed.password);
  const issue = await runtimeIssue(seed, session, deskTag("enroll"));
  try {
    await driver.desktopRuntimeStartRequestSpy();
    try {
      await test.step("opening the project consults the engine profile and names its refusal", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
        await expect.poll(() => driver.desktopRuntimeRuntimeBannerVisible(), RUNTIME_POLL).toBe(true);
        // The scratch stack ships the CE model seam, which never offers the
        // desktop a model lane: the profile answers unavailable and the
        // notice carries its sentence instead of a preparing state.
        await expect
          .poll(() => driver.page.getByTestId("runtime-notice").innerText(), RUNTIME_POLL)
          .toContain(NEW_NO_MODEL_TEXT);
      });

      await test.step("the host self-check runs but nothing enrolls or starts without an engine", async () => {
        await expect
          .poll(async () => (await desktopInvocations(driver.page, "managed_doctor")).length, RUNTIME_POLL)
          .toBeGreaterThanOrEqual(1);
        await expect
          .poll(async () => (await desktopInvocations(driver.page, "managed_stop_daemon")).length, RUNTIME_POLL)
          .toBeGreaterThanOrEqual(1);
        expect(enrollPosts(await driver.desktopRuntimeSpyUrls())).toEqual([]);
        for (const command of ["managed_bootstrap", "managed_enroll", "managed_start_daemon"]) {
          expect(await desktopInvocations(driver.page, command)).toEqual([]);
        }
        const keys = await driver.desktopRuntimeStorageKeys();
        expect(keys.session.filter((key) => key.startsWith("pidash-managed-workspaces:"))).toEqual([]);
      });

      await test.step("leaving the project unmounts the project-scoped notice", async () => {
        await driver.desktopRuntimeOpenRunners(seed.workspaceSlug);
        await expect.poll(() => driver.desktopRuntimeRuntimeBannerVisible(), RUNTIME_POLL).toBe(false);
      });
    } finally {
      await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
    }
  } finally {
    await deleteIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
  }

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk002Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await enterRuntimeSignedIn(driver, seed.email, seed.password);
  const issue = await runtimeIssue(seed, session, deskTag("press"));
  try {
    await test.step("open the issue so the supervisor settles first", async () => {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect.poll(() => driver.desktopRuntimeRuntimeBannerVisible(), RUNTIME_POLL).toBe(true);
      await expect
        .poll(async () => (await desktopInvocations(driver.page, "managed_doctor")).length, RUNTIME_POLL)
        .toBeGreaterThanOrEqual(1);
    });

    await test.step("the visit leaves the selection alone and enrolls nothing", async () => {
      const selectionBefore = await serverDeskRuntimeIssueExecutor(
        seed.workspaceSlug,
        seed.projectId,
        issue.id,
        session
      );
      await driver.desktopRuntimeStartRequestSpy();
      try {
        await driver.page.reload();
        await expect.poll(() => driver.desktopRuntimeRuntimeBannerVisible(), RUNTIME_POLL).toBe(true);
        expect(enrollPosts(await driver.desktopRuntimeSpyUrls())).toEqual([]);
      } finally {
        await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
      }
      expect(await desktopInvocations(driver.page, "managed_enroll")).toEqual([]);
      expect(await desktopInvocations(driver.page, "managed_start_daemon")).toEqual([]);
      expect(await serverDeskRuntimeIssueExecutor(seed.workspaceSlug, seed.projectId, issue.id, session)).toEqual(
        selectionBefore
      );
      const keys = await driver.desktopRuntimeStorageKeys();
      expect(keys.session.filter((key) => key.startsWith("pidash-managed-workspaces:"))).toEqual([]);
    });

    await test.step("no Run control exists yet to press (tripwire for the RUN area)", async () => {
      // The readiness gate itself (prepareAgentRun's early return and its
      // project connection) is unit-covered; the press half of this row
      // needs a Run control on the issue detail, which this build has not
      // got. This pins the gap so the day the control lands the leg fails
      // and grows the press proof instead of silently staying partial.
      await expect.poll(() => driver.page.getByTestId("issue-run-ai").count(), RUNTIME_POLL).toBe(0);
    });
  } finally {
    await runsDeleteRuns(seed.workspaceSlug, seed.projectId, issue.id, session).catch(() => undefined);
    await deleteIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
  }

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk003Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await enterRuntimeSignedIn(
    driver,
    seed.email,
    seed.password,
    {
      doctor: { runnerPresent: false, enginePresent: true, hostLabel: "parity-host" },
    },
    false
  );
  const issue = await runtimeIssue(seed, session, deskTag("repair"));
  try {
    await driver.desktopRuntimeStartRequestSpy();
    try {
      await test.step("the missing binary surfaces its repair notice", async () => {
        await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
        await expect
          .poll(() => driver.page.getByTestId("runtime-notice").innerText(), RUNTIME_POLL)
          .toContain(NEW_DAMAGED_TEXT);
      });

      await test.step("nothing enrolls or starts behind the damaged agent", async () => {
        expect(enrollPosts(await driver.desktopRuntimeSpyUrls())).toEqual([]);
        for (const command of ["managed_bootstrap", "managed_enroll", "managed_start_daemon"]) {
          expect(await desktopInvocations(driver.page, command)).toEqual([]);
        }
        expect((await desktopInvocations(driver.page, "managed_doctor")).length).toBeGreaterThanOrEqual(1);
      });

      await test.step("refocusing retries the host check", async () => {
        const before = (await desktopInvocations(driver.page, "managed_doctor")).length;
        await driver.deskRuntimeDispatchWindowFocus();
        await expect
          .poll(async () => (await desktopInvocations(driver.page, "managed_doctor")).length, RUNTIME_POLL)
          .toBeGreaterThan(before);
      });
    } finally {
      await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
    }
  } finally {
    await deleteIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
  }

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk004Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await enterRuntimeSignedIn(driver, seed.email, seed.password);
  const issue = await runtimeIssue(seed, session, deskTag("refusal"));

  await test.step("the project options refuse the managed executor deterministically", async () => {
    const options = await serverDeskRuntimeProjectExecutorOptions(seed.workspaceSlug, seed.projectId, session);
    // The scratch instance leaves the managed switch off, so the first
    // gate wins deterministically.
    expect(options).toEqual(
      expect.arrayContaining([{ kind: "managed_runner", available: false, reasonCode: "managed_runner_disabled" }])
    );
  });

  await test.step("pinning the refusal stays impossible", async () => {
    const outcome = await serverDeskRuntimePinIssueExecutorStatus(
      seed.workspaceSlug,
      seed.projectId,
      issue.id,
      "managed_runner",
      session
    );
    expect(outcome.status).toBe(400);
    expect(outcome.body).toContain("agent_executor");
  });

  await test.step("the execution picker shows the managed row disabled with its reason", async () => {
    await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
    await expect.poll(() => driver.issueDetailTitle(), RUNTIME_POLL).not.toBeNull();
    // The trigger carries the current selection, not a fixed label.
    await driver.page.getByRole("group", { name: "Runs on" }).getByRole("button").first().click();
    const list = driver.page.getByRole("listbox", { name: "Execution targets" });
    await expect.poll(() => list.count(), RUNTIME_POLL).toBe(1);
    const row = list.getByRole("option", { name: /Pi Dash Agent \(desktop\)/ });
    await expect.poll(() => row.count(), RUNTIME_POLL).toBe(1);
    await expect.poll(() => row.first().isDisabled(), RUNTIME_POLL).toBe(true);
    expect(
      ((await row
        .first()
        .innerText()
        .catch(() => "")) ?? "") as string
    ).toContain("The built-in agent is switched off on this server.");
    await driver.page.keyboard.press("Escape").catch(() => undefined);
  });

  await test.step("the rail contact carries the same refusal instead of a link", async () => {
    await driver.desktopRuntimeOpenRunners(seed.workspaceSlug);
    // The rail resolves link-vs-reason behind the availability read: wait
    // for the refusal to land before asserting no link (an early read can
    // catch the optimistic link).
    const reason = driver.page.getByTestId("runners-rail-local-reason");
    await expect.poll(() => reason.count(), RUNTIME_POLL).toBe(1);
    expect(
      (
        ((await reason
          .first()
          .innerText()
          .catch(() => "")) ?? "") as string
      ).trim()
    ).not.toBe("");
    const links = await driver.desktopRuntimeRailChatLinks();
    expect(links.filter((link) => link.href.includes(BUILTIN_CHAT_ID))).toEqual([]);
  });

  await deleteIssue(seed.workspaceSlug, seed.projectId, issue.id, session);

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk005Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await enterRuntimeSignedIn(driver, seed.email, seed.password);
  const online = await plantRunner(seed, "newleg", "online");
  try {
    await test.step("the runners rail lists the built-in contact beside cloud runners", async () => {
      await driver.desktopRuntimeOpenRunners(seed.workspaceSlug);
      await expect.poll(() => driver.desktopRuntimeRailSectionHeaders(), RUNTIME_POLL).toContain("BUILT-IN");
      const links = await driver.desktopRuntimeRailChatLinks();
      expect(links.map((link) => link.name).join("\n")).toContain(online.name);
    });

    await test.step("the built-in chat opens its local-mode surface directly", async () => {
      await driver.desktopRuntimeOpenChat(seed.workspaceSlug, BUILTIN_CHAT_ID);
      await expect.poll(() => driver.desktopRuntimeApprovalModeVisible(), RUNTIME_POLL).toBe(true);
      expect(await driver.desktopRuntimeIsTauriPresent()).toBe(true);
      expect(await driver.desktopRuntimeApprovalPromptVisible()).toBe(false);
    });
  } finally {
    await serverDesktopRuntimeCleanupRunner(online.id);
  }

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk006Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await enterRuntimeSignedIn(driver, seed.email, seed.password);

  await test.step("a cloud runner holds a settled turn over REST", async () => {
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "newleg", "online");
    try {
      const chat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await expect(serverDeskRuntimeWarmChatSession(chat.id, session)).resolves.toMatchObject({ status: 202 });
      await serverDesktopRuntimeSendChatMessage(chat.id, "cloud turn one", session);
      await serverDeskRuntimeSettleChatTurn(chat.id);
      const messages = await serverDesktopRuntimeChatMessages(chat.id, session);
      expect(messages.map((message) => message.content).join("\n")).toContain("cloud turn one");
      await expect(serverDeskRuntimeCancelChatSession(chat.id, session)).resolves.toMatchObject({ status: 200 });
      const idle = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await expect(serverDeskRuntimeCloseChatSession(idle.id, session)).resolves.toMatchObject({ status: 200 });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  });

  await test.step("opening the built-in chat attempts the daemon bring-up quietly", async () => {
    // Warm-up only saves latency: its refusal is swallowed and the session
    // stays usable, but the bring-up must still have been attempted.
    await driver.desktopRuntimeOpenChat(seed.workspaceSlug, BUILTIN_CHAT_ID);
    await ensureChatSelected(driver);
    await expect
      .poll(async () => (await desktopInvocations(driver.page, "managed_doctor")).length, RUNTIME_POLL)
      .toBeGreaterThanOrEqual(1);
    expect(await desktopInvocations(driver.page, "chat_warm")).toEqual([]);
    await driver.page.getByRole("textbox", { name: "Message" }).fill("a draft stays sendable");
    const send = driver.page.getByRole("button", { name: "Send message", exact: true });
    await expect.poll(() => send.isDisabled(), RUNTIME_POLL).toBe(false);
    await driver.page.getByRole("textbox", { name: "Message" }).fill("");
  });

  await test.step("sending stores the user turn, then names the missing engine", async () => {
    await sendChatMessageStored(driver, "local turn one");
    await driver.page.getByRole("textbox", { name: "Message" }).fill("another draft stays sendable");
    const send = driver.page.getByRole("button", { name: "Send message", exact: true });
    await expect.poll(() => send.isDisabled(), RUNTIME_POLL).toBe(false);
    await driver.page.getByRole("textbox", { name: "Message" }).fill("");
  });

  await test.step("no phantom busy state parks the composer without a live turn", async () => {
    await expect.poll(() => driver.page.getByRole("button", { name: "Stop response" }).count(), RUNTIME_POLL).toBe(0);
    expect(await desktopInvocations(driver.page, "chat_cancel")).toEqual([]);
  });

  await test.step("closing the conversation closes it natively", async () => {
    await driver.page.getByRole("button", { name: "Close chat" }).click();
    await expect
      .poll(async () => (await desktopInvocations(driver.page, "chat_close")).length, RUNTIME_POLL)
      .toBeGreaterThanOrEqual(1);
  });

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk007Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await enterRuntimeSignedIn(driver, seed.email, seed.password);

  await test.step("the mode control pins a choice that survives reload", async () => {
    await driver.desktopRuntimeOpenChat(seed.workspaceSlug, BUILTIN_CHAT_ID);
    await expect.poll(() => driver.desktopRuntimeApprovalModeVisible(), RUNTIME_POLL).toBe(true);
    const mode = driver.page.getByRole("group", { name: "Approval mode" });
    await mode.getByRole("button", { name: "Working copy", exact: true }).click();
    await expect
      .poll(() => mode.getByRole("button", { name: "Working copy", exact: true }).getAttribute("aria-pressed"))
      .toBe("true");
    const keys = await driver.desktopRuntimeStorageKeys();
    expect(keys.local).toContain(`pidash:chat-approval-mode:${BUILTIN_CHAT_ID}`);
    await driver.page.reload();
    await expect
      .poll(
        () =>
          driver.page
            .getByRole("group", { name: "Approval mode" })
            .getByRole("button", { name: "Working copy", exact: true })
            .getAttribute("aria-pressed"),
        RUNTIME_POLL
      )
      .toBe("true");
  });

  await test.step("the pinned choice survives failed turns on new sessions", async () => {
    // No warm or send can reach the bridge without an available engine
    // (DESK-001 row note), so the mode capture travels no observable call
    // here; what the page owns is that the choice stays pinned across
    // sessions and refusals instead of resetting.
    const mode = driver.page.getByRole("group", { name: "Approval mode" });
    const choice = (): Promise<string | null> =>
      mode.getByRole("button", { name: "Working copy", exact: true }).getAttribute("aria-pressed");
    const before = await ensureChatSelected(driver);
    await driver.page.getByRole("button", { name: "New chat", exact: true }).click();
    await expect.poll(() => shownChatSession(driver), RUNTIME_POLL).not.toBe(before);
    await sendChatMessageStored(driver, "mode turn one");
    await expect.poll(() => choice(), RUNTIME_POLL).toBe("true");
    expect(await desktopInvocations(driver.page, "chat_warm")).toEqual([]);
    expect(await desktopInvocations(driver.page, "chat_send")).toEqual([]);
  });

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk008Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await enterRuntimeSignedIn(driver, seed.email, seed.password, { chatHold: true });

  await test.step("a seeded approval resolves through its own endpoint", async () => {
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "newleg", "online");
    try {
      const chat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await serverDesktopRuntimeSendChatMessage(chat.id, "turn with queued approvals", session);
      const approvalId = await serverDesktopRuntimePlantChatApproval({
        sessionId: chat.id,
        localApprovalId: `parity-595-approval-${Date.now()}`,
        kind: "command_execution",
        reason: "parity probe",
        payload: { command: "cargo test --workspace --offline", cwd: "/tmp" },
      });
      const queued = await serverDesktopRuntimeListChatApprovals(wsId, session);
      expect(queued).toEqual(expect.arrayContaining([expect.objectContaining({ id: approvalId, status: "pending" })]));
      await expect(serverDeskRuntimeDecideChatApproval(approvalId, "accept", session)).resolves.toMatchObject({
        status: "accepted",
        decisionSource: "web",
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  });

  await test.step("the engine's approval renders inline with its target and reason", async () => {
    await driver.desktopRuntimeOpenChat(seed.workspaceSlug, BUILTIN_CHAT_ID);
    const chatId = await ensureChatSelected(driver);
    await sendChatMessage(driver, "please run the tests");
    await desktopEmit(driver.page, "chat://frame", {
      result: "chat_approval_request",
      data: {
        chat_session_id: chatId,
        local_approval_id: "mock-approval-1",
        kind: "command_execution",
        reason: "mock tests need running",
        expires_at: null,
        payload: { command: "cargo test --workspace --offline" },
      },
    });
    await expect.poll(() => driver.desktopRuntimeApprovalPromptVisible(), RUNTIME_POLL).toBe(true);
    expect(await driver.page.getByTestId("chat-approval-target").innerText()).toContain("cargo test");
    expect(await driver.page.getByTestId("chat-approval-reason").innerText()).toContain("mock tests need running");
  });

  await test.step("approving decides natively and clears the prompt", async () => {
    await driver.page.getByRole("button", { name: "Approve", exact: true }).click();
    await expect
      .poll(async () => (await desktopInvocations(driver.page, "chat_decide")).length, RUNTIME_POLL)
      .toBeGreaterThanOrEqual(1);
    const decisions = await desktopInvocations(driver.page, "chat_decide");
    expect((decisions[0]?.args as { decision?: unknown } | undefined)?.decision).toBe("accept");
    await expect.poll(() => driver.desktopRuntimeApprovalPromptVisible(), RUNTIME_POLL).toBe(false);
    const chatId = shownChatSession(driver);
    await desktopEmit(driver.page, "chat://frame", {
      result: "chat_message_completed",
      data: {
        chat_session_id: chatId,
        message_id: "mock-message-approval",
        assistant_message: "mocked tests passed",
        status: "completed",
      },
    });
    await expect
      .poll(
        async () => (await driver.desktopRuntimeChatBubbles()).map((bubble) => bubble.text).join("\n"),
        RUNTIME_POLL
      )
      .toContain("mocked tests passed");
  });

  await test.step("a command also offers always-allow for the session", async () => {
    await sendChatMessage(driver, "please run the tests again");
    const chatId = shownChatSession(driver);
    await desktopEmit(driver.page, "chat://frame", {
      result: "chat_approval_request",
      data: {
        chat_session_id: chatId,
        local_approval_id: "mock-approval-2",
        kind: "command_execution",
        reason: "",
        expires_at: null,
        payload: { command: "cargo test --workspace --offline" },
      },
    });
    await expect.poll(() => driver.desktopRuntimeApprovalPromptVisible(), RUNTIME_POLL).toBe(true);
    await driver.page.getByRole("button", { name: "Always allow this session", exact: true }).click();
    await expect
      .poll(async () => (await desktopInvocations(driver.page, "chat_decide")).length, RUNTIME_POLL)
      .toBeGreaterThanOrEqual(2);
    const decisions = await desktopInvocations(driver.page, "chat_decide");
    expect((decisions[decisions.length - 1]?.args as { decision?: unknown } | undefined)?.decision).toBe(
      "accept_for_session"
    );
  });

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk009Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await enterRuntimeSignedIn(driver, seed.email, seed.password);

  await test.step("cloud turns still persist server-side", async () => {
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "newleg", "online");
    try {
      const chat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await serverDesktopRuntimeSendChatMessage(chat.id, "persisted turn", session);
      await serverDeskRuntimeSettleChatTurn(chat.id);
      const messages = await serverDesktopRuntimeChatMessages(chat.id, session);
      expect(messages.map((message) => message.content).join("\n")).toContain("persisted turn");
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  });

  await test.step("local turns persist on the machine behind native history verbs", async () => {
    await driver.desktopRuntimeOpenChat(seed.workspaceSlug, BUILTIN_CHAT_ID);
    await ensureChatSelected(driver);
    await sendChatMessageStored(driver, "remember this turn");
    const history = driver.page.getByTestId("runner-chat-history");
    await expect.poll(() => history.getByText("Mocked chat", { exact: true }).count(), RUNTIME_POLL).toBe(1);
  });

  await test.step("renaming retitles the stored conversation", async () => {
    const history = driver.page.getByTestId("runner-chat-history");
    await history.getByTestId("runner-chat-row-menu").first().click();
    await driver.page.getByRole("menuitem", { name: "Rename", exact: true }).click();
    await driver.page.getByRole("textbox", { name: "Name" }).fill("Renamed one");
    await driver.page.getByRole("button", { name: "Save name", exact: true }).click();
    await expect.poll(() => history.getByText("Renamed one", { exact: true }).count(), RUNTIME_POLL).toBe(1);
    expect((await desktopInvocations(driver.page, "chat_rename_session")).length).toBe(1);
  });

  await test.step("deleting drops one conversation with confirmation", async () => {
    const history = driver.page.getByTestId("runner-chat-history");
    await driver.page.getByRole("button", { name: "New chat", exact: true }).click();
    await expect.poll(() => history.getByText("Mocked chat", { exact: true }).count(), RUNTIME_POLL).toBe(1);
    await history.getByTestId("runner-chat-row-menu").first().click();
    await driver.page.getByRole("menuitem", { name: "Delete", exact: true }).click();
    await expect.poll(() => driver.pageText(), RUNTIME_POLL).toContain("Delete this chat?");
    await driver.page.getByRole("button", { name: "Delete chat", exact: true }).click();
    await expect
      .poll(async () => (await desktopInvocations(driver.page, "chat_delete_session")).length, RUNTIME_POLL)
      .toBe(1);
  });

  await test.step("clearing wipes the on-machine history with confirmation", async () => {
    await driver.page.getByTestId("runner-chat-history-clear").click();
    await expect.poll(() => driver.pageText(), RUNTIME_POLL).toContain("Clear the on-machine history?");
    await driver.page.getByRole("button", { name: "Clear history", exact: true }).click();
    await expect
      .poll(async () => (await desktopInvocations(driver.page, "chat_clear_history")).length, RUNTIME_POLL)
      .toBe(1);
    await expect.poll(() => driver.pageText(), RUNTIME_POLL).toContain("No chats yet.");
  });

  await test.step("history survives a reload from the on-machine store", async () => {
    await ensureChatSelected(driver);
    await sendChatMessageStored(driver, "surviving turn");
    await expect
      .poll(
        () => driver.page.getByTestId("runner-chat-history").getByText("Mocked chat", { exact: true }).count(),
        RUNTIME_POLL
      )
      .toBe(1);
    expect((await desktopInvocations(driver.page, "chat_list_sessions")).length).toBeGreaterThanOrEqual(1);
  });

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk010Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await enterRuntimeSignedIn(driver, seed.email, seed.password);

  const runner = await plantRunner(seed, "newleg", "online");
  const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
  const cloudChat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
  await serverDesktopRuntimeSendChatMessage(cloudChat.id, "kept across sign-out", session);
  await serverDeskRuntimeSettleChatTurn(cloudChat.id);
  const issue = await runtimeIssue(seed, session, deskTag("signout"));

  async function signOutDesktop(deleteHistory: boolean): Promise<void> {
    const trigger = driver.page.getByTestId("sidebar-user-menu").first();
    await trigger.waitFor(RUNTIME_POLL);
    await trigger.evaluate((element) => (element as HTMLElement).click());
    const item = driver.page.getByRole("menuitem", { name: "Sign out" }).first();
    await item.waitFor(RUNTIME_POLL);
    await item.evaluate((element) => (element as HTMLElement).click());
    await expect.poll(() => driver.pageText(), RUNTIME_POLL).toContain("Sign out?");
    if (deleteHistory) {
      await driver.page.getByTestId("runtime-sign-out-delete-history").click();
    }
    await driver.page.getByRole("button", { name: "Sign out", exact: true }).last().click();
    await expect.poll(() => driver.pageText(), RUNTIME_POLL).toContain("The desktop app can't sign you in yet");
  }

  await test.step("a local turn exists before signing out", async () => {
    await driver.desktopRuntimeOpenChat(seed.workspaceSlug, BUILTIN_CHAT_ID);
    await ensureChatSelected(driver);
    await sendChatMessageStored(driver, "kept local turn");
  });

  await test.step("the supervisor connects on the issue page first", async () => {
    // Server enrollment revocation rides dispose, which only sends it when
    // this page checked the host: connect the supervisor the way opening
    // a project does before signing out.
    await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
    await expect.poll(() => driver.desktopRuntimeRuntimeBannerVisible(), RUNTIME_POLL).toBe(true);
    await expect
      .poll(async () => (await desktopInvocations(driver.page, "managed_doctor")).length, RUNTIME_POLL)
      .toBeGreaterThanOrEqual(1);
  });

  await test.step("signing out keeps history by default and tears the daemon down", async () => {
    await driver.desktopRuntimeStartRequestSpy();
    try {
      await signOutDesktop(false);
      const signOuts = await desktopInvocations(driver.page, "managed_sign_out");
      expect(signOuts.length).toBeGreaterThanOrEqual(1);
      expect(JSON.stringify(signOuts[signOuts.length - 1]?.args ?? null)).not.toContain("clearChatHistory");
      expect(await desktopInvocations(driver.page, "chat_clear_history")).toEqual([]);
      expect(await desktopInvocations(driver.page, "desktop_clear_web_data")).not.toEqual([]);
      await expect
        .poll(async () => enrollPosts(await driver.desktopRuntimeSpyUrls()).length, RUNTIME_POLL)
        .toBeGreaterThanOrEqual(1);
      const keys = await driver.desktopRuntimeStorageKeys();
      expect(keys.session.filter((key) => key.startsWith("pidash-managed-workspaces:"))).toEqual([]);
    } finally {
      await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
    }
  });

  await test.step("signing back in finds the kept history", async () => {
    const again = await signInSession(seed.email, seed.password);
    await driver.page.context().addCookies(sessionBrowserCookies(again));
    await driver.desktopRuntimeOpenChat(seed.workspaceSlug, BUILTIN_CHAT_ID);
    await expect
      .poll(
        () => driver.page.getByTestId("runner-chat-history").getByText("Mocked chat", { exact: true }).count(),
        RUNTIME_POLL
      )
      .toBe(1);
  });

  await test.step("signing out with deletion wipes the on-machine history", async () => {
    await signOutDesktop(true);
    // The page never calls chat_clear_history itself: it names the account
    // on managed_sign_out and the native side wipes. Assert the call that
    // carries the choice, then the wiped store after signing back in.
    const signOuts = await desktopInvocations(driver.page, "managed_sign_out");
    expect(JSON.stringify(signOuts.map((call) => call.args))).toContain("clearChatHistory");
    const again = await signInSession(seed.email, seed.password);
    await driver.page.context().addCookies(sessionBrowserCookies(again));
    await driver.desktopRuntimeOpenChat(seed.workspaceSlug, BUILTIN_CHAT_ID);
    // Warm-up auto-creates one fresh conversation on the wiped store; the
    // kept turn from before must be gone with the old row.
    await ensureChatSelected(driver);
    const history = driver.page.getByTestId("runner-chat-history");
    await expect.poll(() => history.getByText("No turns yet", { exact: true }).count(), RUNTIME_POLL).toBe(1);
    await expect.poll(async () => (await driver.desktopRuntimeChatBubbles()).length, RUNTIME_POLL).toBe(0);
  });

  await test.step("the cloud transcript survives both sign-outs server-side", async () => {
    const fresh = await signInSession(seed.email, seed.password);
    const messages = await serverDesktopRuntimeChatMessages(cloudChat.id, fresh);
    expect(messages.map((message) => message.content).join("\n")).toContain("kept across sign-out");
    await serverDesktopRuntimeCleanupRunner(runner.id);
  });
  await deleteIssue(seed.workspaceSlug, seed.projectId, issue.id, await signInSession(seed.email, seed.password));

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}

async function desk026Positive(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  const session = await enterRuntimeSignedIn(driver, seed.email, seed.password);
  const issue = await runtimeIssue(seed, session, deskTag("focus"));
  try {
    await test.step("open the issue so the supervisor settles on the refusal", async () => {
      await driver.openIssueDetail(seed.workspaceSlug, issue.seq);
      await expect
        .poll(() => driver.page.getByTestId("runtime-notice").innerText(), RUNTIME_POLL)
        .toContain(NEW_NO_MODEL_TEXT);
    });

    await test.step("refocusing re-reads the profile without rewriting any engine setup", async () => {
      // The live refresh re-polls the profile on focus; with no available
      // engine the notice keeps its sentence and no config or credential
      // write fires. The rewrite/rotate path needs the cloud model lane
      // (DESK-001 row note).
      const stopsBefore = (await desktopInvocations(driver.page, "managed_stop_daemon")).length;
      await driver.desktopRuntimeStartRequestSpy();
      try {
        await driver.deskRuntimeDispatchWindowFocus();
        await expect
          .poll(async () => (await desktopInvocations(driver.page, "managed_stop_daemon")).length, RUNTIME_POLL)
          .toBeGreaterThan(stopsBefore);
        const urls = await driver.desktopRuntimeSpyUrls();
        expect(urls.some((url) => url.includes("agent-profile"))).toBe(true);
        expect(await desktopInvocations(driver.page, "managed_write_engine_config")).toEqual([]);
        expect(await desktopInvocations(driver.page, "managed_write_model_token")).toEqual([]);
        expect(await driver.page.getByTestId("runtime-notice").innerText()).toContain(NEW_NO_MODEL_TEXT);
      } finally {
        await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
      }
    });

    await test.step("no engine credential residue is kept in web storage", async () => {
      const dump = await driver.page.evaluate(() => {
        const read = (storage: Storage): string => {
          const entries: string[] = [];
          for (let index = 0; index < storage.length; index++) {
            const key = storage.key(index);
            if (key !== null) entries.push(`${key}=${storage.getItem(key) ?? ""}`);
          }
          return entries.join("\n");
        };
        return `${read(window.sessionStorage)}\n${read(window.localStorage)}`;
      });
      expect(dump).not.toContain("machine_token");
      expect(dump).not.toContain("mock-token");
    });
  } finally {
    await deleteIssue(seed.workspaceSlug, seed.projectId, issue.id, session);
  }

  await test.step("the server session stays valid throughout", async () => {
    expect(await sessionValid(await signInSession(seed.email, seed.password))).toBe(true);
  });
}
