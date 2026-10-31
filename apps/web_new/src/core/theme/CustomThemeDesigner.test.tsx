// @vitest-environment jsdom
// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import * as React from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { CustomPalette } from "./theme.js";

import {
  CustomThemeDesigner,
  normalizeHexColor,
  parseThemeConfigFile,
  serializeThemeConfig,
} from "./CustomThemeDesigner.js";

describe("theme config helpers", () => {
  it("normalizes hex colors strictly", () => {
    expect(normalizeHexColor("#3F76FF")).toBe("#3f76ff");
    expect(normalizeHexColor("#abc")).toBe("#aabbcc");
    expect(normalizeHexColor("  #abc  ")).toBe("#aabbcc");
    expect(normalizeHexColor("red")).toBeNull();
    expect(normalizeHexColor("#abcd")).toBeNull();
    expect(normalizeHexColor("")).toBeNull();
  });

  it("parses config files and rejects junk", () => {
    expect(
      parseThemeConfigFile(JSON.stringify({ primary: "#112233", background: "#445566", darkPalette: true }))
    ).toEqual({ palette: { primary: "#112233", background: "#445566", darkPalette: true } });
    expect(parseThemeConfigFile(JSON.stringify({ primary: "#112233", background: "#445566" }))).toEqual({
      palette: { primary: "#112233", background: "#445566", darkPalette: false },
    });
    expect(parseThemeConfigFile("nope")).toEqual({ error: "That file is not valid JSON." });
    expect(parseThemeConfigFile("[]")).toEqual({ error: "That file is not a theme config." });
    expect(parseThemeConfigFile("42")).toEqual({ error: "That file is not a theme config." });
    expect(parseThemeConfigFile("null")).toEqual({ error: "That file is not a theme config." });
    expect(parseThemeConfigFile(JSON.stringify({ primary: "red" }))).toEqual({
      error: "The config needs brand and background hex colors.",
    });
    expect(parseThemeConfigFile(JSON.stringify({ primary: "#112233" }))).toEqual({
      error: "The config needs brand and background hex colors.",
    });
    expect(parseThemeConfigFile(JSON.stringify({ background: "#445566" }))).toEqual({
      error: "The config needs brand and background hex colors.",
    });
    expect(serializeThemeConfig({ primary: "#a", background: "#b", darkPalette: false })).toBe(
      JSON.stringify({ primary: "#a", background: "#b", darkPalette: false }, null, 2)
    );
  });
});

