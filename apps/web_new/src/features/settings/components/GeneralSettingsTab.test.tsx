// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// General settings tab (parity/specs/profile.md §6, PROF-011..014): the
// names form and its inline rules, avatar dialog wiring, the mail-gated
// email entry, and the self-managed danger zone.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const harness = vi.hoisted(() => ({
  me: undefined as Record<string, unknown> | undefined,
  instance: undefined as Record<string, unknown> | undefined,
  save: { mutate: vi.fn(), isPending: false },
  upload: { mutate: vi.fn(), isPending: false },
  storeAvatar: { mutate: vi.fn(), isPending: false },
  removeAvatar: { mutate: vi.fn(), isPending: false },
  deactivate: { mutate: vi.fn(), isPending: false },
  notifyError: vi.fn(),
}));

vi.mock("../../../core/session/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/session/index.js")>()),
  useMe: () => ({ data: harness.me }),
  useInstanceConfig: () => ({ data: harness.instance }),
}));

vi.mock("../api.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api.js")>()),
  useSaveProfile: () => harness.save,
  useUploadAvatar: () => harness.upload,
  useStoreAvatar: () => harness.storeAvatar,
  useRemoveAvatar: () => harness.removeAvatar,
}));

vi.mock("../../auth/index.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../auth/index.js")>()),
  useDeactivateAccount: () => harness.deactivate,
}));

vi.mock("../../../shared/shell/index.js", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../shared/shell/index.js")>();
  return { ...original, notifyError: harness.notifyError };
});

vi.mock("../../../core/api/client.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../core/api/client.js")>()),
  getAppClient: () => ({ baseUrl: "https://api.test" }),
}));

import { GeneralSettingsTab, GENERAL_SAVE_LABEL, GENERAL_SAVING_LABEL } from "./GeneralSettingsTab.js";

const ME = {
  id: "user-1",
  email: "ada@example.com",
  first_name: "Ada",
  last_name: "Lovelace",
  display_name: "ada",
  avatar: "/api/assets/v2/static/abc/",
  avatar_url: "/api/assets/v2/static/abc/",
};

function renderTab(): void {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={queryClient}>
      <GeneralSettingsTab />
    </QueryClientProvider>
  );
}

beforeEach(() => {
  harness.me = ME;
  // jsdom has no object URLs; the dialog only needs a stable preview.
  Object.defineProperty(URL, "createObjectURL", { configurable: true, value: () => "blob:preview" });
  harness.instance = { is_smtp_configured: false, is_self_managed: false };
  harness.save.mutate = vi.fn();
  harness.upload.mutate = vi.fn();
  harness.storeAvatar.mutate = vi.fn();
  harness.removeAvatar.mutate = vi.fn();
  harness.deactivate.mutate = vi.fn();
  harness.save.isPending = false;
  harness.notifyError.mockClear();
});

afterEach(() => vi.restoreAllMocks());

