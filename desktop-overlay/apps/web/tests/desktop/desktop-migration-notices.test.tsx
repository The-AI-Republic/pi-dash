/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * The one-time migration notices (design §13, PO-11) against a fake settings
 * store: shown while pending, the pod-work switch patches the runner, and
 * "Got it" dismisses them for good — a later mount shows nothing.
 */

import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Notices = {
  legacy_folders: { workspace: string; project: string; path: string }[];
  pod_member_projects: { runner_id: string; workspace: string; project: string }[];
};

const mocks = vi.hoisted(() => ({
  store: { notices: { legacy_folders: [], pod_member_projects: [] } as Notices, dismissed: [] as string[] },
  listeners: new Set<() => void>(),
  setPodWork: vi.fn(),
  folderKey: (n: { path: string }) => `folder:${n.path}`,
  podKey: (n: { runner_id: string }) => `pod:${n.runner_id}`,
}));

vi.mock("@/services/desktop-notices", () => {
  const { folderKey, podKey } = mocks;
  return {
    folderKey,
    podKey,
    hasNotices: (n: Notices) => n.legacy_folders.length > 0 || n.pod_member_projects.length > 0,
    loadNotices: async () => structuredClone(mocks.store),
    dismissNotices: async (_account: string, keys: string[]) => {
      mocks.store.dismissed.push(...keys);
      mocks.store.notices = {
        legacy_folders: mocks.store.notices.legacy_folders.filter((n) => !keys.includes(folderKey(n))),
        pod_member_projects: mocks.store.notices.pod_member_projects.filter((n) => !keys.includes(podKey(n))),
      };
      return structuredClone(mocks.store);
    },
    subscribeNotices: (listener: () => void) => {
      mocks.listeners.add(listener);
      return () => mocks.listeners.delete(listener);
    },
    setPodWork: mocks.setPodWork,
  };
});

import { DesktopMigrationNotices } from "../../core/components/desktop-migration-notices";

beforeEach(() => {
  vi.resetAllMocks();
  mocks.setPodWork.mockResolvedValue({});
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
    render(<DesktopMigrationNotices account="u-1" />);
    await act(async () => {});
    expect(screen.queryByText(HEADING)).toBeNull();
  });

  it("names the left-over folders and the pod-member projects", async () => {
    render(<DesktopMigrationNotices account="u-1" />);
    await screen.findByText(HEADING);
    expect(screen.getByText("/app/managed/workdirs/acme/WEB")).toBeTruthy();
    expect(screen.getByText("acme / API")).toBeTruthy();
    expect(screen.getByRole<HTMLInputElement>("switch", { name: "Pod work in API" }).checked).toBe(true);
  });

  it("the switch turns pod work off for that project's engine", async () => {
    render(<DesktopMigrationNotices account="u-1" />);
    const toggle = await screen.findByRole<HTMLInputElement>("switch", { name: "Pod work in API" });
    await act(async () => {
      fireEvent.click(toggle);
    });
    expect(mocks.setPodWork).toHaveBeenCalledWith("r-api", false);
    expect(screen.getByRole<HTMLInputElement>("switch", { name: "Pod work in API" }).checked).toBe(false);
  });

  it("Got it dismisses them for good: shown once", async () => {
    const first = render(<DesktopMigrationNotices account="u-1" />);
    await screen.findByText(HEADING);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    });
    expect(mocks.store.dismissed).toEqual(["folder:/app/managed/workdirs/acme/WEB", "pod:r-api"]);
    expect(screen.queryByText(HEADING)).toBeNull();
    first.unmount();
    // The next launch.
    render(<DesktopMigrationNotices account="u-1" />);
    await act(async () => {});
    expect(screen.queryByText(HEADING)).toBeNull();
  });

  it("appears when the migration records notices while the app is open", async () => {
    const pending = mocks.store.notices;
    mocks.store.notices = { legacy_folders: [], pod_member_projects: [] };
    render(<DesktopMigrationNotices account="u-1" />);
    await act(async () => {});
    expect(screen.queryByText(HEADING)).toBeNull();
    mocks.store.notices = pending;
    await act(async () => {
      for (const listener of mocks.listeners) listener();
    });
    await screen.findByText(HEADING);
  });
});
