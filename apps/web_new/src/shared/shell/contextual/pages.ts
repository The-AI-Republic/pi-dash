// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page contextual palette entries (shell spec, SHELL-091). Pure
// selector: lock, visibility, archive and favorite toggles plus
// copy-URL, each gated on its own capability (owner-or-admin for
// lock, visibility and archive; member or better for favorites).
// Writes go through the thin page contracts; full page ownership
// stays in the pages area.

import { PAGE_ACCESS_PRIVATE, PAGE_ACCESS_PUBLIC } from "@pidash/api-client";

import type { PidashCommand } from "../../commands/registry.js";
import { CONTEXT_INDICATORS, CONTEXT_SECTIONS } from "./types.js";

/** Per-entry capabilities behind the page group. */
export interface PageCapabilities {
  lock: boolean;
  changeAccess: boolean;
  archive: boolean;
  favorite: boolean;
}

export interface PageContextInput {
  pageId: string;
  /** Page name shown in the chip. */
  pageName: string;
  locked: boolean;
  /** 0 public, 1 private. */
  access: number;
  archived: boolean;
  favorite: boolean;
  capabilities: PageCapabilities;
  setLocked: (locked: boolean) => void;
  setAccess: (access: number) => void;
  setArchived: (archived: boolean) => void;
  toggleFavorite: () => void;
  copyUrl: () => void;
}

export function selectPageContext(input: PageContextInput): PidashCommand[] {
  const section = CONTEXT_SECTIONS.page;
  const context = { kind: "page" as const, indicator: CONTEXT_INDICATORS.page, subject: input.pageName };
  const prefix = `context.page.${input.pageId}`;
  const commands: PidashCommand[] = [];

  function action(title: string, name: string, shortcut: string, run: () => void): PidashCommand {
    return { id: `${prefix}.${name}`, title, section, shortcut, context, run };
  }

  if (input.capabilities.lock) {
    commands.push(action(input.locked ? "Unlock" : "Lock", "lock", "shift+l", () => input.setLocked(!input.locked)));
  }
  if (input.capabilities.changeAccess) {
    const makePrivate = input.access !== PAGE_ACCESS_PRIVATE;
    commands.push(
      action(makePrivate ? "Make private" : "Make public", "access", "shift+a", () =>
        input.setAccess(makePrivate ? PAGE_ACCESS_PRIVATE : PAGE_ACCESS_PUBLIC)
      )
    );
  }
  if (input.capabilities.archive) {
    commands.push(
      action(input.archived ? "Restore" : "Archive", "archive", "shift+r", () => input.setArchived(!input.archived))
    );
  }
  if (input.capabilities.favorite) {
    commands.push(
      action(input.favorite ? "Remove from favorites" : "Add to favorites", "favorite", "shift+f", () =>
        input.toggleFavorite()
      )
    );
  }
  commands.push(action("Copy URL", "copy-url", "mod+shift+,", () => input.copyUrl()));
  return commands;
}

/**
 * Derive page capabilities from ownership and project standing. Owners
 * and project admins lock, change access and archive; members and
 * better favorite. The page's project ids collapse to the caller's
 * highest project role (computed by the mounting route).
 */
export function selectPageCapabilities(input: {
  isOwner: boolean;
  highestProjectRole: number | null;
}): PageCapabilities {
  const manage = input.isOwner || input.highestProjectRole === 20;
  const favorite = input.isOwner || (input.highestProjectRole !== null && input.highestProjectRole >= 15);
  return { lock: manage, changeAccess: manage, archive: manage, favorite };
}
