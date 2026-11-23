// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// AI-assistant tab body (parity/specs/profile.md §10, PROF-021..023): the
// quiet loading frame, one-time seeding from each read, the exact payload
// each save posts, the key clearing after a save, and the tool-server
// actions carrying the row they touched.
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError, type LlmConfig, type McpServer, type SttConfig } from "@pidash/api-client";

const harness = vi.hoisted(() => ({
  llm: { data: undefined as LlmConfig | undefined },
  voice: { data: undefined as SttConfig | undefined },
  servers: { data: undefined as McpServer[] | undefined },
  saveLlm: { mutate: vi.fn(), isPending: false },
  testLlmModel: { mutate: vi.fn(), isPending: false },
  removeLlm: { mutate: vi.fn(), isPending: false },
  saveVoice: { mutate: vi.fn(), isPending: false },
  testVoice: { mutate: vi.fn(), isPending: false },
  removeVoice: { mutate: vi.fn(), isPending: false },
  addServer: { mutate: vi.fn(), isPending: false },
  toggleServer: { mutate: vi.fn(), isPending: false },
  removeServer: { mutate: vi.fn(), isPending: false },
}));

vi.mock("../api/queries.js", () => ({
  useAssistantLlm: () => harness.llm,
  useAssistantVoice: () => harness.voice,
  useAssistantServers: () => harness.servers,
}));

vi.mock("../api/mutations.js", () => ({
  useSaveAssistantLlm: () => harness.saveLlm,
  useTestAssistantLlmModel: () => harness.testLlmModel,
  useRemoveAssistantLlm: () => harness.removeLlm,
  useSaveAssistantVoice: () => harness.saveVoice,
  useTestAssistantVoice: () => harness.testVoice,
  useRemoveAssistantVoice: () => harness.removeVoice,
  useAddToolServer: () => harness.addServer,
  useToggleToolServer: () => harness.toggleServer,
  useRemoveToolServer: () => harness.removeServer,
}));

// Dictation ships hidden (DICTATION_UI_ENABLED); this suite switches it on
// so the dormant UI stays covered, and pin the hidden default separately.
const dictationUi = vi.hoisted(() => ({ enabled: true }));
vi.mock("../../../shared/dictation/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/dictation/index.js")>();
  return {
    ...original,
    get DICTATION_UI_ENABLED() {
      return dictationUi.enabled;
    },
  };
});

vi.mock("@tanstack/react-router", () => ({
  useParams: () => ({ ws: "acme" }),
}));

vi.mock("./AssistantModelsSection.js", () => ({
  AssistantModelsSection: ({ workspaceSlug }: { workspaceSlug: string }) => (
    <div data-testid="assistant-models-section-stub">{workspaceSlug}</div>
  ),
}));

import { AssistantSettingsPage } from "./AssistantSettingsPage.js";

const SERVER_ID = "3f7c1d1e-0d1a-4c59-9f0b-6a2f3e4d5c6b";

const LLM: LlmConfig = {
  provider_kind: "openai_compatible",
  base_url: "https://llm.test/v1",
  model_name: "model-a",
  has_api_key: true,
  last_verified_at: null,
};

const VOICE: SttConfig = {
  base_url: "https://stt.test/v1",
  model_name: "whisper-1",
  has_api_key: false,
  last_verified_at: null,
  enabled: true,
};

const SERVER: McpServer = {
  id: SERVER_ID,
  name: "probe",
  url: "https://mcp.test",
  has_auth_header: false,
  tool_prefix: "probe",
  effective_tool_prefix: "probe",
  is_enabled: true,
  created_at: "2026-10-19T00:00:00Z",
  updated_at: "2026-10-19T00:00:00Z",
};

beforeEach(() => {
  for (const mutation of [
    harness.saveLlm,
    harness.testLlmModel,
    harness.removeLlm,
    harness.saveVoice,
    harness.testVoice,
    harness.removeVoice,
    harness.addServer,
    harness.toggleServer,
    harness.removeServer,
  ]) {
    mutation.mutate = vi.fn();
    mutation.isPending = false;
  }
  harness.llm = { data: LLM };
  harness.voice = { data: VOICE };
  harness.servers = { data: [SERVER] };
});