describe("GeneralSettingsTab", () => {
  it("holds a loading frame until the member row arrives", () => {
    harness.me = undefined;
    renderTab();
    expect(screen.getByTestId("general-settings-loading")).toHaveAttribute("aria-busy", "true");
    expect(screen.queryByTestId("general-settings-form")).not.toBeInTheDocument();
  });

  it("seeds the form and header from the member row with a locked address", () => {
    renderTab();
    expect(screen.getByLabelText("First name")).toHaveValue("Ada");
    expect(screen.getByLabelText("Last name")).toHaveValue("Lovelace");
    expect(screen.getByLabelText("Display name")).toHaveValue("ada");
    expect(screen.getByLabelText("Email")).toHaveValue("ada@example.com");
    expect(screen.getByLabelText("Email")).toBeDisabled();
    expect(screen.getByRole("img", { name: "Ada Lovelace" })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: "Ada Lovelace" }).querySelector("img")).toHaveAttribute(
      "src",
      "https://api.test/api/assets/v2/static/abc/"
    );
  });

  it("shows per-field inline errors and blocks the save", async () => {
    renderTab();
    const user = userEvent.setup();
    await user.clear(screen.getByLabelText("First name"));
    await user.clear(screen.getByLabelText("Display name"));
    await user.type(screen.getByLabelText("Last name"), "3");
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_LABEL }));
    expect(screen.getByText("Please enter first name")).toBeInTheDocument();
    expect(screen.getByText("Please enter display name")).toBeInTheDocument();
    expect(screen.getByText("Names can only contain letters, spaces, hyphens, and apostrophes")).toBeInTheDocument();
    expect(harness.save.mutate).not.toHaveBeenCalled();
  });

  it("rejects display names outside the display alphabet and over-long names", async () => {
    renderTab();
    const user = userEvent.setup();
    await user.clear(screen.getByLabelText("Display name"));
    await user.type(screen.getByLabelText("Display name"), "ada lovelace");
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_LABEL }));
    expect(
      screen.getByText("Display name can only contain letters, numbers, periods, hyphens, and underscores")
    ).toBeInTheDocument();

    // The field caps typing at fifty; a paste or prefill can still exceed it.
    fireEvent.change(screen.getByLabelText("Last name"), { target: { value: "x".repeat(51) } });
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_LABEL }));
    expect(screen.getByText("Keep this to 50 characters or fewer")).toBeInTheDocument();
    expect(harness.save.mutate).not.toHaveBeenCalled();
  });

  it("shows the saving label and blocks a second submit while the write runs", () => {
    harness.save.isPending = true;
    renderTab();
    expect(screen.getByRole("button", { name: GENERAL_SAVING_LABEL })).toBeDisabled();
  });

  it("saves the names and accepts an empty last name", async () => {
    const user = userEvent.setup();
    renderTab();
    await user.clear(screen.getByLabelText("Last name"));
    await user.click(screen.getByRole("button", { name: GENERAL_SAVE_LABEL }));
    expect(harness.save.mutate).toHaveBeenCalledWith({
      first_name: "Ada",
      last_name: "",
      display_name: "ada",
    });
  });

  it("opens the avatar dialog and uploads a picked image into the row", async () => {
    const user = userEvent.setup();
    renderTab();
    await user.click(screen.getByRole("button", { name: "Change profile photo" }));
    const dialog = screen.getByRole("dialog", { name: "Upload Image" });
    const file = new File(["x"], "photo.png", { type: "image/png" });
    await user.upload(within(dialog).getByTestId("avatar-file-input"), file);
    await user.click(within(dialog).getByRole("button", { name: "Upload & Save" }));
    expect(harness.upload.mutate).toHaveBeenCalledWith(file, expect.anything());

    const uploadOptions = harness.upload.mutate.mock.calls[0]?.[1] as { onSuccess: (url: string) => void };
    harness.storeAvatar.mutate.mockImplementation((_url: string, options: { onSuccess: () => void }) => {
      options.onSuccess();
    });
    uploadOptions.onSuccess("/api/assets/v2/static/new/");
    expect(harness.storeAvatar.mutate).toHaveBeenCalledWith("/api/assets/v2/static/new/", expect.anything());
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Upload Image" })).not.toBeInTheDocument());
  });

  it("keeps the dialog open with an inline line when storing fails, and toasts an upload failure", async () => {
    const user = userEvent.setup();
    renderTab();
    await user.click(screen.getByRole("button", { name: "Change profile photo" }));
    const dialog = screen.getByRole("dialog", { name: "Upload Image" });
    const file = new File(["x"], "photo.png", { type: "image/png" });
    await user.upload(within(dialog).getByTestId("avatar-file-input"), file);
    await user.click(within(dialog).getByRole("button", { name: "Upload & Save" }));

    const uploadOptions = harness.upload.mutate.mock.calls[0]?.[1] as {
      onSuccess: (url: string) => void;
      onError: () => void;
    };
    uploadOptions.onError();
    expect(harness.notifyError).toHaveBeenCalledWith("Could not upload that image", "Try a different file.");
    expect(screen.getByRole("dialog", { name: "Upload Image" })).toBeInTheDocument();

    harness.storeAvatar.mutate.mockImplementation((_url: string, options: { onError: () => void }) => {
      options.onError();
    });
    uploadOptions.onSuccess("/api/assets/v2/static/new/");
    await waitFor(() => expect(screen.getByText("Try a different file.")).toBeInTheDocument());
  });

  it("removes the stored photo through the asset route", async () => {
    const user = userEvent.setup();
    renderTab();
    await user.click(screen.getByRole("button", { name: "Change profile photo" }));
    const dialog = screen.getByRole("dialog", { name: "Upload Image" });
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));
    expect(harness.removeAvatar.mutate).toHaveBeenCalledWith({ kind: "asset", assetId: "abc" }, expect.anything());
    const removeOptions = harness.removeAvatar.mutate.mock.calls[0]?.[1] as { onSuccess: () => void };
    removeOptions.onSuccess();
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Upload Image" })).not.toBeInTheDocument());
  });

  it("hides the email entry until the instance has mail configured", async () => {
    const user = userEvent.setup();
    renderTab();
    expect(screen.queryByRole("button", { name: "Change email" })).not.toBeInTheDocument();
    cleanup();
    harness.instance = { is_smtp_configured: true, is_self_managed: false };
    renderTab();
    await user.click(screen.getByRole("button", { name: "Change email" }));
    expect(screen.getByRole("dialog", { name: "Change email" })).toBeInTheDocument();
  });

  it("hides the danger zone on managed instances and deactivates after confirming", async () => {
    const user = userEvent.setup();
    renderTab();
    expect(screen.queryByRole("heading", { name: "Deactivate account" })).not.toBeInTheDocument();
    cleanup();
    harness.instance = { is_smtp_configured: false, is_self_managed: true };
    renderTab();
    await user.click(screen.getAllByRole("button", { name: "Deactivate account" })[0] as HTMLElement);
    const dialog = screen.getByRole("dialog", { name: "Deactivate your account" });
    await user.click(within(dialog).getByRole("button", { name: "Deactivate account" }));
    expect(harness.deactivate.mutate).toHaveBeenCalledWith(undefined, expect.anything());
  });

  it("signs out to the returned landing after deactivation succeeds", async () => {
    const assign = vi.fn();
    const original = window.location;
    Object.defineProperty(window, "location", { configurable: true, value: { ...original, assign } });
    try {
      harness.instance = { is_smtp_configured: false, is_self_managed: true };
      const user = userEvent.setup();
      renderTab();
      await user.click(screen.getAllByRole("button", { name: "Deactivate account" })[0] as HTMLElement);
      await user.click(
        within(screen.getByRole("dialog", { name: "Deactivate your account" })).getByRole("button", {
          name: "Deactivate account",
        })
      );
      const options = harness.deactivate.mutate.mock.calls[0]?.[1] as { onSuccess: (landing: string) => void };
      options.onSuccess("/accounts/sign-in/");
      expect(assign).toHaveBeenCalledWith("/accounts/sign-in/");
    } finally {
      Object.defineProperty(window, "location", { configurable: true, value: original });
    }
  });
});
