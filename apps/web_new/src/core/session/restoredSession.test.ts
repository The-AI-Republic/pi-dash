// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// PIDESKAPP-194: a desktop launch restores the persisted query cache before
// the router starts, and a 401 mid-session only flips the session store. A
// remembered user is no live session either way: the route gates must ask
// the server, so an expired session reaches sign-in instead of bouncing
// straight back into the stale app, and the expired account's rows must
// not paint for the next account or restore on the next launch.
import { ApiError, type Me, type Profile } from "@pidash/api-client";
import type { QueryClient } from "@tanstack/react-query";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { KeyValueStore } from "../platform/types.js";
import { createCachePersistor, QUERY_CACHE_STORAGE_KEY } from "../query/persistence.js";
import { createQueryClient } from "../query/client.js";
import { appCacheShouldPersist } from "../../features/instance/entry.js";
import { instanceAppKeys } from "../../features/instance/queries.js";
import { decidePageGate, isOnboardedProfile, loadGateInputs } from "./guards.js";
import { sessionKeys } from "./queries.js";
import { useSessionStore } from "./store.js";

function memoryStore(): KeyValueStore & { values: Map<string, string> } {
  const values = new Map<string, string>();
  return {
    values,
    get: async (key: string) => values.get(key) ?? null,
    set: async (key: string, value: string) => {
      values.set(key, value);
    },
    remove: async (key: string) => {
      values.delete(key);
    },
  } as unknown as KeyValueStore & { values: Map<string, string> };
}

const me = { id: "u-1", email: "a@example.com", is_password_autoset: false } as unknown as Me;
const profile = { is_onboarded: true } as unknown as Profile;
const workspaceRows = ["ws", "acme", "issues", "list"] as const;

type Answer = { status: number } | { body: unknown } | { network: true };

/** An API client answering by path; unknown paths answer 401. */
function apiClient(answers: Record<string, Answer>) {
  const getJson = vi.fn(async (path: string) => {
    const answer = answers[path] ?? { status: 401 };
    if ("body" in answer) return answer.body;
    if ("network" in answer) throw new ApiError({ status: 0, code: "network", message: "offline" });
    throw new ApiError({ status: answer.status, code: "http", message: "rejected" });
  });
  return { client: { getJson, parse: (_schema: unknown, body: unknown) => body } as never, getJson };
}

const persistOptions = { shouldPersist: appCacheShouldPersist("desktop"), saveDelayMs: 0 };

/** A desktop launch whose previous run saved a signed-in cache to `storage`. */
async function launchWithSavedUser(storage = memoryStore()): Promise<QueryClient> {
  const before = createQueryClient();
  before.setQueryData(sessionKeys.me, me);
  before.setQueryData(sessionKeys.profile, profile);
  before.setQueryData(workspaceRows, ["previous account's issue"]);
  const writer = createCachePersistor(before, storage, persistOptions);
  await writer.save();
  writer.stop();
  // This launch: the startup answer is read live, then the cache restores
  // before the router starts (main.tsx).
  const after = createQueryClient();
  after.setQueryData(instanceAppKeys.bootstrap, { kind: "ready" });
  const reader = createCachePersistor(after, storage, persistOptions);
  await reader.restore();
  reader.stop();
  return after;
}

function entryGate(inputs: { me: Me | null; profile: Profile | null }) {
  return decidePageGate({
    page: "signed-out-only",
    signedIn: inputs.me !== null,
    onboarded: isOnboardedProfile(inputs.profile),
    hasPassword: inputs.me?.is_password_autoset === false,
    currentPath: "/",
  }).action;
}

afterEach(() => {
  useSessionStore.setState({ status: "unknown", returnUrl: null });
});

describe("restored desktop cache vs. the route gates (PIDESKAPP-194)", () => {
  it("asks the server instead of trusting a restored user, so an expired session reaches sign-in", async () => {
    const queryClient = await launchWithSavedUser();
    const { client, getJson } = apiClient({});

    const inputs = await loadGateInputs(queryClient, client);

    expect(getJson).toHaveBeenCalledWith("/api/users/me/");
    expect(inputs).toEqual({ me: null, profile: null });
    expect(entryGate(inputs)).not.toBe("landing");
  });

  it("drops the expired account's rows and keeps the instance answer", async () => {
    const storage = memoryStore();
    const queryClient = await launchWithSavedUser(storage);
    const persistor = createCachePersistor(queryClient, storage, persistOptions);

    await loadGateInputs(queryClient, apiClient({}).client);

    expect(queryClient.getQueryData(sessionKeys.me)).toBeUndefined();
    expect(queryClient.getQueryData(workspaceRows)).toBeUndefined();
    expect(queryClient.getQueryData(instanceAppKeys.bootstrap)).toEqual({ kind: "ready" });
    expect(useSessionStore.getState().status).toBe("signed-out");
    // The next save no longer carries the account, so a relaunch or the
    // next account to sign in cannot restore its rows.
    await persistor.save();
    persistor.stop();
    const saved = storage.values.get(QUERY_CACHE_STORAGE_KEY) ?? "";
    expect(saved).not.toContain("previous account");
    expect(saved).not.toContain("u-1");
    // A relaunch from that payload reads the session live again.
    const relaunched = await launchFrom(storage);
    expect(relaunched.getQueryData(sessionKeys.me)).toBeUndefined();
  });

  it("confirms a live session with a read and lands as before", async () => {
    const queryClient = await launchWithSavedUser();
    const { client, getJson } = apiClient({
      "/api/users/me/": { body: me },
      "/api/users/me/profile/": { body: profile },
    });

    const inputs = await loadGateInputs(queryClient, client);

    expect(inputs).toEqual({ me, profile });
    expect(entryGate(inputs)).toBe("landing");
    expect(getJson).toHaveBeenCalledWith("/api/users/me/");
    // Confirmed this process: the next gate trusts the cache again.
    getJson.mockClear();
    await loadGateInputs(queryClient, client);
    expect(getJson).not.toHaveBeenCalled();
  });

  it("keeps the restored user when the read fails in transit (offline warm launch)", async () => {
    const queryClient = await launchWithSavedUser();
    const { client } = apiClient({
      "/api/users/me/": { network: true },
      "/api/users/me/profile/": { network: true },
    });

    await expect(loadGateInputs(queryClient, client)).resolves.toEqual({ me, profile });
    expect(queryClient.getQueryData(workspaceRows)).toEqual(["previous account's issue"]);
  });

  it("re-reads a user cached this process once a 401 expired the session", async () => {
    const queryClient = createQueryClient();
    queryClient.setQueryData(sessionKeys.me, me);
    queryClient.setQueryData(sessionKeys.profile, profile);
    const { client, getJson } = apiClient({});

    // A fresh in-memory user satisfies the gate without a read...
    await expect(loadGateInputs(queryClient, client)).resolves.toEqual({ me, profile });
    expect(getJson).not.toHaveBeenCalled();
    // ...until the middleware marks the session expired.
    useSessionStore.getState().markExpired("/acme/projects");
    const inputs = await loadGateInputs(queryClient, client);

    expect(getJson).toHaveBeenCalledWith("/api/users/me/");
    expect(inputs).toEqual({ me: null, profile: null });
    expect(entryGate(inputs)).not.toBe("landing");
  });
});

async function launchFrom(storage: KeyValueStore): Promise<QueryClient> {
  const client = createQueryClient();
  const reader = createCachePersistor(client, storage, persistOptions);
  await reader.restore();
  reader.stop();
  return client;
}