describe("AssistantSettingsPage", () => {
  it("renders the Models section first with the workspace slug", () => {
    render(<AssistantSettingsPage />);
    const section = screen.getByTestId("assistant-models-section-stub");
    expect(section).toHaveTextContent("acme");
    const page = screen.getByTestId("assistant-settings-page");
    expect(page.firstElementChild).toBe(section);
  });

  it("holds a quiet frame until both configs arrive", () => {
    harness.llm = { data: undefined };
    render(<AssistantSettingsPage />);
    expect(screen.getByTestId("assistant-settings-loading")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("assistant-settings-page")).not.toBeInTheDocument();
  });

  it("holds the frame when only the voice read is missing", () => {
    harness.voice = { data: undefined };
    render(<AssistantSettingsPage />);
    expect(screen.getByTestId("assistant-settings-loading")).toBeInTheDocument();
  });

  it("leaves the voice section out while dictation is switched off", () => {
    dictationUi.enabled = false;
    try {
      render(<AssistantSettingsPage />);
      expect(screen.getByTestId("assistant-llm")).toBeInTheDocument();
      expect(screen.queryByTestId("assistant-voice")).toBeNull();
    } finally {
      dictationUi.enabled = true;
    }
  });

  it("seeds every form from the member's reads", () => {
    render(<AssistantSettingsPage />);
    expect(screen.getByTestId("assistant-settings-page")).toBeInTheDocument();
    expect(within(screen.getByTestId("assistant-llm")).getByLabelText("Base URL")).toHaveValue("https://llm.test/v1");
    expect(within(screen.getByTestId("assistant-voice")).getByLabelText("Base URL")).toHaveValue("https://stt.test/v1");
    expect(within(screen.getByTestId("assistant-llm")).getByText("model-a")).toBeInTheDocument();
    expect(screen.getByLabelText("Model")).toHaveValue("whisper-1");
    expect(screen.getByText("probe_*")).toBeInTheDocument();
  });

  it("posts the trimmed URL provider with the typed key, then clears the key", async () => {
    const user = userEvent.setup();
    render(<AssistantSettingsPage />);
    const key = screen.getAllByLabelText("API key")[0]!;
    await user.type(key, "  secret  ");
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]!);
    const [input, options] = harness.saveLlm.mutate.mock.calls[0]!;
    expect(input).toEqual({
      provider_kind: "openai_compatible",
      base_url: "https://llm.test/v1",
      model_name: "model-a",
      models: ["model-a"],
      api_key: "secret",
    });
    expect(key).toHaveValue("  secret  ");
    options.onSuccess({ ...LLM, models: ["model-a"] });
    await waitFor(() => expect(key).toHaveValue(""));
  });

  it("keeps the stored key when the field is blank, posting no api_key", async () => {
    const user = userEvent.setup();
    render(<AssistantSettingsPage />);
    await user.type(screen.getByLabelText("Add a model ID this key serves"), "model-b{Enter}");
    await user.click(screen.getByRole("button", { name: "Remove model-a" }));
    expect(screen.getAllByRole("button", { name: "Save" })[0]).toBeEnabled();
    await user.selectOptions(screen.getByLabelText("Provider"), "anthropic");
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]!);
    expect(harness.saveLlm.mutate.mock.calls[0]![0]).toEqual({
      provider_kind: "anthropic",
      model_name: "model-b",
      models: ["model-b"],
    });
    expect(screen.getAllByLabelText("API key")[0]).toHaveValue("");
  });

  it("keeps the stored voice key when the field is blank, posting no api_key", async () => {
    const user = userEvent.setup();
    harness.voice = { data: { ...VOICE, has_api_key: true } };
    render(<AssistantSettingsPage />);
    await user.click(screen.getAllByRole("button", { name: "Save" })[1]!);
    expect(harness.saveVoice.mutate.mock.calls[0]![0]).toEqual({
      base_url: "https://stt.test/v1",
      model_name: "whisper-1",
    });
  });

  it("omits the endpoint for the named provider", async () => {
    const user = userEvent.setup();
    render(<AssistantSettingsPage />);
    await user.selectOptions(screen.getByLabelText("Provider"), "anthropic");
    await user.type(screen.getAllByLabelText("API key")[0]!, "k");
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]!);
    expect(harness.saveLlm.mutate.mock.calls[0]![0]).toEqual({
      provider_kind: "anthropic",
      model_name: "model-a",
      models: ["model-a"],
      api_key: "k",
    });
  });

  it("posts the voice config and clears its key on success", async () => {
    const user = userEvent.setup();
    render(<AssistantSettingsPage />);
    const keys = screen.getAllByLabelText("API key");
    await user.type(keys[1]!, "voice-key");
    await user.click(screen.getAllByRole("button", { name: "Save" })[1]!);
    const [input, options] = harness.saveVoice.mutate.mock.calls[0]!;
    expect(input).toEqual({ base_url: "https://stt.test/v1", model_name: "whisper-1", api_key: "voice-key" });
    options.onSuccess();
    await waitFor(() => expect(screen.getAllByLabelText("API key")[1]).toHaveValue(""));
  });

  it("wires test and remove to their own hooks", async () => {
    const user = userEvent.setup();
    harness.voice = { data: { ...VOICE, has_api_key: true } };
    harness.llm = { data: { ...LLM, has_api_key: true } };
    render(<AssistantSettingsPage />);
    await user.click(within(screen.getByTestId("assistant-llm")).getByRole("button", { name: "Test model-a" }));
    await user.click(within(screen.getByTestId("assistant-voice")).getByRole("button", { name: "Test connection" }));
    await user.click(within(screen.getByTestId("assistant-llm")).getByRole("button", { name: "Remove" }));
    await user.click(within(screen.getByTestId("assistant-voice")).getByRole("button", { name: "Remove" }));
    expect(harness.testLlmModel.mutate).toHaveBeenCalledTimes(1);
    expect(harness.testLlmModel.mutate.mock.calls[0]![0]).toBe("model-a");
    expect(harness.testVoice.mutate).toHaveBeenCalledTimes(1);
    expect(harness.removeLlm.mutate).toHaveBeenCalledTimes(1);
    expect(harness.removeVoice.mutate).toHaveBeenCalledTimes(1);
  });

  it("adds a server with the trimmed fields, an optional auth header, then clears the draft", async () => {
    const user = userEvent.setup();
    render(<AssistantSettingsPage />);
    await user.type(screen.getByLabelText("Name"), "  wiki  ");
    await user.type(screen.getByLabelText("Server URL"), "  https://wiki.test/mcp  ");
    await user.click(screen.getByRole("button", { name: "Add server" }));
    const [input, options] = harness.addServer.mutate.mock.calls[0]!;
    expect(input).toEqual({ name: "wiki", url: "https://wiki.test/mcp" });
    options.onSuccess();
    await waitFor(() => expect(screen.getByLabelText("Name")).toHaveValue(""));
  });

  it("sends a typed auth header when one is present", async () => {
    const user = userEvent.setup();
    render(<AssistantSettingsPage />);
    await user.type(screen.getByLabelText("Name"), "wiki");
    await user.type(screen.getByLabelText("Server URL"), "https://wiki.test/mcp");
    await user.type(screen.getByLabelText("Authorization header (optional)"), "  Bearer t  ");
    await user.click(screen.getByRole("button", { name: "Add server" }));
    expect(harness.addServer.mutate.mock.calls[0]![0]).toEqual({
      name: "wiki",
      url: "https://wiki.test/mcp",
      auth_header: "Bearer t",
    });
  });

  it("toggles the opposite of a row's state and clears its pending mark on settle", async () => {
    const user = userEvent.setup();
    render(<AssistantSettingsPage />);
    await user.click(screen.getByRole("button", { name: "Disable probe" }));
    const [vars, options] = harness.toggleServer.mutate.mock.calls[0]!;
    expect(vars).toEqual({ serverId: SERVER_ID, isEnabled: false });
    expect(screen.getByRole("button", { name: "Disable probe" })).toHaveAttribute("aria-busy", "true");
    options.onSettled();
    await waitFor(() => expect(screen.getByRole("button", { name: "Disable probe" })).not.toHaveAttribute("aria-busy"));
  });

  it("removes a row by id and name, clearing its pending mark", async () => {
    const user = userEvent.setup();
    render(<AssistantSettingsPage />);
    const row = screen.getByTestId("tool-server-row");
    await user.click(within(row).getByRole("button", { name: "Remove" }));
    const [vars, options] = harness.removeServer.mutate.mock.calls[0]!;
    expect(vars).toEqual({ serverId: SERVER_ID, name: "probe" });
    options.onSettled();
    await waitFor(() =>
      expect(within(screen.getByTestId("tool-server-row")).getByRole("button", { name: "Remove" })).toBeEnabled()
    );
  });

  it("deletes the key after confirming the last model ID's removal", async () => {
    const user = userEvent.setup();
    render(<AssistantSettingsPage />);
    await user.click(screen.getByRole("button", { name: "Remove model-a" }));
    expect(harness.removeLlm.mutate).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Remove both" }));
    expect(harness.removeLlm.mutate).toHaveBeenCalledTimes(1);
  });

  it("surfaces a refused save per ID and retries with Save anyway", async () => {
    const user = userEvent.setup();
    render(<AssistantSettingsPage />);
    await user.click(screen.getAllByRole("button", { name: "Save" })[0]!);
    const [, options] = harness.saveLlm.mutate.mock.calls[0]!;
    act(() => {
      options.onError(
        new ApiError({
          message: "Model test failed",
          status: 400,
          code: "model_test_failed",
          body: {
            error: "model_test_failed",
            models: [{ model: "model-a", status: "unverified", code: "provider_unreachable", detail: "Timed out." }],
          },
        })
      );
    });
    expect(screen.getByTestId("byok-model-detail")).toHaveTextContent("Timed out.");
    expect(screen.getByTestId("byok-model-status")).toHaveTextContent("Not verified");
    await user.click(screen.getByRole("button", { name: "Save anyway" }));
    expect(harness.saveLlm.mutate.mock.calls[1]![0]).toEqual({
      provider_kind: "openai_compatible",
      base_url: "https://llm.test/v1",
      model_name: "model-a",
      models: ["model-a"],
      save_unverified: true,
    });
  });

  it("does not re-seed a form from a refetch with the same values, so typing survives", async () => {
    const user = userEvent.setup();
    const { rerender } = render(<AssistantSettingsPage />);
    const draft = screen.getByLabelText("Add a model ID this key serves");
    await user.type(draft, "typed");
    harness.llm = { data: { ...LLM } };
    harness.voice = { data: { ...VOICE } };
    rerender(<AssistantSettingsPage />);
    expect(screen.getByLabelText("Add a model ID this key serves")).toHaveValue("typed");
    expect(screen.getByLabelText("Model")).toHaveValue("whisper-1");
  });

  it("re-seeds the model-ID list when the read itself changes", () => {
    const { rerender } = render(<AssistantSettingsPage />);
    expect(screen.getByText("model-a")).toBeInTheDocument();
    harness.llm = { data: { ...LLM, model_name: "model-b" } };
    rerender(<AssistantSettingsPage />);
    expect(screen.queryByText("model-a")).toBeNull();
    expect(screen.getByText("model-b")).toBeInTheDocument();
  });

  it("binds no shortcuts, drops or file inputs (PROF-027)", async () => {
    const user = userEvent.setup();
    const { container } = render(<AssistantSettingsPage />);
    expect(container.querySelectorAll('input[type="file"]')).toHaveLength(0);
    expect(container.querySelectorAll("[draggable='true']")).toHaveLength(0);
    await user.keyboard("{Control>}k{/Control}");
    expect(harness.saveLlm.mutate).not.toHaveBeenCalled();
    expect(harness.addServer.mutate).not.toHaveBeenCalled();
    expect(harness.toggleServer.mutate).not.toHaveBeenCalled();
    expect(harness.removeServer.mutate).not.toHaveBeenCalled();
  });

  it("renders an unregistered voice section without its stored key", () => {
    harness.voice = { data: { ...VOICE, enabled: false, has_api_key: false } };
    render(<AssistantSettingsPage />);
    expect(screen.queryByRole("heading", { name: "Voice dictation" })).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Tool servers (MCP)" })).toBeInTheDocument();
  });
});
