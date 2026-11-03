// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Custom palette designer (shell spec, SHELL-095). Edits the brand and
// background colors plus the light/dark base behind the custom theme,
// saves through the caller's persist, and round-trips the palette as a
// JSON config file (import validates before it fills the form). The
// profile area mounts this beside its appearance rows; the palette's
// Custom theme option is owned by the palette entries slice.

import { Button, Input } from "@pidash/kit";
import * as React from "react";

import type { CustomPalette } from "./theme.js";

const HEX_PATTERN = /^#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{3})$/;

/** Normalize three-digit hex to six digits; returns null when invalid. */
export function normalizeHexColor(raw: string): string | null {
  const value = raw.trim();
  if (!HEX_PATTERN.test(value)) return null;
  if (value.length === 4) {
    const [, r, g, b] = value;
    return `#${r}${r}${g}${g}${b}${b}`.toLowerCase();
  }
  return value.toLowerCase();
}

/** Parse an imported config file; null (with a reason) when unusable. */
export function parseThemeConfigFile(text: string): { palette: CustomPalette } | { error: string } {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text) as unknown;
  } catch {
    return { error: "That file is not valid JSON." };
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return { error: "That file is not a theme config." };
  }
  const record = parsed as Record<string, unknown>;
  const primary = typeof record["primary"] === "string" ? normalizeHexColor(record["primary"]) : null;
  const background = typeof record["background"] === "string" ? normalizeHexColor(record["background"]) : null;
  if (primary === null || background === null) {
    return { error: "The config needs brand and background hex colors." };
  }
  return { palette: { primary, background, darkPalette: record["darkPalette"] === true } };
}

/** Serialize the form for config download. */
export function serializeThemeConfig(palette: CustomPalette): string {
  return JSON.stringify(palette, null, 2);
}

/** Read a picked file as text (FileReader works everywhere File does). */
function readFileText(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(typeof reader.result === "string" ? reader.result : "");
    reader.onerror = () => reject(reader.error ?? new Error("Could not read the file."));
    reader.readAsText(file);
  });
}

export interface CustomThemeDesignerProps {
  /** The saved palette the form starts from. */
  initial: CustomPalette;
  /** True while the save write is in flight. */
  saving?: boolean;
  /** Persist the palette (apply + profile write live with the caller). */
  onSave: (palette: CustomPalette) => void;
  /**
   * Applied while the form is valid, before any save, so a color typed
   * here previews on a swatch beside its field. Omit where there is no
   * preview surface (config round-trip callers).
   */
  onPreview?: ((palette: CustomPalette) => void) | undefined;
}

