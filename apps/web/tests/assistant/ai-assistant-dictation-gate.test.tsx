/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, waitFor } from "@testing-library/react";
import { SWRConfig } from "swr";
import { beforeEach, describe, expect, it, vi } from "vitest";

const { getLLMConfig, getSTTConfig } = vi.hoisted(() => ({ getLLMConfig: vi.fn(), getSTTConfig: vi.fn() }));

vi.mock("@pi-dash/services", () => ({
  AssistantService: class {
    getLLMConfig = getLLMConfig;
    getSTTConfig = getSTTConfig;
  },
}));

vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { ERROR: "ERROR", SUCCESS: "SUCCESS", INFO: "INFO" },
  setToast: vi.fn(),
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

// The MCP servers section has its own service calls and tests; stub it out.
vi.mock("@/components/settings/profile/content/pages/assistant-mcp-servers", () => ({
  AssistantMCPServersSettings: () => <div>MCP servers</div>,
}));

// Imported after the mocks are declared.
import { AIAssistantProfileSettings } from "@/components/settings/profile/content/pages/ai-assistant";

function renderPage() {
  return render(
    <SWRConfig value={{ provider: () => new Map(), dedupingInterval: 0 }}>
      <AIAssistantProfileSettings />
    </SWRConfig>
  );
}

const sttConfig = { base_url: "", model_name: "", has_api_key: false, last_verified_at: null };

beforeEach(() => {
  getSTTConfig.mockReset();
  getLLMConfig.mockReset().mockResolvedValue({
    provider_kind: "openai_compatible",
    base_url: "",
    model_name: "",
    has_api_key: false,
    last_verified_at: null,
  });
});

describe("AI Assistant settings: voice dictation kill switch", () => {
  it("omits the Voice dictation section when dictation is disabled and no key is saved", async () => {
    getSTTConfig.mockResolvedValue({ ...sttConfig, enabled: false });
    renderPage();
    await screen.findByText("MCP servers");
    await waitFor(() => expect(getSTTConfig).toHaveBeenCalled());

    expect(screen.queryByText("Voice dictation")).toBeNull();
  });

  it("omits the section when the dictation config cannot be read", async () => {
    getSTTConfig.mockRejectedValue({ error: "boom" });
    renderPage();
    await screen.findByText("MCP servers");
    await waitFor(() => expect(getSTTConfig).toHaveBeenCalled());

    expect(screen.queryByText("Voice dictation")).toBeNull();
  });

  it("shows the section when dictation is enabled", async () => {
    getSTTConfig.mockResolvedValue({ ...sttConfig, enabled: true });
    renderPage();

    expect(await screen.findByText("Voice dictation")).toBeTruthy();
  });

  it("keeps the section when disabled but a key is saved, so the user can remove it", async () => {
    getSTTConfig.mockResolvedValue({ ...sttConfig, has_api_key: true, enabled: false });
    renderPage();

    expect(await screen.findByText(/voice dictation is currently unavailable/i)).toBeTruthy();
  });
});
