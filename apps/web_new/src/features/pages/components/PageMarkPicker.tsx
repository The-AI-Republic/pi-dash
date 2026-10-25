// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Mark chooser for a page draft (parity/specs/pages.md §3.6 and §5.2,
// PAGE-008). The page's mark is the `logo_props` value the create route
// stores: one emoji or one named symbol with a colour. The picker is
// inline so it can sit inside the create dialog, and it draws its tiles
// through the settings area's shared emblem catalogue and the lazy symbol
// wrapper rather than keeping a second copy of either.

import { Button, Input, Popover } from "@pidash/kit";
import * as React from "react";

import {
  DEFAULT_EMBLEM_ICON_COLOR,
  EMBLEM_ICON_COLORS,
  emojiFromQuery,
  iconNameLabel,
  markEmblem,
  searchEmblemEmoji,
  searchEmblemIcons,
  type EmblemIcon,
} from "../../project-settings/index.js";
import { ProjectMarkIcon, projectMark } from "../../../shared/pickers/index.js";

type MarkMode = "emoji" | "icon";

/**
 * Leading glyph for one catalogue icon, drawn through the shared lazy
 * wrapper so the 100+ symbol map stays out of the pages chunk. An unknown
 * name falls back to the icon's first letter.
 */
function IconGlyph({ icon, color }: { icon: EmblemIcon; color: string }): React.ReactElement {
  return (
    <span aria-hidden="true" className="inline-flex items-center justify-center" style={{ color }}>
      <ProjectMarkIcon
        mark={{ kind: "icon", name: icon.name, color }}
        identifier={icon.label}
        size={18}
        fallback={<span>{icon.label.slice(0, 1)}</span>}
      />
    </span>
  );
}

const tileClass = (selected: boolean): string =>
  `flex size-9 items-center justify-center rounded-(--radius-control) border text-h3 ${
    selected ? "border-(--accent) bg-(--subtle)" : "border-transparent hover:bg-(--subtle)"
  }`;

export interface PageMarkPickerProps {
  /** Current draft mark; an empty record means none chosen yet. */
  draft: Record<string, unknown>;
  onDraftChange: (draft: Record<string, unknown>) => void;
  /** Test hook / sizing of the trigger; the create dialog keeps the default. */
  triggerTestId?: string | undefined;
  triggerClassName?: string | undefined;
  /** Accessible name of the trigger; the create dialog keeps the default. */
  triggerLabel?: string | undefined;
}

/** The trigger's own face: the draft's glyph, else a neutral page sheet. */
export function pageMarkTriggerGlyph(draft: Record<string, unknown>): {
  kind: "emoji" | "icon" | "none";
  value: string;
} {
  const mark = projectMark(draft);
  if (mark.kind === "emoji") return { kind: "emoji", value: mark.glyph };
  if (mark.kind === "icon") return { kind: "icon", value: mark.name };
  return { kind: "none", value: "" };
}

/** The colour an icon pick should start from: the draft's own, else the default. */
export function pageMarkColor(draft: Record<string, unknown>): string {
  const mark = projectMark(draft);
  return mark.kind === "icon" && mark.color !== null ? mark.color : DEFAULT_EMBLEM_ICON_COLOR;
}

