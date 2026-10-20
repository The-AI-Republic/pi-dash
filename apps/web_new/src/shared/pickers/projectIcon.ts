// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Project mark resolution (parity/specs/issues.md §17, ISS-211). A project
// names its mark either with an emoji or with a Material icon and a color;
// this module reads both shapes from the raw logo value and maps
// the icon name onto the closest Lucide glyph, since the new frontend's
// icon set is Lucide throughout.
import {
  Anchor,
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Bell,
  Bookmark,
  Box,
  Briefcase,
  Calendar,
  Camera,
  Check,
  Circle,
  CircleCheck,
  Clock,
  Cloud,
  Compass,
  Copy,
  Download,
  Eye,
  File,
  FileText,
  Flag,
  Folder,
  Globe,
  Grid,
  Heart,
  Home,
  Image,
  Inbox,
  Info,
  Key,
  Layers,
  Link,
  List,
  Lock,
  LogOut,
  Mail,
  Map,
  Menu,
  Minus,
  Moon,
  MoreHorizontal,
  Package,
  Pause,
  Pencil,
  Play,
  RefreshCw,
  Plus,
  Rocket,
  Search,
  Send,
  Settings,
  Shield,
  Star,
  Sun,
  Tag,
  Trash,
  TrendingUp,
  Upload,
  User,
  Users,
  Zap,
} from "lucide-react";

/**
 * A project's mark: an emoji glyph, an uploaded image, an icon name plus
 * color, or the short code's initial. The General tab's emblem control
 * stores an upload as `{ in_use: "upload", url }`; older rows written by the
 * emoji control carry the emoji's decimal codepoints (for example
 * before the upload path carry the emoji's decimal codepoints (for example
 * "128640" for a rocket) rather than the character itself.
 */
export type ProjectMark =
  | { kind: "emoji"; glyph: string }
  | { kind: "image"; url: string }
  | { kind: "icon"; name: string; color: string | null }
  | { kind: "initial" };

/**
 * Decode the stored emoji form. A dash-separated decimal list is a set of
 * Unicode code points; anything that does not decode (out of range, not a
 * number) is returned untouched so a literal character still renders.
 */
export function decodeEmojiValue(value: string): string {
  if (!/^\d+(-\d+)*$/.test(value)) return value;
  const points = value.split("-").map((part) => Number(part));
  if (points.some((point) => !isCodePoint(point))) return value;
  try {
    return String.fromCodePoint(...points);
  } catch {
    return value;
  }
}

/** A scalar value: in range and not a lone UTF-16 surrogate. */
function isCodePoint(point: number): boolean {
  return Number.isInteger(point) && point >= 0 && point <= 0x10ffff && (point < 0xd800 || point > 0xdfff);
}

/**
 * Material icon names the new frontend carries. The projects dialog offers
 * the Material Symbols set in the old app; every name the project form can
 * store resolves to the closest Lucide glyph here, and anything outside
 * the map falls back to the project's initial rather than an empty box.
 */
const ICON_MAP: Readonly<Record<string, typeof Anchor>> = {
  search: Search,
  home: Home,
  menu: Menu,
  close: Minus,
  settings: Settings,
  done: Check,
  check_circle: CircleCheck,
  favorite: Heart,
  add: Plus,
  delete: Trash,
  arrow_back: ArrowLeft,
  star: Star,
  logout: LogOut,
  add_circle: Plus,
  cancel: Circle,
  arrow_drop_down: ArrowDown,
  more_vert: MoreHorizontal,
  check: Check,
  arrow_forward: ArrowRight,
  menu_open: Menu,
  arrow_upward: ArrowUp,
  arrow_downward: ArrowDown,
  refresh: RefreshCw,
  sync: RefreshCw,
  autorenew: RefreshCw,
  sort: List,
  filter_list: List,
  open_in_new: Link,
  link: Link,
  create: Pencil,
  edit: Pencil,
  folder: Folder,
  folder_open: Folder,
  description: FileText,
  file: File,
  insert_drive_file: File,
  image: Image,
  photo: Image,
  mail: Mail,
  email: Mail,
  send: Send,
  notifications: Bell,
  lock: Lock,
  lock_open: Key,
  visibility: Eye,
  visibility_off: Eye,
  person: User,
  person_add: Users,
  group: Users,
  groups: Users,
  account_circle: User,
  people: Users,
  calendar_today: Calendar,
  event: Calendar,
  schedule: Clock,
  access_time: Clock,
  cloud: Cloud,
  public: Globe,
  language: Globe,
  place: Map,
  location_on: Map,
  map: Map,
  favorite_border: Heart,
  bookmark: Bookmark,
  label: Tag,
  sell: Tag,
  local_offer: Tag,
  business: Briefcase,
  work: Briefcase,
  dashboard: Grid,
  apps: Grid,
  grid_view: Grid,
  category: Grid,
  inventory_2: Package,
  all_inbox: Inbox,
  inbox: Inbox,
  info: Info,
  help: Info,
  report: Flag,
  flag: Flag,
  security: Shield,
  verified_user: Shield,
  key: Key,
  vpn_key: Key,
  layers: Layers,
  view_list: List,
  list: List,
  trending_up: TrendingUp,
  insights: TrendingUp,
  bolt: Zap,
  flash_on: Zap,
  rocket_launch: Rocket,
  play_arrow: Play,
  pause: Pause,
  download: Download,
  upload: Upload,
  file_copy: Copy,
  content_copy: Copy,
  light_mode: Sun,
  dark_mode: Moon,
  nightlight: Moon,
  explore: Compass,
  navigation: Compass,
  camera_alt: Camera,
  photo_camera: Camera,
  anchor: Anchor,
  box: Box,
};

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/** The project's mark with its emoji, icon, or initial fallback. */
export function projectMark(logo: Record<string, unknown>): ProjectMark {
  if (logo.in_use === "upload") {
    const url = logo["url"];
    if (typeof url === "string" && url.trim() !== "") return { kind: "image", url };
  }
  if (logo.in_use === "emoji") {
    const emoji = record(logo.emoji);
    const url = emoji?.["url"];
    const value = emoji?.["value"];
    if (typeof url === "string" && url.trim() !== "") return { kind: "image", url };
    if (typeof value === "string" && value.trim() !== "") {
      const glyph = decodeEmojiValue(value);
      if (glyph.trim() !== "") return { kind: "emoji", glyph };
    }
  }
  if (logo.in_use === "icon") {
    const icon = record(logo.icon);
    const name = icon?.["name"];
    if (typeof name === "string" && ICON_MAP[name] !== undefined) {
      const color = icon?.["color"];
      return { kind: "icon", name, color: typeof color === "string" && color.trim() !== "" ? color : null };
    }
  }
  return { kind: "initial" };
}

/** The Lucide glyph for a stored Material icon name, when one exists. */
export function projectIconGlyph(name: string): typeof Anchor | null {
  return ICON_MAP[name] ?? null;
}

/** The mark's text form: the emoji glyph, else the short code's initial. */
export function projectMarkText(mark: ProjectMark, identifier: string): string {
  if (mark.kind === "emoji") return mark.glyph;
  const initial = identifier.trim().slice(0, 1).toUpperCase();
  return initial === "" ? "?" : initial;
}
