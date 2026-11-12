// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Listener = (p: { account: string; prompt: unknown } | null) => void;

const mocks = vi.hoisted(() => ({
  listener: null as Listener | null,
  replace: vi.fn(),
  decline: vi.fn(),
  setOpen: vi.fn(),
}));

vi.mock("../../../core/api/client.js", () => ({
  getAppClient: () => ({ baseUrl: "https://pidash.test" }),
}));

vi.mock("../../../core/platform/index.js", () => ({
  platform: {
    agentRuntime: {},
    apiOrigin: "https://pidash.test",
    cliSetup: undefined,
  },
}));

vi.mock("./desktop-replace.js", () => ({
  registerReplacePrompt: (listener: Listener | null) => {
    mocks.listener = listener;
  },
  replaceMachine: mocks.replace,
  declineReplace: mocks.decline,
  setReplacePromptOpen: mocks.setOpen,
  errorText: (e: unknown) => {
    const code = (e as { body?: { error?: string } })?.body?.error;
    if (code === "source_machine_has_live_session") return "That computer is still connected to Pi Dash.";
    return e instanceof Error ? e.message : String(e);
  },
}));

vi.mock("./service.js", () => ({
  compatibleHello: () => ({ machine_id: "machine-new" }),
  currentDiscovery: () => ({ kind: "compatible", hello: { machine_id: "machine-new" } }),
  signInWorkspace: vi.fn(),
}));

import { ReplaceMachineDialog } from "./ReplaceMachineDialog.js";

const PROMPT = {
  machineId: "machine-new",
  candidate: { id: "old-1", label: "rich-laptop", host_label: "rich-laptop", last_seen_at: "2026-10-30T12:00:00Z" },
};

async function open(): Promise<void> {
  await act(async () => {
    mocks.listener!({ account: "u-1", prompt: PROMPT });
  });
}

beforeEach(() => {
  vi.resetAllMocks();
  mocks.decline.mockResolvedValue(undefined);
});

afterEach(() => cleanup());

describe("replace-machine dialog", () => {
  it("stays closed until there is a machine to ask about", () => {
    render(<ReplaceMachineDialog />);
    expect(screen.queryByText(/Is this rich-laptop/)).toBeNull();
  });

  it("asks with the old machine's name and last-seen date", async () => {
    render(<ReplaceMachineDialog />);
    await open();
    const date = new Date(PROMPT.candidate.last_seen_at).toLocaleDateString();
    expect(screen.getByText(`Is this rich-laptop, last seen ${date}?`)).toBeTruthy();
  });

  it("Replace it merges and reports what came back", async () => {
    mocks.replace.mockResolvedValue({ merge: {}, restored: ["acme/WEB"], failed: [], ownAgentRunners: 1 });
    render(<ReplaceMachineDialog />);
    await open();
    fireEvent.click(screen.getByRole("button", { name: "Replace it" }));
    await screen.findByText(/was replaced by this computer/);
    expect(mocks.replace).toHaveBeenCalledWith(PROMPT, expect.anything());
    expect(mocks.decline).not.toHaveBeenCalled();
    expect(screen.getByText(/back on for acme\/WEB/)).toBeTruthy();
    expect(screen.getByText(/1 runner with your own agent stay offline/)).toBeTruthy();
  });

  it("a failed merge says so and offers to try again", async () => {
    mocks.replace.mockRejectedValue(
      Object.assign(new Error("Request failed with status code 409"), {
        body: { error: "source_machine_has_live_session" },
      })
    );
    render(<ReplaceMachineDialog />);
    await open();
    expect(mocks.setOpen).toHaveBeenLastCalledWith(true);
    fireEvent.click(screen.getByRole("button", { name: "Replace it" }));
    await screen.findByText("Replacing it did not finish: That computer is still connected to Pi Dash.");
    expect(screen.getByRole("button", { name: "Replace it" })).toBeTruthy();
  });

  it("No, keep it declines for this account and closes", async () => {
    render(<ReplaceMachineDialog />);
    await open();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "No, keep it" }));
    });
    expect(mocks.decline).toHaveBeenCalledWith("u-1", PROMPT, expect.anything());
    expect(mocks.replace).not.toHaveBeenCalled();
    expect(screen.queryByText(/Is this rich-laptop/)).toBeNull();
  });
});