export function PageMarkPicker({
  draft,
  onDraftChange,
  triggerTestId = "page-create-mark",
  triggerClassName = "flex size-9 shrink-0 items-center justify-center rounded-(--radius-control) border border-(--border) bg-(--subtle) hover:bg-(--bg)",
  triggerLabel,
}: PageMarkPickerProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [mode, setMode] = React.useState<MarkMode>("icon");
  const [query, setQuery] = React.useState("");
  const [color, setColor] = React.useState<string>(() => pageMarkColor(draft));
  const current = pageMarkTriggerGlyph(draft);

  // A reopened picker starts from what the draft actually carries, so a
  // second icon pick keeps the colour the first one chose instead of
  // silently falling back to the default.
  React.useEffect(() => {
    if (open) return;
    setQuery("");
    setColor(pageMarkColor(draft));
  }, [open, draft]);

  const choose = (next: Record<string, unknown>): void => {
    onDraftChange(next);
    setOpen(false);
  };

  const emojiChoices = searchEmblemEmoji(query);
  const typedEmoji = emojiFromQuery(query);
  const typedIsExtra = typedEmoji !== null && !emojiChoices.some((entry) => entry.glyph === typedEmoji);
  const icons = searchEmblemIcons(query);

  return (
    <Popover
      open={open}
      onOpenChange={setOpen}
      className="w-72"
      trigger={
        <button
          type="button"
          aria-label={triggerLabel ?? (current.kind === "none" ? "Choose a page mark" : "Change the page mark")}
          aria-expanded={open}
          data-testid={triggerTestId}
          className={triggerClassName}
        >
          {current.kind === "emoji" ? (
            <span aria-hidden="true">{current.value}</span>
          ) : current.kind === "icon" ? (
            <IconGlyph icon={{ name: current.value, label: current.value }} color={color} />
          ) : (
            <span aria-hidden="true" className="text-(--text-muted)">
              ¶
            </span>
          )}
        </button>
      }
    >
      <div className="flex flex-col gap-(--space-3)">
        <div role="tablist" aria-label="Mark kind" className="grid grid-cols-2 gap-(--space-2)">
          {(["emoji", "icon"] as const).map((kind) => (
            <button
              key={kind}
              type="button"
              role="tab"
              aria-selected={mode === kind}
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
          label="Search marks"
          hideLabel
          value={query}
          placeholder={mode === "emoji" ? "Search or type an emoji" : "Search icons"}
          onChange={(event) => setQuery(event.target.value)}
        />

        {mode === "emoji" ? (
          <div className="flex flex-col gap-(--space-2)">
            {typedIsExtra ? (
              <Button
                variant="secondary"
                aria-label={`Use ${typedEmoji}`}
                onClick={() => choose(markEmblem("emoji", typedEmoji))}
              >
                Use {typedEmoji}
              </Button>
            ) : null}
            {emojiChoices.length === 0 && !typedIsExtra ? (
              <p className="text-body text-(--text-muted)">No emoji matches that search.</p>
            ) : null}
            {emojiChoices.length === 0 ? null : (
              <ul aria-label="Emoji" className="grid max-h-64 grid-cols-6 gap-(--space-1) overflow-y-auto">
                {emojiChoices.map((entry) => (
                  <li key={entry.glyph}>
                    <button
                      type="button"
                      aria-label={entry.name}
                      aria-pressed={current.kind === "emoji" && current.value === entry.glyph}
                      className={tileClass(current.kind === "emoji" && current.value === entry.glyph)}
                      onClick={() => choose(markEmblem("emoji", entry.glyph))}
                    >
                      {entry.glyph}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        ) : (
          <div className="flex flex-col gap-(--space-2)">
            <div role="group" aria-label="Icon colour" className="flex flex-wrap gap-(--space-1)">
              {EMBLEM_ICON_COLORS.map((swatch) => (
                <button
                  key={swatch.value}
                  type="button"
                  aria-label={swatch.name}
                  aria-pressed={color === swatch.value}
                  className={`size-5 rounded-full border-2 ${
                    color === swatch.value ? "border-(--text)" : "border-transparent"
                  }`}
                  style={{ backgroundColor: swatch.value }}
                  onClick={() => setColor(swatch.value)}
                />
              ))}
            </div>
            {icons.length === 0 ? (
              <p className="text-body text-(--text-muted)">No icon matches that search.</p>
            ) : (
              <ul aria-label="Icons" className="grid max-h-64 grid-cols-6 gap-(--space-1) overflow-y-auto">
                {icons.map((icon) => (
                  <li key={icon.name}>
                    <button
                      type="button"
                      aria-label={iconNameLabel(icon.name)}
                      aria-pressed={current.kind === "icon" && current.value === icon.name}
                      className={tileClass(current.kind === "icon" && current.value === icon.name)}
                      onClick={() => choose(markEmblem("icon", { name: icon.name, color }))}
                    >
                      <IconGlyph icon={icon} color={color} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

        {current.kind === "none" ? null : (
          <div className="flex justify-end">
            <Button variant="ghost" size="small" onClick={() => choose({})}>
              Remove mark
            </Button>
          </div>
        )}
      </div>
    </Popover>
  );
}
