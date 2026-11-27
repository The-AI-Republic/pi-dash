/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * On the desktop, this computer's own engine runners are the "This computer"
 * contact; the nav must not list them a second time under "Runners"
 * (design §12.1). Other computers' engines and own-agent runners stay.
 */

import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  localIds: new Set<string>(),
  list: vi.fn(),
}));

vi.mock("mobx-react", () => ({ observer: (component: unknown) => component }));
vi.mock("@pi-dash/i18n", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@pi-dash/services", () => ({
  RunnerService: class {
    list = mocks.list;
  },
}));
vi.mock("@/components/runners/local-chat-contacts", () => ({
  useLocalChatContacts: () =>
    mocks.localIds.size ? [{ id: "pidash-builtin", name: "This computer", label: "Built-in" }] : [],
}));
vi.mock("@/components/runners/this-computer", () => ({
  useThisComputer: () => ({ localEngineRunnerIds: mocks.localIds }),
}));

import { RunnersSideNav } from "@/components/runners/runners-side-nav";

const RUNNERS = [
  { id: "engine-here", name: "builtin-laptop-acme-web", status: "online" },
  { id: "engine-elsewhere", name: "builtin-desktop-acme-web", status: "offline" },
  { id: "own-agent-here", name: "claude-on-laptop", status: "online" },
];

describe("RunnersSideNav and this computer", () => {
  it("hides this computer's engine runners behind the local contact", async () => {
    mocks.localIds = new Set(["engine-here"]);
    mocks.list.mockResolvedValue(RUNNERS);
    render(
      <MemoryRouter>
        <RunnersSideNav workspaceId="ws-1" workspaceSlug="acme" />
      </MemoryRouter>
    );
    expect(await screen.findByText("builtin-desktop-acme-web")).toBeTruthy();
    expect(screen.getByText("claude-on-laptop")).toBeTruthy();
    expect(screen.getByText("This computer")).toBeTruthy();
    expect(screen.queryByText("builtin-laptop-acme-web")).toBeNull();
  });

  it("lists every runner in the web app", async () => {
    mocks.localIds = new Set();
    mocks.list.mockResolvedValue(RUNNERS);
    render(
      <MemoryRouter>
        <RunnersSideNav workspaceId="ws-2" workspaceSlug="acme" />
      </MemoryRouter>
    );
    expect(await screen.findByText("builtin-laptop-acme-web")).toBeTruthy();
    expect(screen.getByText("builtin-desktop-acme-web")).toBeTruthy();
  });
});
