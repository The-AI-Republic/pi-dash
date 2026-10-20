// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Emblem upload dialog (PSET-007): a valid image runs the upload and hands
// back its URL; a refused type never starts one; a failed upload keeps the
// dialog open with the reason and applies nothing.
import { fireEvent, render, screen } from "@testing-library/react";
import * as React from "react";
import { describe, expect, it, vi } from "vitest";

import { EmblemUploadDialog } from "./EmblemUploadDialog.js";

function renderDialog(onUpload = vi.fn(async () => "/api/assets/emblem.png")) {
  const onUploaded = vi.fn();
  const onOpenChange = vi.fn();
  render(<EmblemUploadDialog open onOpenChange={onOpenChange} onUpload={onUpload} onUploaded={onUploaded} />);
  return { onUpload, onUploaded, onOpenChange };
}

function pick(file: File): void {
  fireEvent.change(screen.getByTestId("general-emblem-file"), { target: { files: [file] } });
}

describe("EmblemUploadDialog", () => {
  it("uploads a valid image and applies its URL", async () => {
    const { onUpload, onUploaded, onOpenChange } = renderDialog();
    const file = new File(["bytes"], "emblem.png", { type: "image/png" });
    pick(file);
    await vi.waitFor(() => expect(onUploaded).toHaveBeenCalledWith("/api/assets/emblem.png"));
    expect(onUpload).toHaveBeenCalledWith(file);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("refuses a non-image before uploading", () => {
    const { onUpload } = renderDialog();
    pick(new File(["<svg/>"], "emblem.svg", { type: "image/svg+xml" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Choose a PNG, JPEG, WebP or GIF image.");
    expect(onUpload).not.toHaveBeenCalled();
  });

  it("keeps the dialog open with the reason when the upload fails", async () => {
    const { onUploaded } = renderDialog(vi.fn(async () => Promise.reject(new Error("Upload refused"))));
    pick(new File(["bytes"], "emblem.png", { type: "image/png" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Upload refused");
    expect(onUploaded).not.toHaveBeenCalled();
  });

  it("falls back to its own message for a non-error rejection", async () => {
    renderDialog(vi.fn(async () => Promise.reject("no")));
    pick(new File(["bytes"], "emblem.png", { type: "image/png" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The emblem could not be uploaded.");
  });

  it("ignores a file picker dismissed without a file", () => {
    const { onUpload } = renderDialog();
    fireEvent.change(screen.getByTestId("general-emblem-file"), { target: { files: [] } });
    expect(onUpload).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
