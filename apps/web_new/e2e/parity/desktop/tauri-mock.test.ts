// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The mocked on-machine chat store follows the native commands' contract
// (desktop/src-tauri/src/chat_history.rs): session ids never repeat, a turn
// bumps the conversation's activity, the list sorts by that activity, each
// conversation numbers its own events, and the counters survive a page load.
// DESK-009 turns on those rules: after a clear the leg must send into the
// auto-created conversation and find its turn on the reload.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { installDesktopMock } from "./tauri-mock";

type Invoke = (command: string, args?: unknown) => Promise<unknown>;

interface Installed {
  invoke: Invoke;
  store: Map<string, string>;
  mock: Window["__PIDASH_DESKTOP_MOCK__"];
}

async function install(persisted?: Map<string, string>): Promise<Installed> {
  let script: (seed: unknown) => void = () => undefined;
  let seed: unknown;
  const page = {
    addInitScript: async (fn: (seed: unknown) => void, arg: unknown) => {
      script = fn;
      seed = arg;
    },
  };
  await installDesktopMock(page as never);
  const store = persisted ?? new Map<string, string>();
  vi.stubGlobal("sessionStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => store.set(key, value),
  });
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    writable: true,
    value: { location: { origin: "https://app.test" } },
  });
  script(seed);
  const mock = (globalThis.window as unknown as Record<string, unknown>)[
    "__PIDASH_DESKTOP_MOCK__"
  ] as Installed["mock"];
  const tauri = (globalThis.window as unknown as Record<string, unknown>)["__TAURI__"] as {
    core: { invoke: Invoke };
  };
  return { invoke: tauri.core.invoke, store, mock };
}

const ACCOUNT = "me@parity.test";

let now = 1_000;
beforeEach(() => {
  now = 1_000;
  vi.spyOn(Date, "now").mockImplementation(() => now);
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  delete (globalThis as unknown as Record<string, unknown>)["window"];
});

function create(invoke: Invoke) {
  return invoke("chat_create_session", {
    account: ACCOUNT,
    session: { workspace: "acme", project: "PROJ" },
  }) as Promise<Record<string, unknown>>;
}

function append(invoke: Invoke, sessionId: string, content: string, role = "user") {
  return invoke("chat_append_event", { account: ACCOUNT, sessionId, event: { role, content } });
}

function list(invoke: Invoke) {
  return invoke("chat_list_sessions", { account: ACCOUNT }) as Promise<Array<Record<string, unknown>>>;
}

describe("mocked on-machine chat store", () => {
  it("never reuses a session id after a delete or a clear", async () => {
    const { invoke } = await install();
    const first = await create(invoke);
    expect(first["id"]).toBe("mock-chat-1");
    await invoke("chat_delete_session", { account: ACCOUNT, sessionId: first["id"] });
    const second = await create(invoke);
    expect(second["id"]).toBe("mock-chat-2");
    await invoke("chat_clear_history", { account: ACCOUNT });
    const third = await create(invoke);
    expect(third["id"]).toBe("mock-chat-3");
  });

  it("keeps the id counter across a page load", async () => {
    const lastRun = await install();
    await create(lastRun.invoke);
    await create(lastRun.invoke);
    await lastRun.invoke("chat_clear_history", { account: ACCOUNT });
    const reloaded = await install(lastRun.store);
    const created = await create(reloaded.invoke);
    expect(created["id"]).toBe("mock-chat-3");
  });

  it("a turn bumps the conversation's activity", async () => {
    const { invoke } = await install();
    const older = await create(invoke);
    now = 2_000;
    const newer = await create(invoke);
    now = 3_000;
    await append(invoke, older["id"] as string, "surviving turn");
    const rows = await list(invoke);
    expect(rows.map((row) => row["id"])).toEqual([older["id"], newer["id"]]);
    expect(rows[0]?.["updated_at"]).toBe(3_000);
  });

  it("orders by creation when no conversation took a turn", async () => {
    const { invoke } = await install();
    const older = await create(invoke);
    now = 2_000;
    const newer = await create(invoke);
    const rows = await list(invoke);
    expect(rows.map((row) => row["id"])).toEqual([newer["id"], older["id"]]);
  });

  it("numbers each conversation's events from one", async () => {
    const { invoke } = await install();
    const first = await create(invoke);
    const second = await create(invoke);
    const one = (await append(invoke, first["id"] as string, "one")) as Record<string, unknown>;
    const other = (await append(invoke, second["id"] as string, "other")) as Record<string, unknown>;
    const two = (await append(invoke, first["id"] as string, "two")) as Record<string, unknown>;
    expect([one["seq"], two["seq"]]).toEqual([1, 2]);
    expect(other["seq"]).toBe(1);
    const events = (await invoke("chat_list_events", { account: ACCOUNT, sessionId: first["id"] })) as Array<
      Record<string, unknown>
    >;
    expect(events.map((event) => [event["seq"], event["content"]])).toEqual([
      [1, "one"],
      [2, "two"],
    ]);
  });

  it("refuses a turn for a conversation the store does not hold", async () => {
    const { invoke } = await install();
    await expect(append(invoke, "mock-chat-gone", "lost")).rejects.toThrow("no such session");
  });

  it("thread-id writes move the conversation's activity too", async () => {
    const { invoke } = await install();
    const created = await create(invoke);
    now = 5_000;
    await invoke("chat_set_thread_id", { account: ACCOUNT, sessionId: created["id"], engineThreadId: "thread-1" });
    const row = (await invoke("chat_get_session", { account: ACCOUNT, sessionId: created["id"] })) as Record<
      string,
      unknown
    >;
    expect(row["engine_thread_id"]).toBe("thread-1");
    expect(row["updated_at"]).toBe(5_000);
  });

  it("lists only the account's conversations", async () => {
    const { invoke } = await install();
    await create(invoke);
    await invoke("chat_create_session", { account: "someone@parity.test", session: {} });
    const rows = await list(invoke);
    expect(rows.map((row) => row["id"])).toEqual(["mock-chat-1"]);
  });

  it("reads a store written before the id counter existed as empty", async () => {
    const store = new Map<string, string>([
      [
        "pidash-mock-desktop-chat",
        JSON.stringify({
          sessions: [["me@parity.test:mock-chat-9", { id: "mock-chat-9", updated_at: 1 }]],
          events: [],
          seq: 4,
          modes: {},
        }),
      ],
    ]);
    const { invoke } = await install(store);
    const rows = await list(invoke);
    expect(rows.map((row) => row["id"])).toEqual(["mock-chat-9"]);
    const created = await create(invoke);
    expect(created["id"]).toBe("mock-chat-1");
  });

  it("reads an unparsable store as empty", async () => {
    const store = new Map<string, string>([["pidash-mock-desktop-chat", "{not json"]]);
    const { invoke } = await install(store);
    expect(await list(invoke)).toEqual([]);
    expect((await create(invoke))["id"]).toBe("mock-chat-1");
  });
});
