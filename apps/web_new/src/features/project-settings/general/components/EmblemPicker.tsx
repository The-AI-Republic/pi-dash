// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Emblem picker (parity/specs/project-settings-general.md §PSET-007). A
// dialog with two surfaces — a searchable emoji alphabet and the Material
// symbols this frontend draws, each with a colour swatch. A pick swaps the
// draft mark and dirties the form; the normal save persists it. Any emoji
// is reachable: searching a character not in the alphabet lets the
// operator use it directly. There is no cover, gallery or provider search.

import { Button, Dialog, Input } from "@pidash/kit";
import * as React from "react";

import { ProjectMark, projectMark } from "../../../../shared/pickers/index.js";
import { projectIconGlyph } from "../../../../shared/pickers/projectIconGlyphs.js";
import {
  DEFAULT_EMBLEM_ICON_COLOR,
  EMBLEM_ICON_COLORS,
  iconNameLabel,
  searchEmblemEmoji,
  selectedEmblemIcon,
  searchEmblemIcons,
  emojiFromQuery,
  type EmblemIcon,
} from "../emblemCatalog.js";
import { markEmblem } from "../rules.js";

export type EmblemMode = "emoji" | "icon";

export interface EmblemPickerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Current draft mark, used to mark the chosen tile. */
  logoProps: Record<string, unknown>;
  onPick: (logoProps: Record<string, unknown>) => void;
}

/** The tile class shared by both surfaces. */
function tileClass(selected: boolean): string {
  return `flex h-12 w-full items-center justify-center rounded-(--radius-control) border ${
    selected ? "border-(--accent) bg-(--subtle)" : "border-(--border) hover:bg-(--subtle)"
  }`;
}

/** The mode to open on: the draft's own kind when it names one. */
export function initialEmblemMode(logoProps: Record<string, unknown>): EmblemMode {
  return selectedEmblemIcon(logoProps) === null ? "emoji" : "icon";
}

/** One symbol in the grid; a name outside the map draws nothing. */
export function IconTile({ icon, color }: { icon: EmblemIcon; color: string | null }): React.ReactElement {
  const Glyph = projectIconGlyph(icon.name);
  return (
    <span aria-hidden="true" className="flex size-6 items-center justify-center" style={{ color: color ?? "inherit" }}>
      {Glyph === null ? null : <Glyph size={20} strokeWidth={1.5} />}
    </span>
  );
}

