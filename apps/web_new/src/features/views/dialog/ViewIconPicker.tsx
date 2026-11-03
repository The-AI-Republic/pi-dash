// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// View icon picker (parity/specs/views.md §5, VIEW-013). The dialog's
// logo control: a trigger showing the current mark (or the default
// glyph) opens a popover with an Emoji tab and an Icon tab. An emoji
// pick stores an emoji mark; a glyph pick stores the glyph name with the
// chosen swatch color. The picker closes on every pick.

import { IconButton, Popover } from "@pidash/kit";
import {
  Activity,
  Bookmark,
  CalendarDays,
  Flag,
  Folder,
  Globe,
  Kanban,
  LayoutGrid,
  List,
  Star,
  Tag,
  Target,
  type LucideIcon,
} from "lucide-react";
import * as React from "react";

/** A saved view's logo, as the create/update payloads carry it. */
export type ViewLogoProps =
  | { in_use: "emoji"; emoji: { value: string } }
  | { in_use: "icon"; icon: { name: string; color: string } };

/** Glyphs the Icon tab offers, in offer order. */
const VIEW_GLYPHS: Readonly<Record<string, LucideIcon>> = {
  Activity,
  Bookmark,
  CalendarDays,
  Flag,
  Folder,
  Globe,
  Kanban,
  LayoutGrid,
  List,
  Star,
  Tag,
  Target,
};

/** Swatch colors a glyph pick can carry; the first is the default. */
export const VIEW_ICON_COLORS: readonly string[] = ["#5b5bd6", "#0e7c3e", "#b25e09", "#c0362c", "#7c3aed", "#0e7490"];

/** Emoji the Emoji tab offers. A fixed set: no picker library loads here. */
export const VIEW_EMOJIS: readonly string[] = [
  "🚀",
  "⭐",
  "🎯",
  "🔥",
  "💡",
  "✅",
  "📌",
  "📊",
  "📅",
  "📝",
  "🐞",
  "🛠️",
  "⚙️",
  "🔍",
  "💬",
  "❤️",
  "🎨",
  "📦",
  "🧪",
  "🚧",
  "✨",
  "🏷️",
  "📁",
  "🌍",
];

/** Fallback mark: no logo yet, or a name this client does not know. */
function DefaultMark(): React.ReactElement {
  return <LayoutGrid size={16} strokeWidth={2} aria-hidden="true" />;
}

/** The mark a logo carries: its emoji, its glyph, or the default. */
export function ViewLogoMark({ logo }: { logo: ViewLogoProps | null }): React.ReactElement {
  if (logo?.in_use === "emoji") {
    return (
      <span aria-hidden="true" className="text-body leading-none">
        {logo.emoji.value}
      </span>
    );
  }
  if (logo?.in_use === "icon") {
    const Glyph = VIEW_GLYPHS[logo.icon.name];
    if (Glyph === undefined) return <DefaultMark />;
    return <Glyph size={16} strokeWidth={2} aria-hidden="true" color={logo.icon.color} />;
  }
  return <DefaultMark />;
}

export interface ViewIconPickerProps {
  /** Current logo; null until the first pick. */
  value: ViewLogoProps | null;
  onChange: (next: ViewLogoProps) => void;
  /** True while a submit is in flight; the picker takes no edits. */
  disabled?: boolean | undefined;
}

type PickerTab = "emoji" | "icon";

const TAB_LABELS: Readonly<Record<PickerTab, string>> = { emoji: "Emoji", icon: "Icon" };

/** Logo control for the view dialog. The picker closes on every pick. */
export function ViewIconPicker({ value, onChange, disabled = false }: ViewIconPickerProps): React.ReactElement {
  const [open, setOpen] = React.useState(false);
  const [tab, setTab] = React.useState<PickerTab>("emoji");
  const [color, setColor] = React.useState<string>(VIEW_ICON_COLORS[0]!);

  function pickEmoji(emoji: string): void {
    onChange({ in_use: "emoji", emoji: { value: emoji } });
    setOpen(false);
  }

  function pickGlyph(name: string): void {
    onChange({ in_use: "icon", icon: { name, color } });
    setOpen(false);
  }

  return (
    <Popover
      trigger={
        <IconButton label="Choose view icon" variant="secondary" disabled={disabled} data-testid="view-icon-picker">
          <ViewLogoMark logo={value} />
        </IconButton>
      }
      open={open}
      onOpenChange={setOpen}
    >
      <div role="tablist" aria-label="Icon type" className="flex gap-(--space-2) px-(--space-2) pt-(--space-2)">
        {(Object.keys(TAB_LABELS) as PickerTab[]).map((key) => (
          <button
            key={key}
            type="button"
            role="tab"
            aria-selected={tab === key}
            onClick={() => setTab(key)}
            className={
              tab === key
                ? "text-body rounded-(--radius-control) bg-(--subtle) px-(--space-4) py-(--space-2) font-medium text-(--text)"
                : "text-body rounded-(--radius-control) px-(--space-4) py-(--space-2) text-(--text-muted) hover:bg-(--subtle)"
            }
          >
            {TAB_LABELS[key]}
          </button>
        ))}
      </div>
      {tab === "emoji" ? (
        <div
          role="tabpanel"
          aria-label="Emoji"
          data-testid="view-icon-emojis"
          className="grid max-h-56 grid-cols-8 gap-(--space-1) overflow-y-auto p-(--space-2)"
        >
          {VIEW_EMOJIS.map((emoji) => (
            <button
              key={emoji}
              type="button"
              aria-label={`Emoji ${emoji}`}
              onClick={() => pickEmoji(emoji)}
              className="text-body rounded-(--radius-control) p-(--space-2) leading-none hover:bg-(--subtle)"
            >
              <span aria-hidden="true">{emoji}</span>
            </button>
          ))}
        </div>
      ) : (
        <div role="tabpanel" aria-label="Icon" className="flex flex-col gap-(--space-2) p-(--space-2)">
          <div
            role="radiogroup"
            aria-label="Icon color"
            data-testid="view-icon-colors"
            className="flex gap-(--space-2) px-(--space-2)"
          >
            {VIEW_ICON_COLORS.map((swatch) => (
              <button
                key={swatch}
                type="button"
                role="radio"
                aria-checked={color === swatch}
                aria-label={`Icon color ${swatch}`}
                onClick={() => setColor(swatch)}
                style={{ backgroundColor: swatch }}
                className="size-5 rounded-full outline-offset-2 data-[checked]:outline-2 data-[checked]:outline-(--focus-ring-color)"
                data-checked={color === swatch ? "true" : undefined}
              />
            ))}
          </div>
          <div data-testid="view-icon-glyphs" className="grid max-h-56 grid-cols-6 gap-(--space-1) overflow-y-auto">
            {Object.keys(VIEW_GLYPHS).map((name) => {
              const Glyph = VIEW_GLYPHS[name]!;
              return (
                <button
                  key={name}
                  type="button"
                  aria-label={`View icon ${name}`}
                  onClick={() => pickGlyph(name)}
                  className="rounded-(--radius-control) p-(--space-2) hover:bg-(--subtle)"
                >
                  <Glyph size={16} strokeWidth={2} aria-hidden="true" color={color} />
                </button>
              );
            })}
          </div>
        </div>
      )}
    </Popover>
  );
}
