// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Emblem catalogue (parity/specs/project-settings-general.md §PSET-007).
// The picker offers two kinds of artwork: a named emoji alphabet wide
// enough to reach any common character (and a free-text escape hatch for
// one outside it), and the Material symbol names the project form can
// store, each drawn through the shared Lucide map. Icon picks carry a
// colour from the fixed swatches. There is no gallery and no provider
// search — the catalogue is the whole surface.

import { PROJECT_ICON_NAMES, projectMark } from "../../../shared/pickers/index.js";

export interface EmblemEmoji {
  glyph: string;
  /** Searchable name, also the tooltip a sighted operator reads. */
  name: string;
}

export interface EmblemEmojiGroup {
  label: string;
  emoji: ReadonlyArray<EmblemEmoji>;
}

/** Emoji the picker shows for each group; any other character stays reachable through search. */
export const EMBLEM_EMOJI_GROUPS: ReadonlyArray<EmblemEmojiGroup> = [
  {
    label: "Nature",
    emoji: [
      { glyph: "🌊", name: "wave" },
      { glyph: "🌿", name: "herb" },
      { glyph: "🚀", name: "rocket" },
      { glyph: "🔥", name: "fire" },
      { glyph: "⚡", name: "bolt" },
      { glyph: "🌙", name: "crescent moon" },
      { glyph: "☀️", name: "sun" },
      { glyph: "🌈", name: "rainbow" },
      { glyph: "⛰️", name: "mountain" },
      { glyph: "🏔️", name: "snow mountain" },
      { glyph: "🌋", name: "volcano" },
      { glyph: "🏝️", name: "island" },
      { glyph: "🌵", name: "cactus" },
      { glyph: "🌲", name: "evergreen" },
      { glyph: "🌳", name: "tree" },
      { glyph: "🍀", name: "four leaf clover" },
      { glyph: "🌻", name: "sunflower" },
      { glyph: "🌸", name: "cherry blossom" },
      { glyph: "🌹", name: "rose" },
      { glyph: "🍁", name: "maple leaf" },
      { glyph: "❄️", name: "snowflake" },
      { glyph: "💧", name: "droplet" },
      { glyph: "🌍", name: "globe" },
      { glyph: "🌌", name: "galaxy" },
    ],
  },
  {
    label: "Animals",
    emoji: [
      { glyph: "🦊", name: "fox" },
      { glyph: "🐝", name: "bee" },
      { glyph: "🦋", name: "butterfly" },
      { glyph: "🐙", name: "octopus" },
      { glyph: "🐢", name: "turtle" },
      { glyph: "🦉", name: "owl" },
      { glyph: "🦅", name: "eagle" },
      { glyph: "🐺", name: "wolf" },
      { glyph: "🦁", name: "lion" },
      { glyph: "🐯", name: "tiger" },
      { glyph: "🐼", name: "panda" },
      { glyph: "🐧", name: "penguin" },
      { glyph: "🦄", name: "unicorn" },
      { glyph: "🐳", name: "whale" },
      { glyph: "🐬", name: "dolphin" },
      { glyph: "🦈", name: "shark" },
    ],
  },
  {
    label: "Work",
    emoji: [
      { glyph: "🎯", name: "target" },
      { glyph: "🧭", name: "compass" },
      { glyph: "💎", name: "gem" },
      { glyph: "🛠️", name: "tools" },
      { glyph: "⚙️", name: "gear" },
      { glyph: "🔧", name: "wrench" },
      { glyph: "🔨", name: "hammer" },
      { glyph: "📐", name: "triangle ruler" },
      { glyph: "📏", name: "straight ruler" },
      { glyph: "🧪", name: "test tube" },
      { glyph: "🔬", name: "microscope" },
      { glyph: "🔭", name: "telescope" },
      { glyph: "💻", name: "laptop" },
      { glyph: "🖥️", name: "desktop" },
      { glyph: "⌨️", name: "keyboard" },
      { glyph: "🖱️", name: "mouse" },
      { glyph: "📱", name: "phone" },
      { glyph: "🗂️", name: "card index" },
      { glyph: "📌", name: "pushpin" },
      { glyph: "📎", name: "paperclip" },
      { glyph: "🔖", name: "bookmark" },
      { glyph: "📊", name: "bar chart" },
      { glyph: "📈", name: "chart up" },
      { glyph: "📉", name: "chart down" },
      { glyph: "🗓️", name: "calendar" },
      { glyph: "⏱️", name: "stopwatch" },
      { glyph: "⏳", name: "hourglass" },
    ],
  },
  {
    label: "Culture",
    emoji: [
      { glyph: "🎨", name: "art palette" },
      { glyph: "🖌️", name: "paintbrush" },
      { glyph: "✏️", name: "pencil" },
      { glyph: "📚", name: "books" },
      { glyph: "📖", name: "open book" },
      { glyph: "📝", name: "memo" },
      { glyph: "🎵", name: "music note" },
      { glyph: "🎧", name: "headphones" },
      { glyph: "🎬", name: "clapper" },
      { glyph: "📷", name: "camera" },
      { glyph: "🎮", name: "game controller" },
      { glyph: "🏆", name: "trophy" },
      { glyph: "🥇", name: "gold medal" },
      { glyph: "🎲", name: "die" },
      { glyph: "🧩", name: "puzzle piece" },
      { glyph: "🎁", name: "gift" },
    ],
  },
  {
    label: "Travel",
    emoji: [
      { glyph: "🚲", name: "bicycle" },
      { glyph: "🛴", name: "scooter" },
      { glyph: "🚗", name: "car" },
      { glyph: "🚐", name: "van" },
      { glyph: "🚆", name: "train" },
      { glyph: "✈️", name: "airplane" },
      { glyph: "🚁", name: "helicopter" },
      { glyph: "⛵", name: "sailboat" },
      { glyph: "🛸", name: "ufo" },
      { glyph: "🛰️", name: "satellite" },
      { glyph: "🗺️", name: "world map" },
      { glyph: "🧳", name: "luggage" },
    ],
  },
  {
    label: "Marks",
    emoji: [
      { glyph: "✨", name: "sparkles" },
      { glyph: "💡", name: "light bulb" },
      { glyph: "✅", name: "check mark" },
      { glyph: "☑️", name: "check box" },
      { glyph: "⭐", name: "star" },
      { glyph: "🌟", name: "glowing star" },
      { glyph: "❤️", name: "heart" },
      { glyph: "🧡", name: "orange heart" },
      { glyph: "💚", name: "green heart" },
      { glyph: "💙", name: "blue heart" },
      { glyph: "💜", name: "purple heart" },
      { glyph: "🖤", name: "black heart" },
      { glyph: "🔔", name: "bell" },
      { glyph: "🔒", name: "lock" },
      { glyph: "🔑", name: "key" },
      { glyph: "🛡️", name: "shield" },
      { glyph: "⚖️", name: "scales" },
      { glyph: "♻️", name: "recycle" },
      { glyph: "🔗", name: "link" },
      { glyph: "📣", name: "megaphone" },
      { glyph: "💬", name: "speech balloon" },
      { glyph: "❓", name: "question mark" },
      { glyph: "❗", name: "exclamation mark" },
      { glyph: "🌀", name: "cyclone" },
    ],
  },
];

