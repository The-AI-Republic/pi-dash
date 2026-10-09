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
  serverDesktopRuntimeWorkspaceId,
  serverDesktopRuntimePlantRunner,
  serverDesktopRuntimeCleanupRunner,
  serverDesktopRuntimeCreateChatSession,
  serverDesktopRuntimeListChatSessions,
  serverDesktopRuntimeChatMessages,
  serverDesktopRuntimeSendChatMessage,
  serverDesktopRuntimePlantChatApproval,
  serverDesktopRuntimeListChatApprovals,
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
          await driver.clickWidgetAction("Manually Run AI");
          // The seeded stack enrolls no runners, so dispatch reports its
          // graceful failure (pinned by ISS-192); the gate half pinned here
          // is that the press never touches the local-enrollment path.
          await expect.poll(() => driver.lastToast(), { timeout: 60_000 }).toMatch(/failed to start agent run/i);
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
      await deleteIssue(seed.workspaceSlug, seed.projectId, issue.id, session).catch(() => undefined);
    }
  }
);

test(
  specTitle(["DESK-003"], "no repair notice on web project pages"),
  { tag: specTags(["DESK-003"]) },
  async ({ driver, seed }) => {
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
