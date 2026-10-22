/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SWRConfig } from "swr";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// vi.mock is hoisted above imports; vi.hoisted keeps the spies shared.
const { getSTTConfig, putSTTConfig, deleteSTTConfig, testSTTConfig, setToast } = vi.hoisted(() => ({
  getSTTConfig: vi.fn(),
  putSTTConfig: vi.fn(),
  deleteSTTConfig: vi.fn(),
  testSTTConfig: vi.fn(),
  setToast: vi.fn(),
}));

vi.mock("@pi-dash/services", () => ({
  AssistantService: class {
    getSTTConfig = getSTTConfig;
    putSTTConfig = putSTTConfig;
    deleteSTTConfig = deleteSTTConfig;
    testSTTConfig = testSTTConfig;
  },
}));

vi.mock("@pi-dash/propel/toast", () => ({
  TOAST_TYPE: { ERROR: "ERROR", SUCCESS: "SUCCESS", INFO: "INFO" },
  setToast,
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
import {
  DictationSettings,
  DICTATION_SETTINGS_ANCHOR,
} from "@/components/settings/profile/content/pages/dictation-settings";

function renderSettings() {
  return render(
    <SWRConfig value={{ provider: () => new Map(), dedupingInterval: 0 }}>
      <DictationSettings />
    </SWRConfig>
  );
}

describe("DictationSettings", () => {
  beforeEach(() => {
    getSTTConfig.mockReset();
    putSTTConfig.mockReset();
    deleteSTTConfig.mockReset();
    testSTTConfig.mockReset();
    setToast.mockReset();
  });
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("AC1: renders the Voice dictation section with Base URL, Model, and API key inputs", async () => {
    getSTTConfig.mockRejectedValue({ detail: "not found" }); // unconfigured
    const { container } = renderSettings();

    expect(screen.getByRole("heading", { name: "Voice dictation" })).toBeInTheDocument();
    expect(screen.getByText("Base URL")).toBeInTheDocument();
    expect(screen.getByText("Model")).toBeInTheDocument();
    expect(screen.getByText("API key")).toBeInTheDocument();
    // Section is anchored for the composer deep-link (sub-issue #5).
    expect(container.querySelector(`#${DICTATION_SETTINGS_ANCHOR}`)).not.toBeNull();
    expect(DICTATION_SETTINGS_ANCHOR).toBe("voice-dictation");
  });

  it("AC2: shows the explicit privacy copy", async () => {
    getSTTConfig.mockRejectedValue({ detail: "not found" });
    renderSettings();
    expect(
      screen.getByText("Your audio is sent directly to the endpoint you configure here and is not stored by Pi Dash.")
    ).toBeInTheDocument();
  });

  it("AC3: with no key saved, Save is gated on Base URL + Model, Test is disabled, Remove hidden", async () => {
    getSTTConfig.mockRejectedValue({ detail: "not found" });
    renderSettings();

    const save = screen.getByRole("button", { name: "Save" });
    const test = screen.getByRole("button", { name: "Test connection" });

    // Both fields empty → Save disabled.
    expect(save).toBeDisabled();
    // No key saved → Test disabled.
    expect(test).toBeDisabled();
    // No key saved → Remove not rendered.
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();

    // Fill only Base URL → still disabled (Model empty).
    await userEvent.type(screen.getByPlaceholderText("https://api.openai.com/v1"), "https://api.openai.com/v1");
    expect(save).toBeDisabled();

    // Fill Model too → enabled.
    await userEvent.type(screen.getByPlaceholderText("whisper-1"), "whisper-1");
    expect(save).toBeEnabled();
  });

  it("AC4: Save posts {base_url, model_name, api_key}, clears the key, and toasts success", async () => {
    getSTTConfig.mockResolvedValue({ base_url: "", model_name: "", has_api_key: false, last_verified_at: null });
    putSTTConfig.mockResolvedValue({ base_url: "u", model_name: "m", has_api_key: true, last_verified_at: null });
    renderSettings();

    await userEvent.type(screen.getByPlaceholderText("https://api.openai.com/v1"), "https://stt.example/v1");
    await userEvent.type(screen.getByPlaceholderText("whisper-1"), "whisper-1");
    const keyInput = screen.getByPlaceholderText("Your transcription API key");
    await userEvent.type(keyInput, "sk-secret");

    await userEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(putSTTConfig).toHaveBeenCalledTimes(1));
    expect(putSTTConfig).toHaveBeenCalledWith({
      base_url: "https://stt.example/v1",
      model_name: "whisper-1",
      api_key: "sk-secret",
    });
    // Key field cleared after save.
    await waitFor(() => expect((keyInput as HTMLInputElement).value).toBe(""));
    expect(setToast).toHaveBeenCalledWith(expect.objectContaining({ type: "SUCCESS", title: "Saved" }));
  });

  it("AC5a: Test connection toasts success on {ok:true}", async () => {
    getSTTConfig.mockResolvedValue({ base_url: "u", model_name: "m", has_api_key: true, last_verified_at: null });
    testSTTConfig.mockResolvedValue({ ok: true });
    renderSettings();

    const test = await screen.findByRole("button", { name: "Test connection" });
    await waitFor(() => expect(test).toBeEnabled());
    await userEvent.click(test);

    await waitFor(() => expect(testSTTConfig).toHaveBeenCalledTimes(1));
    expect(setToast).toHaveBeenCalledWith(expect.objectContaining({ type: "SUCCESS", title: "Connection OK" }));
  });

  it("AC5b: Test connection toasts the returned error_code on {ok:false}", async () => {
    getSTTConfig.mockResolvedValue({ base_url: "u", model_name: "m", has_api_key: true, last_verified_at: null });
    testSTTConfig.mockResolvedValue({ ok: false, error_code: "auth_failed" });
    renderSettings();

    const test = await screen.findByRole("button", { name: "Test connection" });
    await waitFor(() => expect(test).toBeEnabled());
    await userEvent.click(test);

    await waitFor(() =>
      expect(setToast).toHaveBeenCalledWith(
        expect.objectContaining({ type: "ERROR", title: "Connection failed", message: "auth_failed" })
      )
    );
  });

  it("AC6: with a saved key, Test/Remove render and the API-key placeholder shows the saved state; Remove deletes and clears", async () => {
    getSTTConfig.mockResolvedValue({ base_url: "u", model_name: "m", has_api_key: true, last_verified_at: null });
    deleteSTTConfig.mockResolvedValue(undefined);
    renderSettings();

    // Saved-state placeholder.
    const keyInput = await screen.findByPlaceholderText("•••• (saved) — enter to replace");
    expect(keyInput).toBeInTheDocument();

    const remove = await screen.findByRole("button", { name: "Remove" });
    await userEvent.click(remove);

    await waitFor(() => expect(deleteSTTConfig).toHaveBeenCalledTimes(1));
    expect(setToast).toHaveBeenCalledWith(expect.objectContaining({ type: "INFO", title: "Removed" }));
  });
});
