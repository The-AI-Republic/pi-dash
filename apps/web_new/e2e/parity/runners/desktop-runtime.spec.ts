// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-182): desktop-only chat and agent runtime —
// the built-in local-agent contact, inline chat approvals, the
// approval-mode selector, local chat execution/persistence/resume, the
// background agent-runtime supervisor, the per-project enrollment pill,
// and local runtime disposal on sign-out. Rows: RUN-033, RUN-034,
// RUN-035, RUN-036, RUN-044, RUN-045, RUN-047.
//
// The oracle runs on apps/web, where these capabilities are absent by
// design: every scenario pins the web-observable side (missing section /
// inert component / cloud transport) and the server gates a web session
// hits on the desktop-only endpoints.
//
// Fixtures: runners are planted through the Django shell (the web API
// offers no create-runner endpoint); online runners get a live session
// row so the outbox treats them as connected. Chat sessions, messages
// and the queue reads go through the app's own REST endpoints; the one
// daemon write-back (a pending chat approval) is planted through the
// shell. The scratch stack runs no live runner daemon.
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  signInSession,
  serverDesktopRuntimeWorkspaceId,
  serverDesktopRuntimePlantRunner,
  serverDesktopRuntimeCleanupRunner,
  serverDesktopRuntimeCreateChatSession,
  serverDesktopRuntimeListChatSessions,
  serverDesktopRuntimeChatMessages,
  serverDesktopRuntimeSendChatMessage,
  serverDesktopRuntimePlantChatApproval,
  serverDesktopRuntimeListChatApprovals,
  serverDesktopRuntimeAgentProfileRefusal,
  serverDesktopRuntimeAgentTokenRefusal,
  serverDesktopRuntimeDesktopEnrollRefusal,
  type DesktopRuntimeRunner,
  type DesktopRuntimeRunnerStatus,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

let runnerSerial = 0;

function runnerTag(prefix: string): string {
  runnerSerial += 1;
  return `nc182 ${prefix} ${Date.now()} ${runnerSerial}`;
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
    name: runnerTag(prefix),
    status,
  });
}

async function openSignedInRunners(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.desktopRuntimeOpenRunners(seed.workspaceSlug);
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

async function openSignedInProject(driver: ParityDriver, seed: ParitySeedFacts): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
  await expect
    .poll(() => driver.visibleIssueNames().then((names) => names.length), { timeout: 15_000 })
    .toBeGreaterThan(0);
}

