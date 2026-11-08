// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  setUp: vi.fn(),
}));

vi.mock("./service.js", () => ({
  setUpDaemon: (...args: Array<unknown>) => harness.setUp(...args),
}));

vi.mock("../../../core/platform/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/platform/index.js")>();
  return { ...original, platform: { ...original.platform, agentRuntime: {}, cliSetup: {} } };
});

import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { DaemonSetupDialog, SETUP_STEP_TEXT, SETUP_TITLE } from "./DaemonSetupDialog.js";
import { registerSetUpPrompt, requestDaemonSetup } from "./setupPrompt.js";

beforeEach(() => {
  harness.setUp.mockReset();
  createAppClient("https://api.test");
});

afterEach(() => {
  registerSetUpPrompt(null);
  resetAppClient();
});

describe("requestDaemonSetup", () => {
  it("declines when no consent dialog is mounted", async () => {
    await expect(requestDaemonSetup({ reason: "chat", workspaceSlug: "ws" })).resolves.toBe(false);
  });
});

describe("DaemonSetupDialog", () => {
  it("sets nothing up when the user declines", async () => {
    const user = userEvent.setup();
    render(<DaemonSetupDialog />);
    const pending = requestDaemonSetup({ reason: "chat", workspaceSlug: "ws" });
    expect(await screen.findByText(SETUP_TITLE)).toBeVisible();
    expect(screen.getByText(/background service/)).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Not now" }));
    await expect(pending).resolves.toBe(false);
    expect(harness.setUp).not.toHaveBeenCalled();
  });

  it("runs the set-up for the asking workspace and shows each step", async () => {
    const user = userEvent.setup();
    let finish!: () => void;
    harness.setUp.mockImplementation(
      (input: { onStep: (step: string) => void }) =>
        new Promise<void>((resolve) => {
          input.onStep("service");
          finish = resolve;
        })
    );
    render(<DaemonSetupDialog />);
    const pending = requestDaemonSetup({ reason: "engine", workspaceSlug: "acme" });
    await user.click(await screen.findByTestId("daemon-setup-accept"));
    expect(await screen.findByText(SETUP_STEP_TEXT.service)).toBeVisible();
    expect(harness.setUp).toHaveBeenCalledWith(expect.objectContaining({ workspaceSlug: "acme" }));
    await act(async () => finish());
    await expect(pending).resolves.toBe(true);
  });

  it("shows a failure and lets the user try again", async () => {
    const user = userEvent.setup();
    harness.setUp.mockRejectedValueOnce(new Error("install failed")).mockResolvedValueOnce(undefined);
    render(<DaemonSetupDialog />);
    const pending = requestDaemonSetup({ reason: "chat", workspaceSlug: "ws" });
    await user.click(await screen.findByTestId("daemon-setup-accept"));
    expect(await screen.findByRole("alert")).toHaveTextContent("install failed");
    await user.click(screen.getByRole("button", { name: "Try again" }));
    await expect(pending).resolves.toBe(true);
    expect(harness.setUp).toHaveBeenCalledTimes(2);
  });

  it("settles a pending request as declined when it unmounts", async () => {
    const view = render(<DaemonSetupDialog />);
    const pending = requestDaemonSetup({ reason: "chat", workspaceSlug: "ws" });
    await screen.findByText(SETUP_TITLE);
    view.unmount();
    await expect(pending).resolves.toBe(false);
  });
});
