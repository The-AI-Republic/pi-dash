/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { SWRConfig } from "swr";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { getSTTConfig } = vi.hoisted(() => ({ getSTTConfig: vi.fn() }));

vi.mock("@pi-dash/services", () => ({
  AssistantService: class {
    getSTTConfig = getSTTConfig;
  },
}));

vi.mock("@pi-dash/ui", () => ({
  // Strip non-DOM props so React doesn't warn about forwarding them.
  Button: ({
    children,
    loading: _loading,
    variant: _variant,
    ...props
  }: {
    children: React.ReactNode;
    loading?: boolean;
    variant?: string;
  } & React.ButtonHTMLAttributes<HTMLButtonElement>) => <button {...props}>{children}</button>,
}));

// Imported after the mocks are declared.
import { ChatComposer } from "@/components/chat/composer";

function installMediaSupport() {
  Object.defineProperty(navigator, "mediaDevices", {
    configurable: true,
    value: { getUserMedia: vi.fn() },
  });
  // The composer never records in these tests; it only probes for support.
  (globalThis as unknown as { MediaRecorder: unknown }).MediaRecorder = class {
    static isTypeSupported = () => true;
    start() {}
    stop() {}
  };
}

function renderComposer() {
  return render(
    <SWRConfig value={{ provider: () => new Map(), dedupingInterval: 0 }}>
      <MemoryRouter>
        <ChatComposer draft="" onDraftChange={vi.fn()} onSend={vi.fn()} />
      </MemoryRouter>
    </SWRConfig>
  );
}

const unconfigured = { base_url: "", model_name: "", has_api_key: false, last_verified_at: null };

beforeEach(() => {
  getSTTConfig.mockReset();
  installMediaSupport();
});

describe("ChatComposer voice dictation kill switch", () => {
  it("hides the mic and the not-set-up hint when dictation is disabled", async () => {
    getSTTConfig.mockResolvedValue({ ...unconfigured, enabled: false });
    renderComposer();
    await waitFor(() => expect(getSTTConfig).toHaveBeenCalled());

    expect(screen.queryByRole("button", { name: /dictat/i })).toBeNull();
    expect(screen.queryByText(/voice dictation isn.t set up/i)).toBeNull();
  });

  it("hides dictation when the config read fails", async () => {
    getSTTConfig.mockRejectedValue({ error: "boom" });
    renderComposer();
    await waitFor(() => expect(getSTTConfig).toHaveBeenCalled());

    expect(screen.queryByRole("button", { name: /dictat/i })).toBeNull();
  });

  it("shows the mic and the set-up hint when dictation is enabled but unconfigured", async () => {
    getSTTConfig.mockResolvedValue({ ...unconfigured, enabled: true });
    renderComposer();

    expect(await screen.findByRole("button", { name: "Set up voice dictation" })).toBeTruthy();
    expect(screen.getByText(/voice dictation isn.t set up/i)).toBeTruthy();
  });
});