test(
  specTitle(["RUN-033"], "no built-in contact section in the rail on web"),
  { tag: specTags(["RUN-033"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const online = await plantRunner(seed, "rail-on", "online");
    const offline = await plantRunner(seed, "rail-off", "offline");
    try {
      await openSignedInRunners(driver, seed);

      await test.step("rail lists the connected runners with no built-in section", async () => {
        await expect
          .poll(() => driver.desktopRuntimeRailChatLinks().then((links) => links.map((link) => link.name)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining([online.name, offline.name]));
        const links = await driver.desktopRuntimeRailChatLinks();
        expect(links.map((link) => link.href).join("\n")).not.toContain("pidash-builtin");
        expect(await driver.desktopRuntimeRailSectionHeaders()).not.toContain("Built-in");
      });

      await test.step("the desktop availability gate refuses the web session", async () => {
        await expect(serverDesktopRuntimeAgentProfileRefusal(session)).resolves.toEqual({
          status: 403,
          error: "desktop_session_required",
        });
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(online.id);
      await serverDesktopRuntimeCleanupRunner(offline.id);
    }
  }
);

test(
  specTitle(["RUN-034"], "pending chat approvals never render inline on web"),
  { tag: specTags(["RUN-034"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "noprompt", "online");
    try {
      const chat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await serverDesktopRuntimeSendChatMessage(chat.id, "parked turn with a pending approval", session);
      const approvalId = await serverDesktopRuntimePlantChatApproval({
        sessionId: chat.id,
        localApprovalId: `parity-182-${Date.now()}`,
        kind: "command_execution",
        reason: "parity probe",
        payload: { command: "parity probe", cwd: "/tmp" },
      });

      await openSignedInChat(driver, seed, runner.id, chat.id);

      await test.step("the parked turn renders with no inline prompt", async () => {
        await expect
          .poll(() => driver.desktopRuntimeChatBubbles().then((bubbles) => bubbles.map((bubble) => bubble.text)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining(["parked turn with a pending approval"]));
        expect(await driver.desktopRuntimeApprovalPromptVisible()).toBe(false);
      });

      await test.step("the request sits in the queue endpoint instead", async () => {
        const queued = await serverDesktopRuntimeListChatApprovals(wsId, session);
        expect(queued).toEqual(
          expect.arrayContaining([expect.objectContaining({ id: approvalId, status: "pending" })])
        );
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-035"], "no approval-mode control on web and no persisted choice"),
  { tag: specTags(["RUN-035"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "nomode", "online");
    try {
      const chat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await serverDesktopRuntimeSendChatMessage(chat.id, "mode-less turn", session);

      await openSignedInChat(driver, seed, runner.id, chat.id);

      await test.step("the chat header carries no mode control", async () => {
        await expect
          .poll(() => driver.desktopRuntimeChatBubbles().then((bubbles) => bubbles.map((bubble) => bubble.text)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining(["mode-less turn"]));
        expect(await driver.desktopRuntimeApprovalModeVisible()).toBe(false);
      });

      await test.step("no per-runner mode choice is persisted", async () => {
        const keys = await driver.desktopRuntimeStorageKeys();
        expect(keys.local.filter((key) => key.startsWith("pidash:chat-approval-mode:"))).toEqual([]);
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-036"], "chat runs over the cloud transport with server-persisted transcripts"),
  { tag: specTags(["RUN-036"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverDesktopRuntimeWorkspaceId(seed.workspaceSlug);
    const runner = await plantRunner(seed, "cloudchat", "online");
    try {
      const chat = await serverDesktopRuntimeCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);

      await test.step("no native bridge is exposed to the page", async () => {
        expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
      });

      await driver.desktopRuntimeStartRequestSpy();
      try {
        await driver.desktopRuntimeOpenChat(seed.workspaceSlug, runner.id, chat.id);
        await expect
          .poll(() => driver.desktopRuntimeSpyUrls(), { timeout: 15_000 })
          .toEqual(expect.arrayContaining([expect.stringContaining("/api/runners/chat/")]));
      } finally {
        await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
      }

      await test.step("the sent turn persists server-side and survives a reload", async () => {
        await serverDesktopRuntimeSendChatMessage(chat.id, "persist this turn", session);
        const stored = await serverDesktopRuntimeChatMessages(chat.id, session);
        expect(stored).toEqual([expect.objectContaining({ role: "user", content: "persist this turn" })]);
        expect(await serverDesktopRuntimeListChatSessions(wsId, runner.id, session)).toHaveLength(1);
        await driver.desktopRuntimeOpenChat(seed.workspaceSlug, runner.id, chat.id);
        await expect
          .poll(() => driver.desktopRuntimeChatBubbles().then((bubbles) => bubbles.map((bubble) => bubble.text)), {
            timeout: 15_000,
          })
          .toEqual(expect.arrayContaining(["persist this turn"]));
      });
    } finally {
      await serverDesktopRuntimeCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-044"], "agent runtime renders nothing and its endpoints refuse web sessions"),
  { tag: specTags(["RUN-044"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.desktopRuntimeStartRequestSpy();
    try {
      await test.step("sign-in plus project open mounts no runtime banner", async () => {
        await driver.openProjectIssues(seed.workspaceSlug, seed.projectId);
        await expect
          .poll(() => driver.visibleIssueNames().then((names) => names.length), { timeout: 15_000 })
          .toBeGreaterThan(0);
        expect(await driver.desktopRuntimeRuntimeBannerVisible()).toBe(false);
        expect(await driver.desktopRuntimeIsTauriPresent()).toBe(false);
      });

      await test.step("browsing the runners area calls no runtime endpoint", async () => {
        await driver.desktopRuntimeOpenRunners(seed.workspaceSlug);
        const hits = (await driver.desktopRuntimeSpyUrls()).filter(
          (url) => url.includes("/ai-assistant/") || url.includes("/desktop-enroll/")
        );
        expect(hits).toEqual([]);
      });
    } finally {
      await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
    }

    await test.step("the runtime endpoints refuse the web session", async () => {
      await expect(serverDesktopRuntimeAgentProfileRefusal(session)).resolves.toEqual({
        status: 403,
        error: "desktop_session_required",
      });
      await expect(serverDesktopRuntimeAgentTokenRefusal(session)).resolves.toEqual({
        status: 403,
        error: "desktop_session_required",
      });
    });
  }
);

test(
  specTitle(["RUN-045"], "opening a project shows no enrollment banner and enrolls nothing"),
  { tag: specTags(["RUN-045"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    await driver.openEntry();
    await driver.signInWithPassword(seed.email, seed.password);
    await driver.desktopRuntimeStartRequestSpy();
    try {
      await openSignedInProject(driver, seed);
      await test.step("no status banner appears and no enrollment is posted", async () => {
        expect(await driver.desktopRuntimeRuntimeBannerVisible()).toBe(false);
        const enrolls = (await driver.desktopRuntimeSpyUrls()).filter((url) => url.includes("/desktop-enroll/"));
        expect(enrolls).toEqual([]);
      });
    } finally {
      await driver.desktopRuntimeStopRequestSpy().catch(() => undefined);
    }

    await test.step("the enroll endpoint refuses the web session", async () => {
      await expect(serverDesktopRuntimeDesktopEnrollRefusal("POST", session)).resolves.toEqual({
        status: 403,
        error: "desktop_session_required",
      });
    });
  }
);

test(
  specTitle(["RUN-047"], "sign-out performs no local teardown on web"),
  { tag: specTags(["RUN-047"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);

    await test.step("the teardown endpoint refuses the web session", async () => {
      await expect(serverDesktopRuntimeDesktopEnrollRefusal("DELETE", session)).resolves.toEqual({
        status: 403,
        error: "desktop_session_required",
      });
    });

    await openSignedInProject(driver, seed);
    await driver.desktopRuntimeStartRequestSpy();
    try {
      await test.step("sign-out signs out, posts no enrollment call, keeps no cache", async () => {
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
  }
);
