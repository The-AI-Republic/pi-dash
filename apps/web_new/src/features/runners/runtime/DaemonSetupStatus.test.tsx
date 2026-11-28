// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
const setup = vi.hoisted(() => vi.fn(async () => undefined));
vi.mock("./service.js", () => ({ setUpDaemon: setup }));
import { createAppClient, resetAppClient } from "../../../core/api/client.js";
import { DaemonSetupStatus } from "./DaemonSetupStatus.js";
import { publishSetupStatus } from "./setupStatus.js";
afterEach(() => {
  publishSetupStatus(null);
  resetAppClient();
  setup.mockClear();
});
describe("DaemonSetupStatus", () => {
  it("shows non-modal progress with no confirmation", () => {
    publishSetupStatus({ kind: "running", step: "engine" });
    render(<DaemonSetupStatus workspaceSlug="ws" />);
    expect(screen.getByRole("status")).toHaveTextContent("installing the Pi Dash Agent");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
    act(() => publishSetupStatus(null));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
  it("offers retry after failure without asking for agreement", async () => {
    createAppClient("https://api.test");
    publishSetupStatus({ kind: "failed", error: new Error("download failed") });
    render(<DaemonSetupStatus workspaceSlug="ws" />);
    expect(screen.getByRole("alert")).toHaveTextContent("download failed");
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }));
    expect(setup).toHaveBeenCalledWith(expect.objectContaining({ workspaceSlug: "ws" }));
  });
});
