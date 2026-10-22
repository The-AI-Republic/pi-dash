// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Voice-dictation form (parity/specs/profile.md §10, PROF-022): the three
// required fields, the saved-key placeholder, and the kill-switch shapes —
// gone with no stored key, removal-only with one, editable otherwise.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import type { SttConfig } from "@pidash/api-client";

import {
  SAVED_KEY_PLACEHOLDER,
  STT_BASE_URL_PLACEHOLDER,
  STT_KEY_PLACEHOLDER_EMPTY,
  STT_MODEL_PLACEHOLDER,
  VOICE_SECTION_HEADING,
  VOICE_UNAVAILABLE_NOTE,
} from "../lib/copy.js";
import { VoiceForm, emptyVoiceForm, voiceFormFrom, type VoiceFormValues } from "./VoiceForm.js";

function config(overrides: Partial<SttConfig> = {}): SttConfig {
  return {
    base_url: "https://stt.test/v1",
    model_name: "whisper-1",
    has_api_key: false,
    last_verified_at: null,
    enabled: true,
    ...overrides,
  };
}

function renderForm(values: VoiceFormValues, overrides: Partial<React.ComponentProps<typeof VoiceForm>> = {}) {
  const props: React.ComponentProps<typeof VoiceForm> = {
    config: config(),
    values,
    onChange: vi.fn(),
    onSave: vi.fn(),
    onTest: vi.fn(),
    onRemove: vi.fn(),
    saving: false,
    testing: false,
    removing: false,
    ...overrides,
  };
  const result = render(<VoiceForm {...props} />);
  return { props, result };
}

describe("VoiceForm", () => {
  it("seeds from a stored config with a blank key", () => {
    expect(voiceFormFrom(undefined)).toEqual(emptyVoiceForm());
    expect(voiceFormFrom(config())).toEqual({ baseUrl: "https://stt.test/v1", model: "whisper-1", key: "" });
  });

  it("shows the three placeholders and the blank-key hint", () => {
    renderForm(voiceFormFrom(undefined));
    expect(screen.getByRole("heading", { name: VOICE_SECTION_HEADING })).toBeInTheDocument();
    expect(screen.getByPlaceholderText(STT_BASE_URL_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(STT_MODEL_PLACEHOLDER)).toBeInTheDocument();
    expect(screen.getByPlaceholderText(STT_KEY_PLACEHOLDER_EMPTY)).toBeInTheDocument();
  });

  it("gates save on endpoint, model and a typed or stored key", () => {
    renderForm({ baseUrl: "", model: "whisper-1", key: "k" });
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Test connection" })).toBeDisabled();
    renderForm({ baseUrl: "u", model: "", key: "k" });
    expect(screen.getAllByRole("button", { name: "Save" })[1]).toBeDisabled();
    renderForm({ baseUrl: "u", model: "m", key: "" });
    expect(screen.getAllByRole("button", { name: "Save" })[2]).toBeDisabled();
    renderForm({ baseUrl: "u", model: "m", key: "k" }, { config: config({ has_api_key: true }) });
    expect(screen.getAllByRole("button", { name: "Test connection" })[3]).toBeEnabled();
    expect(screen.getAllByRole("button", { name: "Save" })[3]).toBeEnabled();
  });

  it("saves with the key left blank while one is stored", () => {
    renderForm({ baseUrl: "u", model: "m", key: "" }, { config: config({ has_api_key: true }) });
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });

  it("swaps the key placeholder once a key is stored and offers removal", () => {
    renderForm(voiceFormFrom(config({ has_api_key: true })), { config: config({ has_api_key: true }) });
    expect(screen.getByLabelText("API key")).toHaveAttribute("placeholder", SAVED_KEY_PLACEHOLDER);
    expect(screen.getByRole("button", { name: "Remove" })).toBeInTheDocument();
  });

  it("reports typed values and fires the actions", async () => {
    const user = userEvent.setup();
    const { props } = renderForm({ baseUrl: "", model: "", key: "" }, { config: config({ has_api_key: true }) });
    await user.type(screen.getByLabelText("Base URL"), "b");
    expect(props.onChange).toHaveBeenLastCalledWith({ baseUrl: "b", model: "", key: "" });
    await user.type(screen.getByLabelText("Model"), "w");
    expect(props.onChange).toHaveBeenLastCalledWith({ baseUrl: "", model: "w", key: "" });
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(props.onTest).toHaveBeenCalledTimes(1);
    expect(props.onRemove).toHaveBeenCalledTimes(1);
  });

  it("marks in-flight controls busy", () => {
    renderForm(
      { baseUrl: "u", model: "m", key: "k" },
      { config: config({ has_api_key: true }), saving: true, testing: true, removing: true }
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Test connection" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Remove" })).toBeDisabled();
  });

  it("disappears entirely when the instance switch is off and nothing is stored", () => {
    const { result } = renderForm(emptyVoiceForm(), { config: config({ enabled: false, has_api_key: false }) });
    expect(result.container).toBeEmptyDOMElement();
  });

  it("keeps only removal, with the unavailable note, when the switch is off with a stored key", async () => {
    const user = userEvent.setup();
    const { props } = renderForm(emptyVoiceForm(), { config: config({ enabled: false, has_api_key: true }) });
    expect(screen.getByRole("heading", { name: VOICE_SECTION_HEADING })).toBeInTheDocument();
    expect(screen.getByText(VOICE_UNAVAILABLE_NOTE)).toBeInTheDocument();
    expect(screen.queryByLabelText("Base URL")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Test connection" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove" }));
    expect(props.onRemove).toHaveBeenCalledTimes(1);
  });

  it("stays editable when the switch field is absent", () => {
    renderForm(voiceFormFrom(config()), { config: { ...config(), enabled: undefined } });
    expect(screen.getByLabelText("Base URL")).toBeInTheDocument();
  });

  it("shows the verification stamp when verified", () => {
    renderForm(voiceFormFrom(config()), { config: config({ last_verified_at: "2026-10-19T12:00:00Z" }) });
    expect(screen.getByText(/Last verified:/)).toBeInTheDocument();
  });

  it("exposes the dictation deep-link anchor in both shapes", () => {
    const { result } = renderForm(voiceFormFrom(config()));
    expect(screen.getByTestId("assistant-voice")).toHaveAttribute("id", "voice-dictation");
    result.unmount();
    renderForm(emptyVoiceForm(), { config: config({ enabled: false, has_api_key: true }) });
    expect(screen.getByTestId("assistant-voice")).toHaveAttribute("id", "voice-dictation");
  });
});