/** Flat alphabet for a query: blank shows everything, a glyph or name narrows it. */
export function searchEmblemEmoji(query: string, groups = EMBLEM_EMOJI_GROUPS): ReadonlyArray<EmblemEmoji> {
  const trimmed = query.trim().toLowerCase();
  const all = groups.flatMap((group) => group.emoji);
  if (trimmed === "") return all;
  return all.filter((entry) => entry.glyph === query.trim() || entry.name.includes(trimmed));
}

/**
 * A single emoji the operator typed or pasted besides the catalogue. Only
 * one character (with its optional variation selector or joiners) counts;
 * words and longer sequences are not marks.
 */
export function emojiFromQuery(query: string): string | null {
  const trimmed = query.trim();
  if (trimmed === "") return null;
  if (!/^\p{Extended_Pictographic}/u.test(trimmed)) return null;
  // Exactly one grapheme cluster: a pictograph with its variation selector,
  // skin tone, tag sequence or joined family counts; a word, a half-typed
  // search such as "🔥x" or several separate emoji do not.
  const clusters = [...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(trimmed)];
  return clusters.length === 1 ? trimmed : null;
}

/** Swatches an icon pick may take; the first is the default. */
export const EMBLEM_ICON_COLORS = [
  { value: "#6366f1", name: "Indigo" },
  { value: "#0ea5e9", name: "Sky" },
  { value: "#10b981", name: "Emerald" },
  { value: "#f59e0b", name: "Amber" },
  { value: "#ef4444", name: "Red" },
  { value: "#ec4899", name: "Pink" },
  { value: "#8b5cf6", name: "Violet" },
  { value: "#64748b", name: "Slate" },
] as const satisfies ReadonlyArray<{ value: string; name: string }>;

/** The colour a fresh icon pick takes. */
export const DEFAULT_EMBLEM_ICON_COLOR = EMBLEM_ICON_COLORS[0].value;

/** Readable label for a stored symbol name ("rocket_launch" -> "Rocket launch"). */
export function iconNameLabel(name: string): string {
  const words = name.split("_").filter((word) => word !== "");
  const first = words[0];
  if (first === undefined) return name;
  const rest = words.slice(1);
  return [first.charAt(0).toUpperCase() + first.slice(1), ...rest].join(" ");
}

/** The icon a draft names, when it holds one. */
export function selectedEmblemIcon(logoProps: Record<string, unknown>): {
  name: string;
  color: string | null;
} | null {
  const mark = projectMark(logoProps);
  return mark.kind === "icon" ? { name: mark.name, color: mark.color } : null;
}

export interface EmblemIcon {
  name: string;
  label: string;
}

/** Every symbol the project form may store, named for search. */
export const EMBLEM_ICONS: ReadonlyArray<EmblemIcon> = PROJECT_ICON_NAMES.map((name) => ({
  name,
  label: iconNameLabel(name),
}));

/** Icons matching a blank query ("all") or a name fragment. */
export function searchEmblemIcons(query: string, icons = EMBLEM_ICONS): ReadonlyArray<EmblemIcon> {
  const trimmed = query.trim().toLowerCase();
  if (trimmed === "") return icons;
  return icons.filter((icon) => icon.name.includes(trimmed) || icon.label.toLowerCase().includes(trimmed));
}
