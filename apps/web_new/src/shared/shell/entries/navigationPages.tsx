// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Entity picker sub-pages (shell spec, SHELL-087). The project and
// workspace entries open these filterable lists; picking navigates. The
// route feeds already-loaded rows in, so the pages stay pure given data
// and never reach into features for queries.

import type { Project, Workspace } from "@pidash/api-client";

import { filterPageOptions, useCommandPalette, type PalettePage, type PalettePageState } from "../../commands/index.js";
import { PROJECT_PICKER_PAGE_ID, WORKSPACE_PICKER_PAGE_ID } from "./navigation.js";

/** Project rows as the picker needs them. */
export type PickerProject = Pick<Project, "id" | "name" | "identifier">;
/** Workspace rows as the picker needs them. */
export type PickerWorkspace = Pick<Workspace, "id" | "name" | "slug">;

export function projectPickerPage(args: {
  /** Undefined while the route resolves the list (spins, SHELL-093). */
  projects: PickerProject[] | undefined;
  onPick: (projectId: string) => void;
}): PalettePage {
  return {
    id: PROJECT_PICKER_PAGE_ID,
    placeholder: "Open a project",
    usePageState: (query: string): PalettePageState => {
      const closePalette = useCommandPalette((state) => state.closePalette);
      if (args.projects === undefined) {
        return { status: "loading", options: [], emptyText: "No projects found" };
      }
      return {
        status: "ready",
        options: filterPageOptions(
          args.projects.map((project) => ({
            id: project.id,
            title: project.name,
            hint: project.identifier,
            run: () => {
              closePalette();
              args.onPick(project.id);
            },
          })),
          query
        ),
        emptyText: "No projects found",
      };
    },
  };
}

export function workspacePickerPage(args: {
  /** Undefined while the route resolves the list (spins, SHELL-093). */
  workspaces: PickerWorkspace[] | undefined;
  onPick: (workspaceSlug: string) => void;
}): PalettePage {
  return {
    id: WORKSPACE_PICKER_PAGE_ID,
    placeholder: "Open a workspace",
    usePageState: (query: string): PalettePageState => {
      const closePalette = useCommandPalette((state) => state.closePalette);
      if (args.workspaces === undefined) {
        return { status: "loading", options: [], emptyText: "No workspaces found" };
      }
      return {
        status: "ready",
        options: filterPageOptions(
          args.workspaces.map((workspace) => ({
            id: workspace.id,
            title: workspace.name,
            hint: workspace.slug,
            run: () => {
              closePalette();
              args.onPick(workspace.slug);
            },
          })),
          query
        ),
        emptyText: "No workspaces found",
      };
    },
  };
}