export function CustomThemeDesigner({
  initial,
  saving = false,
  onSave,
  onPreview,
}: CustomThemeDesignerProps): React.ReactElement {
  const [primary, setPrimary] = React.useState(initial.primary);
  const [background, setBackground] = React.useState(initial.background);
  const [darkPalette, setDarkPalette] = React.useState(initial.darkPalette);
  const [problem, setProblem] = React.useState<string | null>(null);
  const fileInput = React.useRef<HTMLInputElement>(null);

  // A valid pair previews live; an incomplete one leaves the last preview
  // in place instead of flashing a black swatch mid-typing.
  const primaryPreview = normalizeHexColor(primary);
  const backgroundPreview = normalizeHexColor(background);
  React.useEffect(() => {
    if (onPreview === undefined) return;
    if (primaryPreview === null || backgroundPreview === null) return;
    onPreview({ primary: primaryPreview, background: backgroundPreview, darkPalette });
  }, [onPreview, primaryPreview, backgroundPreview, darkPalette]);

  function save(event: React.FormEvent): void {
    event.preventDefault();
    const next = { primary: normalizeHexColor(primary), background: normalizeHexColor(background) };
    if (next.primary === null || next.background === null) {
      setProblem("Enter hex colors like #3f76ff.");
      return;
    }
    setProblem(null);
    onSave({ primary: next.primary, background: next.background, darkPalette });
  }

  function download(): void {
    const blob = new Blob([serializeThemeConfig({ primary, background, darkPalette })], {
      type: "application/json",
    });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `pi-dash-theme-${Date.now()}.json`;
    document.body.appendChild(link);
    link.click();
    link.remove();
    URL.revokeObjectURL(url);
  }

  async function importFile(event: React.ChangeEvent<HTMLInputElement>): Promise<void> {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    let text: string;
    try {
      text = await readFileText(file);
    } catch {
      setProblem("Could not read the file.");
      return;
    }
    const parsed = parseThemeConfigFile(text);
    if ("error" in parsed) {
      setProblem(parsed.error);
      return;
    }
    setProblem(null);
    setPrimary(parsed.palette.primary);
    setBackground(parsed.palette.background);
    setDarkPalette(parsed.palette.darkPalette);
  }

  return (
    <form
      aria-label="Customize your theme"
      onSubmit={save}
      className="flex flex-col gap-(--space-4) rounded-(--radius-card) border border-(--border) bg-(--surface) p-(--space-4)"
    >
      <h3 className="text-emphasis text-(--text)">Customize your theme</h3>
      <fieldset className="flex flex-col gap-(--space-2)">
        <legend className="text-meta font-medium text-(--text)">Color base</legend>
        <div className="flex gap-(--space-4)">
          {(
            [
              { label: "Light mode", dark: false },
              { label: "Dark mode", dark: true },
            ] as const
          ).map((option) => (
            <label key={option.label} className="text-body flex items-center gap-(--space-2) text-(--text)">
              <input
                type="radio"
                name="palette-base"
                value={option.dark ? "dark" : "light"}
                checked={darkPalette === option.dark}
                onChange={() => setDarkPalette(option.dark)}
                className="accent-(--accent)"
              />
              {option.label}
            </label>
          ))}
        </div>
      </fieldset>
      <div className="flex flex-col gap-(--space-3)">
        <div className="flex items-center gap-(--space-3)">
          <input
            type="color"
            aria-label="Primary color picker"
            value={normalizeHexColor(primary) ?? "#000000"}
            onChange={(event) => setPrimary(event.target.value)}
            className="h-8 w-12 shrink-0 cursor-pointer rounded-(--radius-control) border border-(--border) bg-(--bg)"
          />
          <Input
            label="Primary color"
            name="primary"
            value={primary}
            style={primaryPreview === null ? undefined : { backgroundColor: primaryPreview }}
            onChange={(event) => setPrimary(event.target.value)}
          />
        </div>
        <div className="flex items-center gap-(--space-3)">
          <input
            type="color"
            aria-label="Background color picker"
            value={normalizeHexColor(background) ?? "#000000"}
            onChange={(event) => setBackground(event.target.value)}
            className="h-8 w-12 shrink-0 cursor-pointer rounded-(--radius-control) border border-(--border) bg-(--bg)"
          />
          <Input
            label="Background color"
            name="background"
            value={background}
            style={backgroundPreview === null ? undefined : { backgroundColor: backgroundPreview }}
            onChange={(event) => setBackground(event.target.value)}
          />
        </div>
      </div>
      {problem !== null ? (
        <p role="alert" className="text-body text-(--danger)">
          {problem}
        </p>
      ) : null}
      <div className="flex flex-wrap items-center gap-(--space-3)">
        <Button type="submit" variant="primary" loading={saving}>
          Set theme
        </Button>
        <Button type="button" variant="secondary" onClick={download}>
          Download config
        </Button>
        <Button type="button" variant="secondary" onClick={() => fileInput.current?.click()}>
          Import config
        </Button>
        <input
          ref={fileInput}
          type="file"
          accept="application/json"
          aria-label="Import theme config file"
          className="hidden"
          onChange={(event) => void importFile(event)}
        />
      </div>
    </form>
  );
}
