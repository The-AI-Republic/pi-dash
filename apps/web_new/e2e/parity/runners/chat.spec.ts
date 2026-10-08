// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Oracle scenarios (NEWFRONT-181): runner chat on cloud/web — open a chat
// from a contact, session history, compose and send, warm-up, streaming
// plus activity strip, stop, close, and header/composer gating.
// Rows: RUN-025 (open/contact), RUN-026 (history), RUN-027 (compose),
// RUN-028 (warm-up), RUN-029 (streaming), RUN-030 (stop), RUN-031
// (close), RUN-032 (header/gating).
//
// Fixtures: runners are planted through the Django shell (the web API
// offers no create-runner endpoint): online/busy runners get a live
// RunnerSession row so the outbox treats them as connected, chat
// sessions and messages go through the app's own REST endpoints, and
// the SSE stream is stubbed per session — the scratch stack runs no
// live runner daemon, so token streaming is proven at the transport
// contract while warm/close/cancel run against the real endpoints.
import { randomUUID } from "node:crypto";
import { test, expect } from "../fixtures";
import type { ParityDriver, ParitySeedFacts } from "../drivers/parity-driver";
import {
  serverCreateRunner,
  serverCleanupRunner,
  serverSetRunnerStatus,
  serverWorkspaceIdBySlug,
  serverRunner,
  serverCreateChatSession,
  serverListChatSessions,
  serverGetChatSession,
  serverChatMessages,
  serverSendChatMessage,
  serverCloseChatSession,
  serverSetChatSession,
  serverChatEventKinds,
  serverReadChatStream,
  signInSession,
  type RunnerChatRunner,
  type RunnerChatStatus,
} from "../helpers/api";
import { specTags, specTitle } from "../helpers/tags";

let runnerSerial = 0;

function runnerTag(prefix: string): string {
  runnerSerial += 1;
  return `nc181 ${prefix} ${Date.now()} ${runnerSerial}`;
}

async function openSignedInChat(
  driver: ParityDriver,
  seed: ParitySeedFacts,
  runnerId: string,
  sessionId?: string
): Promise<void> {
  await driver.openEntry();
  await driver.signInWithPassword(seed.email, seed.password);
  await driver.runnerChatOpen(seed.workspaceSlug, runnerId, sessionId);
}

async function plantRunner(seed: ParitySeedFacts, prefix: string, status: RunnerChatStatus): Promise<RunnerChatRunner> {
  return serverCreateRunner({
    ownerEmail: seed.email,
    workspaceSlug: seed.workspaceSlug,
    projectId: seed.projectId,
    name: runnerTag(prefix),
    status,
  });
}

async function historyTitles(driver: ParityDriver): Promise<{ subtitle: string; active: boolean }[]> {
  return (await driver.runnerChatHistoryItems()).map((item) => ({ subtitle: item.subtitle, active: item.active }));
}

/**
 * Create a session that stays usable for multi-session fixtures. The
 * create endpoint reuses an open message-less session, so the session is
 * given a message (making the next create fresh) and its parked turn is
 * cleared again so the composer sees an idle session.
 */
async function createMessagedSession(
  workspaceId: string,
  runnerId: string,
  sessionCookie: string,
  label: string
): Promise<string> {
  const chat = await serverCreateChatSession({ workspaceId, runnerId }, sessionCookie);
  await serverSendChatMessage(chat.id, label, sessionCookie);
  await serverSetChatSession(chat.id, { activeMessageId: null, activeTurnId: "" });
  return chat.id;
}