export function EmblemPicker({ open, onOpenChange, logoProps, onPick }: EmblemPickerProps): React.ReactElement {
  const [mode, setMode] = React.useState<EmblemMode>(() => initialEmblemMode(logoProps));
  const [query, setQuery] = React.useState("");
  const [color, setColor] = React.useState<string>(DEFAULT_EMBLEM_ICON_COLOR);
  const mark = projectMark(logoProps);
  const currentEmoji = mark.kind === "emoji" ? mark.glyph : null;
  const chosenIcon = selectedEmblemIcon(logoProps);
  const currentIcon = chosenIcon === null ? null : chosenIcon.name;

  React.useEffect(() => {
    if (!open) {
      setQuery("");
      setMode(initialEmblemMode(logoProps));
      return;
    }
    const icon = selectedEmblemIcon(logoProps);
    if (icon !== null && icon.color !== null) setColor(icon.color);
    // Only the open transition re-seeds the dialog; a pick inside the open
    // dialog must not reset the search the operator is typing.
  }, [open]);

  const close = (next: boolean): void => {
    if (!next) setQuery("");
    onOpenChange(next);
  };

  const pick = (logoPropsNext: Record<string, unknown>): void => {
    onPick(logoPropsNext);
    setQuery("");
    onOpenChange(false);
  };

  const emojiChoices = searchEmblemEmoji(query);
  const typedEmoji = emojiFromQuery(query);
  const typedIsExtra = typedEmoji !== null && !emojiChoices.some((entry) => entry.glyph === typedEmoji);
  const icons = searchEmblemIcons(query);

  return (
    <Dialog title="Change emblem" open={open} onOpenChange={close} data-testid="general-emblem-picker">
      <div className="flex flex-col gap-(--space-4)">
        <div role="tablist" aria-label="Artwork kind" className="grid grid-cols-2 gap-(--space-2)">
          {(["emoji", "icon"] as const).map((kind) => (
            <button
              key={kind}
              type="button"
              role="tab"
              aria-selected={mode === kind}
              data-testid={`general-emblem-tab-${kind}`}
              className={`text-body rounded-(--radius-control) border px-(--space-2) py-(--space-1) ${
                mode === kind ? "border-(--accent) bg-(--subtle)" : "border-(--border) hover:bg-(--subtle)"
              }`}
              onClick={() => {
                setMode(kind);
                setQuery("");
              }}
            >
              {kind === "emoji" ? "Emoji" : "Icon"}
            </button>
          ))}
        </div>

        <Input
          label="Search"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder={mode === "emoji" ? "Search or type an emoji" : "Search icons"}
        />

        {mode === "emoji" ? (
          <div className="flex flex-col gap-(--space-2)">
            {emojiChoices.length === 0 ? (
              <p className="text-body text-(--text-muted)">
                {typedIsExtra ? "Use this emoji" : "No emoji match that search."}
              </p>
            ) : null}
            {typedIsExtra ? (
              <Button
                variant="secondary"
                data-testid="general-emblem-typed"
                aria-label={`Use ${typedEmoji}`}
                onClick={() => pick(markEmblem("emoji", typedEmoji))}
              >
                Use {typedEmoji}
              </Button>
            ) : null}
            {emojiChoices.length === 0 ? null : (
              <ul aria-label="Emblems" className="grid grid-cols-6 gap-(--space-2)">
                {emojiChoices.map((entry) => {
                  const selected = currentEmoji === entry.glyph;
                  return (
                    <li key={entry.glyph}>
                      <button
                        type="button"
                        aria-label={entry.glyph}
                        title={entry.name}
                        aria-pressed={selected}
                        data-testid="general-emblem-choice"
                        className={`text-h3 ${tileClass(selected)}`}
                        onClick={() => pick(markEmblem("emoji", entry.glyph))}
                      >
                        {entry.glyph}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        ) : (
          <div className="flex flex-col gap-(--space-3)">
            <div className="flex items-center gap-(--space-2)">
              <span className="text-meta text-(--text-muted)">Colour</span>
              <div role="group" aria-label="Icon colour" className="flex flex-wrap gap-(--space-2)">
                {EMBLEM_ICON_COLORS.map((swatch) => (
                  <button
                    key={swatch.value}
                    type="button"
                    aria-label={swatch.name}
                    aria-pressed={color === swatch.value}
                    data-testid="general-emblem-color"
                    className={`size-6 rounded-full border-2 ${
                      color === swatch.value ? "border-(--text)" : "border-transparent"
                    }`}
                    style={{ backgroundColor: swatch.value }}
                    onClick={() => setColor(swatch.value)}
                  />
                ))}
              </div>
              <ProjectMark
                logoProps={{ in_use: "icon", icon: { name: "star", color } }}
                identifier=""
                size={28}
                className="ml-auto"
              />
            </div>
            {icons.length === 0 ? (
              <p className="text-body text-(--text-muted)">No icon matches that search.</p>
            ) : (
              <ul aria-label="Icons" className="grid max-h-72 grid-cols-6 gap-(--space-2) overflow-y-auto">
                {icons.map((icon) => {
                  const selected = currentIcon === icon.name;
                  return (
                    <li key={icon.name}>
                      <button
                        type="button"
                        aria-label={iconNameLabel(icon.name)}
                        title={iconNameLabel(icon.name)}
                        aria-pressed={selected}
                        data-testid="general-emblem-icon"
                        className={tileClass(selected)}
                        onClick={() => pick(markEmblem("icon", { name: icon.name, color }))}
                      >
                        <IconTile icon={icon} color={color} />
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        )}

        <div className="flex justify-end">
          <Button variant="secondary" onClick={() => close(false)}>
            Cancel
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
