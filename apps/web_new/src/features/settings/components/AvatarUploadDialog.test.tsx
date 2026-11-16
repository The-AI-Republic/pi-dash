// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Avatar dialog (parity/specs/profile.md §6, PROF-012): format and size
// refusals inline, the clean reopen, and the remove action's availability.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AVATAR_MESSAGES, AVATAR_MAX_BYTES } from "../lib/avatar.js";
import { AvatarUploadDialog } from "./AvatarUploadDialog.js";

function setup(overrides: Partial<React.ComponentProps<typeof AvatarUploadDialog>> = {}) {
  const handlers = {
    onOpenChange: vi.fn(),
    onUpload: vi.fn(),
    onRemove: vi.fn(),
  };
  const props: React.ComponentProps<typeof AvatarUploadDialog> = {
    open: true,
    currentSrc: null,
    uploading: false,
    removing: false,
    ...handlers,
    ...overrides,
  };
  const view = render(<AvatarUploadDialog {...props} />);
  return { ...handlers, ...view };
}

function pick(file: File): void {
  const input = screen.getByTestId("avatar-file-input");
  Object.defineProperty(input, "files", { configurable: true, value: [file] });
  input.dispatchEvent(new Event("change", { bubbles: true }));
}

beforeEach(() => {
  Object.defineProperty(URL, "createObjectURL", { configurable: true, value: () => "blob:preview" });
});

describe("AvatarUploadDialog", () => {
  it("offers the prompt, the format hint and the accept list", () => {
    setup();
    const dialog = screen.getByRole("dialog", { name: "Upload Image" });
    expect(within(dialog).getByRole("button", { name: /Drag & drop an image/ })).toBeInTheDocument();
    expect(within(dialog).getByText("JPEG, PNG, WebP or GIF, up to 5 MB.")).toBeInTheDocument();
    expect(within(dialog).getByTestId("avatar-file-input")).toHaveAttribute(
      "accept",
      "image/jpeg,image/png,image/webp,image/gif"
    );
  });

  it("refuses a non-image inline and leaves the upload disabled", () => {
    setup();
    pick(new File(["x"], "notes.txt", { type: "text/plain" }));
    expect(screen.getByText(AVATAR_MESSAGES.invalidFormat)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeDisabled();
  });

  it("refuses a file above five megabytes", () => {
    setup();
    pick(new File(["x"], "big.png", { type: "image/png" }));
    const input = screen.getByTestId("avatar-file-input");
    Object.defineProperty(input, "files", {
      configurable: true,
      value: [new File(["x"], "big.png", { type: "image/png" })],
    });
    const big = new File(["x"], "big.png", { type: "image/png" });
    Object.defineProperty(big, "size", { value: AVATAR_MAX_BYTES + 1 });
    Object.defineProperty(input, "files", { configurable: true, value: [big] });
    input.dispatchEvent(new Event("change", { bubbles: true }));
    expect(screen.getByText(AVATAR_MESSAGES.tooLarge)).toBeInTheDocument();
  });

  it("previews a valid pick and hands it to the upload", async () => {
    const { onUpload } = setup();
    const file = new File(["x"], "photo.png", { type: "image/png" });
    pick(file);
    expect(screen.getByRole("img", { name: "Selected profile photo" })).toHaveAttribute("src", "blob:preview");
    await userEvent.click(screen.getByRole("button", { name: "Upload & Save" }));
    expect(onUpload).toHaveBeenCalledWith(file);
  });

  it("clears a pick when the picker is emptied", () => {
    setup();
    pick(new File(["x"], "photo.png", { type: "image/png" }));
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeEnabled();
    const input = screen.getByTestId("avatar-file-input");
    Object.defineProperty(input, "files", { configurable: true, value: [] });
    input.dispatchEvent(new Event("change", { bubbles: true }));
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Drag & drop an image/ })).toBeInTheDocument();
  });

  it("enables remove only when a photo is stored", async () => {
    const { onRemove, rerender } = setup();
    expect(screen.getByRole("button", { name: "Remove" })).toBeDisabled();
    rerender(
      <AvatarUploadDialog
        open
        currentSrc="/api/assets/v2/static/abc/"
        uploading={false}
        removing={false}
        onOpenChange={vi.fn()}
        onUpload={vi.fn()}
        onRemove={onRemove}
      />
    );
    expect(screen.getByRole("img", { name: "Selected profile photo" })).toHaveAttribute(
      "src",
      "/api/assets/v2/static/abc/"
    );
    const remove = screen.getByRole("button", { name: "Remove" });
    expect(remove).toBeEnabled();
    await userEvent.click(remove);
    expect(onRemove).toHaveBeenCalledTimes(1);
  });

  it("cancels through the dialog's close request", async () => {
    const { onOpenChange } = setup();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("holds every control while an upload or removal runs", async () => {
    const { onOpenChange } = setup({ uploading: true, currentSrc: "/api/assets/v2/static/abc/" });
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Close dialog" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("starts clean when it reopens", () => {
    const view = setup();
    pick(new File(["x"], "photo.png", { type: "image/png" }));
    expect(screen.getByRole("button", { name: "Upload & Save" })).toBeEnabled();
    view.rerender(
      <AvatarUploadDialog
        open={false}
        currentSrc={null}
        uploading={false}
        removing={false}
        onOpenChange={vi.fn()}
        onUpload={vi.fn()}
        onRemove={vi.fn()}
      />
    );
    view.rerender(
      <AvatarUploadDialog
        open
        currentSrc={null}
        uploading={false}
        removing={false}
        onOpenChange={vi.fn()}
        onUpload={vi.fn()}
        onRemove={vi.fn()}
      />
    );
    expect(screen.getByRole("button", { name: /Drag & drop an image/ })).toBeInTheDocument();
    expect(screen.queryByText(AVATAR_MESSAGES.invalidFormat)).not.toBeInTheDocument();
  });

  it("ignores the upload action while no file is picked", async () => {
    const { onUpload } = setup();
    const button = screen.getByRole("button", { name: "Upload & Save" });
    expect(button).toBeDisabled();
    await userEvent.click(button);
    expect(onUpload).not.toHaveBeenCalled();
  });
});
