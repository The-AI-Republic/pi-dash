// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The one-time migration notices (design §13, PO-11) against a fake settings
// store: shown while pending, the pod-work switch patches the runner, and
// "Got it" dismisses them for good — a later mount shows nothing.

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  store: {
    notices: {
      legacy_folders: [] as { workspace: string; project: string; path: string }[],
      pod_member_projects: [] as { runner_id: string; workspace: string; project: string }[],
    },
    dismissed: [] as string[],
  },
  listeners: new Set<() => void>(),
  setPodWork: vi.fn(),
  getServerRunner: vi.fn(),
  replaceOpen: false,
  replaceListeners: new Set<() => void>(),
}));

vi.mock("../../../core/api/client.js", () => ({
  getAppClient: () => ({ baseUrl: "https://pidash.test" }),
}));

vi.mock("../../../core/platform/index.js", () => ({
  platform: {
    agentRuntime: {
      loadNotices: async () => structuredClone(mocks.store),
      dismissNotices: async (_account: string, keys: string[]) => {
        mocks.store.dismissed.push(...keys);
        mocks.store.notices = {
          legacy_folders: mocks.store.notices.legacy_folders.filter((n) => !keys.includes(`folder:${n.path}`)),
          pod_member_projects: mocks.store.notices.pod_member_projects.filter(
            (n) => !keys.includes(`pod:${n.runner_id}`)
          ),
        };
        return structuredClone(mocks.store);
      },
    },
  },
}));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, getRunner: mocks.getServerRunner, updateRunner: mocks.setPodWork };
});

vi.mock("./desktop-replace.js", () => ({
  replacePromptOpen: () => mocks.replaceOpen,
  subscribeReplacePromptOpen: (listener: () => void) => {
    mocks.replaceListeners.add(listener);
    return () => mocks.replaceListeners.delete(listener);
  },
}));

// Keep the real notices helpers (keys, load, subscribe) so the dialog runs
// against the mocked platform. The mocked api-client above backs the pod
// switch, so the real desktop-notices module stays in place.

import { MigrationNoticesDialog } from "./MigrationNoticesDialog.js";

beforeEach(() => {
  vi.resetAllMocks();
  mocks.setPodWork.mockResolvedValue({});
  mocks.getServerRunner.mockResolvedValue({ id: "r-api", pod_dispatch: "pod_member" });
  mocks.replaceOpen = false;
  mocks.replaceListeners.clear();
  mocks.listeners.clear();
  mocks.store = {
    notices: {
      legacy_folders: [{ workspace: "acme", project: "WEB", path: "/app/managed/workdirs/acme/WEB" }],
      pod_member_projects: [{ runner_id: "r-api", workspace: "acme", project: "API" }],
    },
    dismissed: [],
  };
});

afterEach(() => cleanup());

const HEADING = "This app now uses Pi Dash on this computer";

describe("migration notices", () => {
  it("shows nothing when there is nothing pending", async () => {
    mocks.store.notices = { legacy_folders: [], pod_member_projects: [] };
    render(<MigrationNoticesDialog account="u-1" />);
    await act(async () => {});
    expect(screen.queryByText(HEADING)).toBeNull();
  });

  it("names the left-over folders and the pod-member projects", async () => {
    render(<MigrationNoticesDialog account="u-1" />);
    await screen.findByText(HEADING);
    expect(screen.getByText("/app/managed/workdirs/acme/WEB")).toBeTruthy();
    expect(screen.getByText("acme / API")).toBeTruthy();
    expect(screen.getByRole("switch", { name: "Pod work in API" })).toBeTruthy();
  });

  it("the switch shows the server's current pod work", async () => {
    mocks.getServerRunner.mockResolvedValue({ id: "r-api", pod_dispatch: "pinned_only" });
    render(<MigrationNoticesDialog account="u-1" />);
    await screen.findByText(HEADING);
    await act(async () => {});
    expect(mocks.getServerRunner).toHaveBeenCalled();
    const toggle = screen.getByRole("switch", { name: "Pod work in API" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
  });

  it("waits while the Replace it dialog is open instead of stacking on it", async () => {
    mocks.replaceOpen = true;
    render(<MigrationNoticesDialog account="u-1" />);
    await act(async () => {});
    expect(screen.queryByText(HEADING)).toBeNull();
    await act(async () => {
      mocks.replaceOpen = false;
      mocks.replaceListeners.forEach((l) => l());
    });
    await screen.findByText(HEADING);
  });

  it("Got it dismisses them for good", async () => {
    render(<MigrationNoticesDialog account="u-1" />);
    await screen.findByText(HEADING);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    });
    expect(mocks.store.dismissed).toEqual(["folder:/app/managed/workdirs/acme/WEB", "pod:r-api"]);
    expect(screen.queryByText(HEADING)).toBeNull();
  });
});