test(
  specTitle(["RUN-025"], "runner chat opens from a contact with a deep-linkable URL"),
  { tag: specTags(["RUN-025"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const online = await plantRunner(seed, "chatty", "online");
    const offline = await plantRunner(seed, "quiet", "offline");
    try {
      await test.step("chat opens at the runner URL with live status dots", async () => {
        await openSignedInChat(driver, seed, online.id);
        await expect
          .poll(() => driver.runnerChatHeader(), { timeout: 15_000 })
          .toMatchObject({
            name: online.name,
            badge: "online",
          });
        expect(await driver.currentPath()).toContain(`/runners/chat/${online.id}`);
        const contacts = await driver.runnerChatContactNames();
        expect(contacts).toContain(online.name);
        expect(contacts).toContain(offline.name);
        expect(await driver.runnerChatContactDotClass(online.name)).not.toBe(
          await driver.runnerChatContactDotClass(offline.name)
        );
      });

      await test.step("the server stored the planted runners", async () => {
        expect((await serverRunner(online.id, session)).status).toBe("online");
        expect((await serverRunner(offline.id, session)).status).toBe("offline");
      });

      await test.step("switching contacts resets chat state to the new runner", async () => {
        const first = await serverCreateChatSession({ workspaceId: wsId, runnerId: online.id }, session);
        await serverSendChatMessage(first.id, "stay on the first runner", session);
        await driver.runnerChatOpen(seed.workspaceSlug, online.id, first.id);
        await expect
          .poll(() => driver.runnerChatMessageBubbles(), { timeout: 15_000 })
          .toEqual([{ role: "user", text: "stay on the first runner" }]);
        await driver.runnerChatOpenContact(offline.name);
        await expect
          .poll(() => driver.runnerChatHeader(), { timeout: 15_000 })
          .toMatchObject({
            name: offline.name,
          });
        expect(await driver.currentPath()).toContain(`/runners/chat/${offline.id}`);
        expect(await driver.runnerChatMessageBubbles()).toEqual([]);
        expect(await driver.runnerChatHistoryEmptyVisible()).toBe(true);
        expect(await driver.runnerChatActivityStrip()).toEqual([]);
      });
    } finally {
      await serverCleanupRunner(online.id);
      await serverCleanupRunner(offline.id);
    }
  }
);

test(
  specTitle(["RUN-025"], "unknown runner shows the unavailable state without crashing"),
  { tag: specTags(["RUN-025"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const unknownId = "00000000-0000-0000-0000-000000000000";
    await openSignedInChat(driver, seed, unknownId);

    await test.step("header falls back and the composer stays loading", async () => {
      expect(await driver.runnerChatHeader()).toMatchObject({ name: "Runner", badge: "" });
      expect(await driver.runnerChatHistoryEmptyVisible()).toBe(true);
      expect(await driver.runnerChatComposerReason()).toBe("Loading");
    });

    await test.step("the server knows no such runner", async () => {
      await expect(serverRunner(unknownId, session)).rejects.toThrow();
    });
  }
);

test(
  specTitle(["RUN-026"], "history lists newest-first and auto-selects the most recent open session"),
  { tag: specTags(["RUN-026"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "history", "online");
    try {
      const oldest = await createMessagedSession(wsId, runner.id, session, "oldest turn");
      await serverSetChatSession(oldest, { lastMessageAtIso: "2024-01-10T10:00:00Z" });
      const middle = await createMessagedSession(wsId, runner.id, session, "middle turn");
      await serverSetChatSession(middle, { lastMessageAtIso: "2024-06-10T10:00:00Z" });
      const newest = await createMessagedSession(wsId, runner.id, session, "newest turn");
      await serverCloseChatSession(newest, session);
      await serverSetChatSession(newest, { lastMessageAtIso: "2024-12-10T10:00:00Z" });

      await openSignedInChat(driver, seed, runner.id);

      await test.step("newest-first with a closed suffix, most recent open active", async () => {
        await expect.poll(() => driver.runnerChatHistoryItems(), { timeout: 15_000 }).toHaveLength(3);
        const items = await historyTitles(driver);
        expect(items[0]?.subtitle).toContain("closed");
        expect(items.map((item) => item.active)).toEqual([false, true, false]);
        // Auto-select does not rewrite the URL with an explicit selection.
        expect(await driver.currentPath()).not.toContain("sessionId=");
        expect(await driver.runnerChatComposerReason()).toBeNull();
      });

      await test.step("the server still holds the three sessions", async () => {
        expect(await serverListChatSessions(wsId, runner.id, session)).toHaveLength(3);
      });
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-026"], "explicit selection wins and stale ids fall through"),
  { tag: specTags(["RUN-026"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "select", "online");
    try {
      const oldest = await createMessagedSession(wsId, runner.id, session, "oldest turn");
      await serverSetChatSession(oldest, { lastMessageAtIso: "2024-01-10T10:00:00Z" });
      const middle = await createMessagedSession(wsId, runner.id, session, "middle turn");
      await serverSetChatSession(middle, { lastMessageAtIso: "2024-06-10T10:00:00Z" });
      const newest = await createMessagedSession(wsId, runner.id, session, "newest turn");
      await serverCloseChatSession(newest, session);
      await serverSetChatSession(newest, { lastMessageAtIso: "2024-12-10T10:00:00Z" });

      await openSignedInChat(driver, seed, runner.id, newest);

      await test.step("an explicit deep link wins, even for a closed session", async () => {
        await expect
          .poll(() => historyTitles(driver), { timeout: 15_000 })
          .toEqual([
            { subtitle: expect.stringContaining("closed"), active: true },
            { subtitle: expect.any(String), active: false },
            { subtitle: expect.any(String), active: false },
          ]);
        expect(await driver.runnerChatComposerReason()).toBe("Session closed");
      });

      await test.step("clicking another entry reselects and rewrites the URL", async () => {
        await driver.runnerChatClickHistoryItem(2);
        await expect.poll(() => driver.currentPath(), { timeout: 15_000 }).toContain(`sessionId=${oldest}`);
        expect((await historyTitles(driver)).map((item) => item.active)).toEqual([false, false, true]);
        expect(await driver.runnerChatComposerReason()).toBeNull();
      });

      await test.step("a stale session id falls through to auto-select", async () => {
        await driver.runnerChatOpen(seed.workspaceSlug, runner.id, "00000000-0000-0000-0000-000000000001");
        await expect
          .poll(() => historyTitles(driver).then((items) => items.map((item) => item.active)), {
            timeout: 15_000,
          })
          .toEqual([false, true, false]);
      });

      await test.step("viewing history created no sessions", async () => {
        expect(await serverListChatSessions(wsId, runner.id, session)).toHaveLength(3);
      });
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-026"], "new chat shows working and error states"),
  { tag: specTags(["RUN-026"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "newchat", "online");
    try {
      // Messaged, so the UI's New-chat control creates a fresh session
      // instead of reusing this one.
      await createMessagedSession(wsId, runner.id, session, "first turn");
      await openSignedInChat(driver, seed, runner.id);
      await expect.poll(() => driver.runnerChatHistoryItems(), { timeout: 15_000 }).toHaveLength(1);

      await test.step("creating shows a working state, then selects the new session", async () => {
        await driver.runnerChatDelaySessionCreate(2500);
        const done = driver.runnerChatNewChat();
        await expect.poll(() => driver.runnerChatNewChatDisabled(), { timeout: 10_000 }).toBe(true);
        await done;
        await expect.poll(() => driver.runnerChatHistoryItems(), { timeout: 15_000 }).toHaveLength(2);
        expect(await driver.currentPath()).toContain("sessionId=");
        expect((await historyTitles(driver))[0]?.active).toBe(true);
        expect(await serverListChatSessions(wsId, runner.id, session)).toHaveLength(2);
        await driver.runnerChatClearSessionCreateStubs();
      });

      await test.step("a failed create toasts and keeps history unchanged", async () => {
        await driver.runnerChatFailSessionCreate();
        await driver.runnerChatNewChat();
        await expect.poll(() => driver.runnerChatLastToast(), { timeout: 15_000 }).not.toBeNull();
        expect((await driver.runnerChatLastToast())?.title).not.toBe("");
        expect(await driver.runnerChatHistoryItems()).toHaveLength(2);
        expect(await serverListChatSessions(wsId, runner.id, session)).toHaveLength(2);
        await driver.runnerChatClearSessionCreateStubs();
      });
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-027"], "enter sends, shift+enter breaks the line, empty drafts gate send"),
  { tag: specTags(["RUN-027"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "compose", "online");
    try {
      await serverCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await openSignedInChat(driver, seed, runner.id);
      await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 15_000 }).toBeNull();

      await test.step("empty and blank drafts disable send", async () => {
        await driver.runnerChatFillDraft("");
        expect(await driver.runnerChatSendEnabled()).toBe(false);
        await driver.runnerChatFillDraft("   ");
        expect(await driver.runnerChatSendEnabled()).toBe(false);
      });

      await test.step("shift+enter inserts a newline without sending", async () => {
        await driver.runnerChatFillDraft("line one");
        expect(await driver.runnerChatSendEnabled()).toBe(true);
        await driver.runnerChatPressShiftEnter();
        expect(await driver.runnerChatDraftValue()).toBe("line one\n");
        const first = (await serverListChatSessions(wsId, runner.id, session))[0];
        expect(await serverChatMessages(first!.id, session)).toHaveLength(0);
      });

      await test.step("enter sends and the composer gates mid-turn", async () => {
        await driver.runnerChatPressEnter();
        await expect
          .poll(() => driver.runnerChatMessageBubbles(), { timeout: 15_000 })
          .toEqual([{ role: "user", text: "line one" }]);
        expect(await driver.runnerChatDraftValue()).toBe("");
        const first = (await serverListChatSessions(wsId, runner.id, session))[0];
        const messages = await serverChatMessages(first!.id, session);
        expect(messages).toHaveLength(1);
        expect(messages[0]).toMatchObject({ role: "user", content: "line one" });
        expect(await driver.runnerChatComposerReason()).toBe("Response in progress");
        expect(await driver.runnerChatTextareaDisabled()).toBe(true);
        expect(await driver.runnerChatStopVisible()).toBe(true);
      });
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-027"], "failed send restores the draft with banner and toast"),
  { tag: specTags(["RUN-027"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "sendfail", "online");
    try {
      const chat = await serverCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await openSignedInChat(driver, seed, runner.id, chat.id);
      await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 15_000 }).toBeNull();

      await driver.runnerChatFailNextSend();
      await driver.runnerChatFillDraft("doomed draft");
      await driver.runnerChatPressEnter();

      await test.step("draft restored with an inline banner carrying the reason", async () => {
        await expect.poll(() => driver.runnerChatDraftValue(), { timeout: 15_000 }).toBe("doomed draft");
        await expect.poll(() => driver.runnerChatAlertText(), { timeout: 15_000 }).toContain("parity send failure");
      });

      await test.step("a toast carries the same failure", async () => {
        await expect.poll(() => driver.runnerChatLastToast(), { timeout: 15_000 }).not.toBeNull();
        expect((await driver.runnerChatLastToast())?.message).toContain("parity send failure");
      });

      await test.step("the server stored no message", async () => {
        expect(await serverChatMessages(chat.id, session)).toHaveLength(0);
        await driver.runnerChatClearSendFailure();
      });
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-027"], "voice control never submits the draft"),
  { tag: specTags(["RUN-027"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "voice", "online");
    try {
      await serverCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await openSignedInChat(driver, seed, runner.id);
      await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 15_000 }).toBeNull();

      const label = await driver.runnerChatVoiceButtonLabel();
      if (label === null) {
        expect(label).toBeNull();
        return;
      }

      await driver.runnerChatFillDraft("voice draft stays put");
      await driver.runnerChatClickVoiceButton();
      // Unconfigured dictation routes to settings; a configured one
      // records without sending. Either way no message is created.
      let navigated = false;
      try {
        await expect.poll(() => driver.currentPath(), { timeout: 5_000 }).toContain("settings");
        navigated = true;
      } catch {
        navigated = false;
      }
      const chats = await serverListChatSessions(wsId, runner.id, session);
      expect(chats).toHaveLength(1);
      expect(await serverChatMessages(chats[0]!.id, session)).toHaveLength(0);
      if (!navigated) {
        expect(await driver.runnerChatDraftValue()).toBe("voice draft stays put");
      }
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-028"], "warm-up runs once for online and busy runners"),
  { tag: specTags(["RUN-028"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const online = await plantRunner(seed, "warmon", "online");
    const busy = await plantRunner(seed, "warmbusy", "busy");
    try {
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.runnerChatStartApiSpy();
      await driver.runnerChatOpen(seed.workspaceSlug, online.id);

      await test.step("online runners warm exactly once and gain a session", async () => {
        await expect
          .poll(() => driver.runnerChatApiCounts(), { timeout: 20_000 })
          .toMatchObject({ warm: 1, sessionCreate: 1 });
        // Slow server reads prove the counts stay put (de-duplicated).
        expect(await serverListChatSessions(wsId, online.id, session)).toHaveLength(1);
        expect(await driver.runnerChatHistoryItems()).toHaveLength(1);
        expect(await driver.runnerChatApiCounts()).toMatchObject({ warm: 1, sessionCreate: 1 });
      });

      await test.step("busy runners still serve chat", async () => {
        const before = await driver.runnerChatApiCounts();
        await driver.runnerChatOpen(seed.workspaceSlug, busy.id);
        await expect
          .poll(() => driver.runnerChatApiCounts(), { timeout: 20_000 })
          .toMatchObject({ warm: before.warm + 1, sessionCreate: before.sessionCreate + 1 });
        expect(await serverListChatSessions(wsId, busy.id, session)).toHaveLength(1);
        expect(await driver.runnerChatHistoryItems()).toHaveLength(1);
        expect(await driver.runnerChatApiCounts()).toMatchObject({
          warm: before.warm + 1,
          sessionCreate: before.sessionCreate + 1,
        });
      });
    } finally {
      await driver.runnerChatStopApiSpy().catch(() => undefined);
      await serverCleanupRunner(online.id);
      await serverCleanupRunner(busy.id);
    }
  }
);

test(
  specTitle(["RUN-028"], "warm-up skips offline and revoked runners and history views"),
  { tag: specTags(["RUN-028"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const offline = await plantRunner(seed, "warmoff", "online");
    const revoked = await plantRunner(seed, "warmrev", "revoked");
    try {
      // Sessions cannot be created on an offline runner (409), so the
      // history fixture is built while online, then the runner is
      // flipped offline for the view.
      const closed = await serverCreateChatSession({ workspaceId: wsId, runnerId: offline.id }, session);
      await serverCloseChatSession(closed.id, session);
      await serverSetRunnerStatus(offline.id, "offline");

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.runnerChatStartApiSpy();

      await test.step("viewing history on an offline runner warms and creates nothing", async () => {
        await driver.runnerChatOpen(seed.workspaceSlug, offline.id, closed.id);
        await expect
          .poll(() => driver.runnerChatApiCounts().then((counts) => counts.sessionList), { timeout: 15_000 })
          .toBeGreaterThanOrEqual(1);
        expect(await driver.runnerChatHistoryItems()).toHaveLength(1);
        expect(await serverListChatSessions(wsId, offline.id, session)).toHaveLength(1);
        expect(await driver.runnerChatApiCounts()).toMatchObject({ warm: 0, sessionCreate: 0 });
      });

      await test.step("revoked runners are skipped with an empty history", async () => {
        const before = await driver.runnerChatApiCounts();
        await driver.runnerChatOpen(seed.workspaceSlug, revoked.id);
        await expect
          .poll(() => driver.runnerChatApiCounts().then((counts) => counts.sessionList), { timeout: 15_000 })
          .toBeGreaterThan(before.sessionList);
        expect(await driver.runnerChatHistoryEmptyVisible()).toBe(true);
        expect(await serverListChatSessions(wsId, revoked.id, session)).toHaveLength(0);
        expect(await driver.runnerChatApiCounts()).toMatchObject({
          warm: before.warm,
          sessionCreate: before.sessionCreate,
        });
      });
    } finally {
      await driver.runnerChatStopApiSpy().catch(() => undefined);
      await serverCleanupRunner(offline.id);
      await serverCleanupRunner(revoked.id);
    }
  }
);

test(
  specTitle(["RUN-029"], "assistant reply streams token-by-token with markdown"),
  { tag: specTags(["RUN-029"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "stream", "online");
    try {
      const chat = await serverCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await serverSendChatMessage(chat.id, "stubbed turn", session);
      const bubbleId = randomUUID();
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      // Installed before the page subscribes so the stub (not the live
      // server) owns the stream; the asserted frames are re-stubbed fresh
      // below, since each serve is wiped by the reconnect-error refetch.
      await driver.runnerChatStubStream(chat.id, [
        { seq: 1, kind: "assistant_delta", payload: { params: { delta: "priming" } }, message: bubbleId },
      ]);
      await driver.runnerChatOpen(seed.workspaceSlug, runner.id, chat.id);

      await test.step("tokens accumulate into one markdown bubble, never blank", async () => {
        // Settle on the user turn first (initial load done), then hold the
        // reconnect refetch and serve fresh frames: the streamed bubble
        // stays on screen instead of collapsing back to the server state.
        await expect
          .poll(() => driver.runnerChatMessageBubbles(), { timeout: 20_000 })
          .toEqual([{ role: "user", text: "stubbed turn" }]);
        await driver.runnerChatHoldMessageList(30_000);
        try {
          await driver.runnerChatStubStream(chat.id, [
            { seq: 101, kind: "assistant_delta", payload: { params: { delta: "Hello " } }, message: bubbleId },
            // Same sequence twice: deltas are de-duplicated, applied once.
            { seq: 101, kind: "assistant_delta", payload: { params: { delta: "Hello " } }, message: bubbleId },
            { seq: 102, kind: "assistant_delta", payload: { params: { delta: "**world**" } }, message: bubbleId },
          ]);
          await expect
            .poll(() => driver.runnerChatMessageBubbles(), { timeout: 20_000 })
            .toEqual([
              { role: "user", text: "stubbed turn" },
              { role: "assistant", text: "Hello world" },
            ]);
          for (const bubble of await driver.runnerChatMessageBubbles()) {
            if (bubble.role === "assistant") expect(bubble.text).not.toBe("");
          }
          expect(await driver.runnerChatAssistantBubbleHtml(0)).toContain("<strong>");
          // The bubble came from the stream: the server holds no assistant
          // message for this session.
          expect(await serverChatMessages(chat.id, session)).toHaveLength(1);
        } finally {
          await driver.runnerChatReleaseMessageList();
        }
      });

      await test.step("the stream subscribes from the start on the bare events URL", async () => {
        await expect.poll(() => driver.runnerChatStreamRequestUrls(chat.id), { timeout: 15_000 }).not.toHaveLength(0);
        const urls = await driver.runnerChatStreamRequestUrls(chat.id);
        // A fresh subscribe carries no `after` cursor (the transport only
        // sends one when resuming past persisted frames); the server-side
        // step below proves the resumable replay itself.
        expect(urls[0]).toContain(`/chat/sessions/${chat.id}/events/`);
        expect(urls[0]).not.toContain("after=");
      });

      await test.step("the server replays persisted frames by sequence", async () => {
        const head = await serverReadChatStream(chat.id, session, 0, { maxChars: 200 });
        expect(head).toContain("chat_timing");
        const events = await serverChatEventKinds(chat.id);
        const tip = events.reduce((max, event) => Math.max(max, event.seq), 0);
        expect(await serverReadChatStream(chat.id, session, tip, { waitMs: 4000 })).toBe("");
        await driver.runnerChatClearStreamStub(chat.id);
      });
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-029"], "activity strip caps to the last few and terminal frames reconcile"),
  { tag: specTags(["RUN-029"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "strip", "online");
    try {
      const chat = await serverCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      // One user turn mounts the message list (and with it the strip);
      // with zero messages the page renders the empty state instead.
      await serverSendChatMessage(chat.id, "strip turn", session);
      const commands = Array.from({ length: 8 }, (_, i) => ({
        seq: i + 1,
        kind: "raw",
        payload: {
          method: "item/completed",
          params: { item: { type: "commandExecution", command: `c${i + 1}` } },
        },
      }));
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.runnerChatStartApiSpy();
      await driver.runnerChatStubStream(chat.id, [
        ...commands,
        // Lifecycle narration and transcript rows never reach the strip.
        { seq: 9, kind: "turn_started", payload: {} },
        {
          seq: 10,
          kind: "raw",
          payload: { method: "item/started", params: { item: { type: "agentMessage" } } },
        },
      ]);
      await driver.runnerChatOpen(seed.workspaceSlug, runner.id, chat.id);

      await test.step("only the last few actions show", async () => {
        await expect
          .poll(() => driver.runnerChatActivityStrip(), { timeout: 20_000 })
          .toEqual(["Ran: c3", "Ran: c4", "Ran: c5", "Ran: c6", "Ran: c7", "Ran: c8"]);
      });

      await test.step("a terminal frame reconciles with saved history", async () => {
        const before = (await driver.runnerChatApiCounts()).messageList;
        await driver.runnerChatStubStream(chat.id, [{ seq: 11, kind: "turn_completed", payload: {} }]);
        await expect
          .poll(() => driver.runnerChatApiCounts().then((counts) => counts.messageList), { timeout: 30_000 })
          .toBeGreaterThan(before);
        // Terminal frames reconcile; they never join the strip.
        expect(await driver.runnerChatActivityStrip()).toHaveLength(6);
        expect(await serverChatMessages(chat.id, session)).toHaveLength(1);
        await driver.runnerChatClearStreamStub(chat.id);
        await driver.runnerChatStopApiSpy().catch(() => undefined);
      });
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-029"], "transient stream errors show a dismissible banner cleared by frames"),
  { tag: specTags(["RUN-029"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "strerr", "online");
    try {
      const chat = await serverCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.runnerChatStubStream(chat.id, [{ seq: 1, kind: "raw-invalid", payload: {} }]);
      await driver.runnerChatOpen(seed.workspaceSlug, runner.id, chat.id);

      await test.step("a bad frame raises the banner; the next frame clears it", async () => {
        await expect.poll(() => driver.runnerChatAlertText(), { timeout: 20_000 }).not.toBeNull();
        // Hold the reconnect refetch so the recovered bubble stays put
        // instead of collapsing back to the (empty) server state.
        await driver.runnerChatHoldMessageList(30_000);
        try {
          await driver.runnerChatStubStream(chat.id, [
            { seq: 2, kind: "assistant_delta", payload: { params: { delta: "recovered" } }, message: randomUUID() },
          ]);
          await expect.poll(() => driver.runnerChatAlertText(), { timeout: 30_000 }).toBeNull();
          await expect
            .poll(() => driver.runnerChatMessageBubbles(), { timeout: 15_000 })
            .toEqual([{ role: "assistant", text: "recovered" }]);
        } finally {
          await driver.runnerChatReleaseMessageList();
        }
      });

      await test.step("the banner also dismisses by hand", async () => {
        await driver.runnerChatStubStream(chat.id, [{ seq: 3, kind: "raw-invalid", payload: {} }]);
        await expect.poll(() => driver.runnerChatAlertText(), { timeout: 30_000 }).not.toBeNull();
        await driver.runnerChatDismissAlert();
        await expect.poll(() => driver.runnerChatAlertText(), { timeout: 15_000 }).toBeNull();
        expect((await driver.runnerChatStreamRequestUrls(chat.id)).length).toBeGreaterThanOrEqual(2);
        await driver.runnerChatClearStreamStub(chat.id);
      });
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-030"], "stop cancels the in-flight turn and refetches sessions"),
  { tag: specTags(["RUN-030"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "stopper", "online");
    try {
      const chat = await serverCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await serverSendChatMessage(chat.id, "interrupt me", session);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.runnerChatStartApiSpy();
      await driver.runnerChatOpen(seed.workspaceSlug, runner.id, chat.id);

      await test.step("a mid-turn session offers stop and no inline prompt", async () => {
        await expect.poll(() => driver.runnerChatStopVisible(), { timeout: 15_000 }).toBe(true);
        expect(await driver.runnerChatApprovalPromptVisible()).toBe(false);
      });

      await test.step("stop cancels and refetches", async () => {
        const before = (await driver.runnerChatApiCounts()).sessionList;
        await driver.runnerChatClickStop();
        await expect
          .poll(() => driver.runnerChatApiCounts().then((counts) => counts.cancel), { timeout: 15_000 })
          .toBe(1);
        await expect
          .poll(() => driver.runnerChatApiCounts().then((counts) => counts.sessionList), { timeout: 15_000 })
          .toBeGreaterThan(before);
      });

      await test.step("the user turn is stored; cancel is fire-and-forget", async () => {
        const messages = await serverChatMessages(chat.id, session);
        expect(messages).toHaveLength(1);
        expect(messages[0]).toMatchObject({ role: "user", content: "interrupt me" });
        // No daemon answers on the scratch stack, so the turn stays
        // parked server-side; the cancel went out regardless.
        expect((await serverGetChatSession(chat.id, session)).active_message_id).not.toBeNull();
        await driver.runnerChatStopApiSpy().catch(() => undefined);
      });
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-031"], "close moves the session to closed and gates the composer"),
  { tag: specTags(["RUN-031"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "closer", "online");
    try {
      const chat = await serverCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.runnerChatStartApiSpy();
      // Explicit deep link: closing the auto-selected session makes the
      // warm effect backfill a fresh session, while an explicitly viewed
      // session stays put once closed.
      await driver.runnerChatOpen(seed.workspaceSlug, runner.id, chat.id);
      await expect.poll(() => driver.runnerChatHistoryItems(), { timeout: 15_000 }).toHaveLength(1);

      const before = (await driver.runnerChatApiCounts()).sessionList;
      await driver.runnerChatClickClose();

      await test.step("the session closes server-side and history refetches", async () => {
        await expect
          .poll(async () => serverGetChatSession(chat.id, session).then((s) => s.status), {
            timeout: 15_000,
          })
          .toBe("closed");
        expect(await serverListChatSessions(wsId, runner.id, session)).toHaveLength(1);
        await expect
          .poll(() => driver.runnerChatApiCounts().then((counts) => counts.sessionList), { timeout: 15_000 })
          .toBeGreaterThan(before);
      });

      await test.step("composer disabled and history labelled", async () => {
        await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 15_000 }).toBe("Session closed");
        expect(await driver.runnerChatTextareaDisabled()).toBe(true);
        await expect
          .poll(() => historyTitles(driver).then((items) => items[0]?.subtitle ?? ""), { timeout: 15_000 })
          .toContain("closed");
        await driver.runnerChatStopApiSpy().catch(() => undefined);
      });
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);

test(
  specTitle(["RUN-032"], "header shows runner identity and every blocked reason gates input"),
  { tag: specTags(["RUN-032"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const online = await plantRunner(seed, "gate-on", "online");
    const offline = await plantRunner(seed, "gate-off", "offline");
    const revoked = await plantRunner(seed, "gate-rev", "revoked");
    const busy = await plantRunner(seed, "gate-busy", "busy");
    try {
      // Each session is messaged in turn so the next create is fresh
      // (the endpoint reuses an open message-less session); the idle
      // one has its parked turn cleared again afterwards.
      const idle = await createMessagedSession(wsId, online.id, session, "idle turn");
      const midTurn = await serverCreateChatSession({ workspaceId: wsId, runnerId: online.id }, session);
      await serverSendChatMessage(midTurn.id, "parked turn", session);
      const shut = await serverCreateChatSession({ workspaceId: wsId, runnerId: online.id }, session);
      await serverCloseChatSession(shut.id, session);
      const busyIdle = await serverCreateChatSession({ workspaceId: wsId, runnerId: busy.id }, session);

      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);

      await test.step("online header names the runner with its badge", async () => {
        await driver.runnerChatOpen(seed.workspaceSlug, online.id, idle);
        await expect
          .poll(() => driver.runnerChatHeader(), { timeout: 15_000 })
          .toMatchObject({
            name: online.name,
            badge: "online",
          });
        expect((await driver.runnerChatHeader()).secondary).not.toBe("");
        expect(await driver.runnerChatComposerReason()).toBeNull();
        await driver.runnerChatFillDraft("ready");
        expect(await driver.runnerChatSendEnabled()).toBe(true);
      });

      await test.step("offline and revoked runners block with their reasons", async () => {
        await driver.runnerChatOpen(seed.workspaceSlug, offline.id);
        await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 15_000 }).toBe("Runner offline");
        expect(await driver.runnerChatTextareaDisabled()).toBe(true);
        expect((await driver.runnerChatHeader()).badge).toBe("offline");
        await driver.runnerChatOpen(seed.workspaceSlug, revoked.id);
        await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 15_000 }).toBe("Runner revoked");
        expect((await driver.runnerChatHeader()).badge).toBe("revoked");
      });

      await test.step("closed sessions and in-progress turns block", async () => {
        await driver.runnerChatOpen(seed.workspaceSlug, online.id, shut.id);
        await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 15_000 }).toBe("Session closed");
        await driver.runnerChatOpen(seed.workspaceSlug, online.id, midTurn.id);
        await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 15_000 }).toBe("Response in progress");
        expect(await driver.runnerChatStopVisible()).toBe(true);
      });

      await test.step("a busy runner does not block chat", async () => {
        await driver.runnerChatOpen(seed.workspaceSlug, busy.id, busyIdle.id);
        await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 15_000 }).toBeNull();
        await driver.runnerChatFillDraft("busy but chatting");
        expect(await driver.runnerChatSendEnabled()).toBe(true);
        expect((await serverRunner(online.id, session)).status).toBe("online");
        expect((await serverRunner(busy.id, session)).status).toBe("busy");
      });
    } finally {
      await serverCleanupRunner(online.id);
      await serverCleanupRunner(offline.id);
      await serverCleanupRunner(revoked.id);
      await serverCleanupRunner(busy.id);
    }
  }
);

test(
  specTitle(["RUN-032"], "loading state gates the composer until the runner resolves"),
  { tag: specTags(["RUN-032"]) },
  async ({ driver, seed }) => {
    const session = await signInSession(seed.email, seed.password);
    const wsId = await serverWorkspaceIdBySlug(seed.workspaceSlug);
    const runner = await plantRunner(seed, "gateload", "online");
    try {
      await serverCreateChatSession({ workspaceId: wsId, runnerId: runner.id }, session);
      await driver.openEntry();
      await driver.signInWithPassword(seed.email, seed.password);
      await driver.runnerChatDelayRunnerDetail(runner.id, 8000);
      await driver.runnerChatOpen(seed.workspaceSlug, runner.id);

      await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 15_000 }).toBe("Loading");
      await expect.poll(() => driver.runnerChatComposerReason(), { timeout: 20_000 }).toBeNull();
      expect(await driver.runnerChatHeader()).toMatchObject({ name: runner.name, badge: "online" });
      expect((await serverRunner(runner.id, session)).status).toBe("online");
      await driver.runnerChatClearRunnerDetailDelay();
    } finally {
      await serverCleanupRunner(runner.id);
    }
  }
);