describe("CustomThemeDesigner", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  function renderDesigner(
    onSave: (palette: CustomPalette) => void = () => undefined,
    props: { saving?: boolean } = {}
  ) {
    const save = vi.fn(onSave);
    render(
      <CustomThemeDesigner
        initial={{ primary: "#3f76ff", background: "#ffffff", darkPalette: false }}
        onSave={save}
        {...props}
      />
    );
    return save;
  }

  it("saves normalized colors with the chosen base", async () => {
    const user = userEvent.setup();
    const save = renderDesigner();
    await user.click(screen.getByRole("radio", { name: "Dark mode" }));
    const brand = screen.getByLabelText("Primary color");
    await user.clear(brand);
    await user.type(brand, "#ABC");
    await user.click(screen.getByRole("button", { name: "Set theme" }));
    expect(save).toHaveBeenCalledWith({ primary: "#aabbcc", background: "#ffffff", darkPalette: true });
  });

  it("blocks saves with invalid colors", async () => {
    const user = userEvent.setup();
    const save = renderDesigner();
    const brand = screen.getByLabelText("Primary color");
    await user.clear(brand);
    await user.type(brand, "red");
    await user.click(screen.getByRole("button", { name: "Set theme" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Enter hex colors like #3f76ff.");
    expect(save).not.toHaveBeenCalled();
  });

  it("blocks saves when the background is invalid", async () => {
    const user = userEvent.setup();
    const save = renderDesigner();
    const background = screen.getByLabelText("Background color");
    await user.clear(background);
    await user.type(background, "red");
    await user.click(screen.getByRole("button", { name: "Set theme" }));
    expect(screen.getByRole("alert")).toHaveTextContent("Enter hex colors like #3f76ff.");
    expect(save).not.toHaveBeenCalled();
  });

  it("disables the save while persisting", () => {
    renderDesigner(undefined, { saving: true });
    expect(screen.getByRole("button", { name: "Set theme" })).toBeDisabled();
  });

  it("previews every complete color pair with the chosen base", async () => {
    const user = userEvent.setup();
    const onPreview = vi.fn();
    render(
      <CustomThemeDesigner
        initial={{ primary: "#3f76ff", background: "#ffffff", darkPalette: false }}
        onSave={() => undefined}
        onPreview={onPreview}
      />
    );
    await waitFor(() =>
      expect(onPreview).toHaveBeenLastCalledWith({ primary: "#3f76ff", background: "#ffffff", darkPalette: false })
    );
    await user.click(screen.getByRole("radio", { name: "Dark mode" }));
    expect(onPreview).toHaveBeenLastCalledWith({ primary: "#3f76ff", background: "#ffffff", darkPalette: true });
    await user.clear(screen.getByLabelText("Primary color"));
    await user.type(screen.getByLabelText("Primary color"), "#ABCDEF");
    expect(onPreview).toHaveBeenLastCalledWith({ primary: "#abcdef", background: "#ffffff", darkPalette: true });
  });

  it("leaves the last preview in place while a color is incomplete", async () => {
    const user = userEvent.setup();
    const onPreview = vi.fn();
    render(
      <CustomThemeDesigner
        initial={{ primary: "#3f76ff", background: "#ffffff", darkPalette: false }}
        onSave={() => undefined}
        onPreview={onPreview}
      />
    );
    await waitFor(() => expect(onPreview).toHaveBeenCalledTimes(1));
    await user.clear(screen.getByLabelText("Background color"));
    await user.type(screen.getByLabelText("Background color"), "nope");
    expect(onPreview).toHaveBeenCalledTimes(1);
  });

  it("syncs the color pickers with the text fields", async () => {
    const user = userEvent.setup();
    renderDesigner();
    const picker = screen.getByLabelText("Primary color picker") as HTMLInputElement;
    expect(picker.value).toBe("#3f76ff");
    await user.clear(screen.getByLabelText("Primary color"));
    await user.type(screen.getByLabelText("Primary color"), "junk");
    expect((screen.getByLabelText("Primary color picker") as HTMLInputElement).value).toBe("#000000");
  });

  it("edits through the background field and both pickers", async () => {
    const user = userEvent.setup();
    renderDesigner();
    await user.clear(screen.getByLabelText("Background color"));
    await user.type(screen.getByLabelText("Background color"), "junk");
    expect((screen.getByLabelText("Background color picker") as HTMLInputElement).value).toBe("#000000");
    fireEvent.change(screen.getByLabelText("Primary color picker"), { target: { value: "#112233" } });
    expect(screen.getByLabelText("Primary color")).toHaveValue("#112233");
    fireEvent.change(screen.getByLabelText("Background color picker"), { target: { value: "#445566" } });
    expect(screen.getByLabelText("Background color")).toHaveValue("#445566");
  });

  it("opens the file picker from the import button", () => {
    renderDesigner();
    const input = screen.getByLabelText("Import theme config file") as HTMLInputElement;
    const click = vi.spyOn(input, "click").mockImplementation(() => undefined);
    fireEvent.click(screen.getByRole("button", { name: "Import config" }));
    expect(click).toHaveBeenCalledTimes(1);
  });

  it("downloads the current form as a config file", async () => {
    const user = userEvent.setup();
    renderDesigner();
    const createObjectURL = vi.fn(() => "blob:theme");
    const revokeObjectURL = vi.fn();
    vi.stubGlobal("URL", { createObjectURL, revokeObjectURL });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    await user.click(screen.getByRole("button", { name: "Download config" }));
    expect(createObjectURL).toHaveBeenCalled();
    expect(click).toHaveBeenCalled();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:theme");
  });

  it("imports a config file into the form", async () => {
    renderDesigner();
    const file = new File(
      [JSON.stringify({ primary: "#112233", background: "#445566", darkPalette: true })],
      "theme.json",
      { type: "application/json" }
    );
    fireEvent.change(screen.getByLabelText("Import theme config file"), { target: { files: [file] } });
    await waitFor(() => expect(screen.getByLabelText("Primary color")).toHaveValue("#112233"));
    expect(screen.getByLabelText("Background color")).toHaveValue("#445566");
    expect(screen.getByRole("radio", { name: "Dark mode" })).toBeChecked();
  });

  it("reports invalid imports inline and ignores empty picks", async () => {
    renderDesigner();
    const input = screen.getByLabelText("Import theme config file") as HTMLInputElement;
    const bad = new File(["nope"], "bad.json", { type: "application/json" });
    fireEvent.change(input, { target: { files: [bad] } });
    expect(await screen.findByRole("alert")).toHaveTextContent("That file is not valid JSON.");
    // Choosing no file (dialog dismissed) leaves the form untouched.
    fireEvent.change(input, { target: { files: [] } });
    expect(screen.getByLabelText("Primary color")).toHaveValue("#3f76ff");
  });

  it("reports unreadable files inline", async () => {
    renderDesigner();
    class FailingReader {
      error: Error | null = null;
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      readAsText(): void {
        queueMicrotask(() => this.onerror?.());
      }
    }
    vi.stubGlobal("FileReader", FailingReader);
    const file = new File([JSON.stringify({ primary: "#112233", background: "#445566" })], "t.json", {
      type: "application/json",
    });
    fireEvent.change(screen.getByLabelText("Import theme config file"), { target: { files: [file] } });
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not read the file.");
  });

  it("reports reader failures without leaking reader internals", async () => {
    renderDesigner();
    class FailingReader {
      error: Error | null = new Error("disk gone");
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      readAsText(): void {
        queueMicrotask(() => this.onerror?.());
      }
    }
    vi.stubGlobal("FileReader", FailingReader);
    const file = new File([JSON.stringify({ primary: "#112233", background: "#445566" })], "t.json", {
      type: "application/json",
    });
    fireEvent.change(screen.getByLabelText("Import theme config file"), { target: { files: [file] } });
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not read the file.");
    expect(screen.queryByText(/disk gone/)).toBeNull();
  });

  it("treats a non-text read result as an empty config", async () => {
    renderDesigner();
    class BinaryReader {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      result: ArrayBuffer = new ArrayBuffer(8);
      readAsText(): void {
        queueMicrotask(() => this.onload?.());
      }
    }
    vi.stubGlobal("FileReader", BinaryReader);
    const file = new File(["x"], "t.json", { type: "application/json" });
    fireEvent.change(screen.getByLabelText("Import theme config file"), { target: { files: [file] } });
    expect(await screen.findByRole("alert")).toHaveTextContent("That file is not valid JSON.");
  });
});
