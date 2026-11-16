// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Desktop assistant (NEWPIDASH-300): the availability gate chooses between
// the local chat for the Pi Dash Agent and the reason panel, and a role
// without runners access is denied exactly as in the runners area.
import { QueryClientProvider } from "@tanstack/react-query";
import { render as renderRaw, screen, waitFor } from "@testing-library/react";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const contracts = vi.hoisted(() => ({ getPidashAgentProfile: vi.fn() }));
const session = vi.hoisted(() => ({ useWorkspaces: vi.fn(), usePermissions: vi.fn() }));

vi.mock("@pidash/api-client", async (importOriginal) => {
  const original = await importOriginal<typeof import("@pidash/api-client")>();
  return { ...original, ...contracts };
});
vi.mock("../../../core/session/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../core/session/index.js")>();
  return { ...original, useWorkspaces: session.useWorkspaces, usePermissions: session.usePermissions };
});
vi.mock("../../runners/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../runners/index.js")>();
  return {
    ...original,
    RunnerChatPage: (props: { runnerId: string; requestedSessionId: string | null }) => (
      <div
        data-testid="descriptor-chat-slot"
        data-runner={props.runnerId}
        data-session={props.requestedSessionId ?? ""}
      />
    ),
  };
});

const { createAppClient, resetAppClient } = await import("../../../core/api/client.js");
const { testClient } = await import("../../runners/chat/harness.js");
const { DesktopAssistant } = await import("./DesktopAssistant.js");

const Provider = QueryClientProvider as unknown as React.ComponentType<{
  client: ReturnType<typeof testClient>;
  children?: React.ReactNode;
}>;

function render(props: Partial<React.ComponentProps<typeof DesktopAssistant>> = {}) {
  const client = testClient();
  return renderRaw(
    <Provider client={client}>
      <DesktopAssistant workspaceSlug="acme" requestedSessionId={null} onSelectSession={vi.fn()} {...props} />
    </Provider>
  );
}

function profile(available: boolean, reason_code = ""): { available: boolean; reason_code: string } {
  return { available, reason_code };
}

beforeEach(() => {
  createAppClient("https://api.test", {} as never);
  contracts.getPidashAgentProfile.mockResolvedValue(profile(true));
  session.useWorkspaces.mockReturnValue({ data: [{ id: "ws-id", slug: "acme", name: "Acme" }] });
  session.usePermissions.mockReturnValue({ role: 15, isLoading: false, canEdit: true, isAdmin: false });
});

afterEach(() => {
  contracts.getPidashAgentProfile.mockReset();
  resetAppClient();
});

describe("DesktopAssistant", () => {
  it("mounts the local chat for the Pi Dash Agent when the agent is available", async () => {
    render({ requestedSessionId: "s-1" });
    const slot = await screen.findByTestId("descriptor-chat-slot");
    expect(slot).toHaveAttribute("data-runner", "pidash-builtin");
    expect(slot).toHaveAttribute("data-session", "s-1");
  });

  it("shows the server's reason when the agent is unavailable, and no chat", async () => {
    contracts.getPidashAgentProfile.mockResolvedValue(profile(false, "managed_runner_disabled"));
    render();
    expect(await screen.findByTestId("assistant-desktop-unavailable-reason")).toHaveTextContent(
      "The Pi Dash Agent is switched off on this server."
    );
    expect(screen.queryByTestId("descriptor-chat-slot")).toBeNull();
  });

  it("reads a failed profile check as unavailable in its own words", async () => {
    contracts.getPidashAgentProfile.mockRejectedValue(new Error("Desktop session required"));
    render();
    expect(await screen.findByTestId("assistant-desktop-unavailable-reason")).toHaveTextContent(
      "Desktop session required"
    );
    expect(screen.queryByTestId("descriptor-chat-slot")).toBeNull();
  });

  it("denies a role without runners access, exactly as the runners area does", async () => {
    session.usePermissions.mockReturnValue({ role: 5, isLoading: false, canEdit: false, isAdmin: false });
    render();
    expect(await screen.findByTestId("runners-denied")).toBeInTheDocument();
    expect(screen.queryByTestId("descriptor-chat-slot")).toBeNull();
  });

  it("waits for the profile answer before mounting anything", async () => {
    contracts.getPidashAgentProfile.mockReturnValue(new Promise(() => undefined));
    render();
    // Nothing settled yet: no chat, no refusal — the answer decides.
    await waitFor(() => expect(contracts.getPidashAgentProfile).toHaveBeenCalled());
    expect(screen.queryByTestId("descriptor-chat-slot")).toBeNull();
    expect(screen.queryByTestId("assistant-desktop-unavailable")).toBeNull();
  });
});
