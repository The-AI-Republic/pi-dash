// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Per-member tab preferences (parity/specs/shell.md §7, SHELL-079). The
// default tab and hidden list persist on the project user-properties
// endpoint; writes apply optimistically, roll back on failure, and toast.
// Sibling preference branches ride along untouched.

import { useMutation, useQueryClient } from "@tanstack/react-query";
import { updateProjectUserProperties, type ProjectUserProperties } from "@pidash/api-client";

import { getAppClient } from "../../../core/api/client.js";
import { applyOptimisticUpdate } from "../../../core/query/optimistic.js";
import { notifyError, notifySuccess } from "../../../shared/shell/index.js";
import { DEFAULT_TAB_KEY, parseNavigationPrefs, type TabPreferences } from "../tabs.js";
import { projectKeys } from "./keys.js";
import { useProjectProperties } from "./queries.js";

const DEFAULT_SAVED_TITLE = "Default tab saved";
const DEFAULT_SAVED_BODY = "Opening this project will land there from now on.";
const DEFAULT_CLEARED_TITLE = "Default tab cleared";
const DEFAULT_CLEARED_BODY = "Opening this project will land on Work Items.";
const PREFS_FAILED_TITLE = "Couldn't save the tab choice";
const PREFS_FAILED_BODY = "Your tabs are unchanged — check your connection and try again.";

interface SaveInput {
  navigation: TabPreferences;
  siblings: Record<string, unknown>;
  announce: "saved" | "cleared" | "silent";
}

/** Full preferences payload: the navigation branch over kept siblings. */
export function mergeNavigationPrefs(
  siblings: Record<string, unknown>,
  navigation: TabPreferences
): Record<string, unknown> {
  return {
    ...siblings,
    navigation: { default_tab: navigation.defaultTab, hide_in_more_menu: navigation.hiddenTabs },
  };
}

/** Optimistic next payload, or undefined while nothing is cached. */
export function applyNavigationPrefs(
  previous: ProjectUserProperties | undefined,
  navigation: TabPreferences
): ProjectUserProperties | undefined {
  if (previous === undefined) return undefined;
  return { ...previous, preferences: mergeNavigationPrefs(previous.preferences, navigation) };
}

export interface TabPreferencesHandle {
  preferences: TabPreferences;
  isSaving: boolean;
  toggleDefault: (tabKey: string) => void;
  hideTab: (tabKey: string) => void;
  showTab: (tabKey: string) => void;
}

export function useTabPreferences(workspaceSlug: string, projectId: string): TabPreferencesHandle {
  const queryClient = useQueryClient();
  const properties = useProjectProperties(workspaceSlug, projectId);
  const preferences = parseNavigationPrefs(properties.data?.preferences);
  const key = projectKeys.projectProperties(workspaceSlug, projectId);

  const mutation = useMutation({
    mutationFn: async (input: SaveInput) => {
      const rollback = applyOptimisticUpdate<ProjectUserProperties>(queryClient, key, (previous) =>
        applyNavigationPrefs(previous, input.navigation)
      );
      try {
        return await updateProjectUserProperties(getAppClient(), workspaceSlug, projectId, {
          preferences: mergeNavigationPrefs(input.siblings, input.navigation),
        });
      } catch (error) {
        rollback();
        notifyError(PREFS_FAILED_TITLE, PREFS_FAILED_BODY);
        throw error;
      }
    },
    onSuccess: (_data, input) => {
      if (input.announce === "saved") notifySuccess(DEFAULT_SAVED_TITLE, DEFAULT_SAVED_BODY);
      else if (input.announce === "cleared") notifySuccess(DEFAULT_CLEARED_TITLE, DEFAULT_CLEARED_BODY);
    },
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: key });
    },
  });

  function save(input: Omit<SaveInput, "siblings">): void {
    // No cached payload yet: the menus stay hidden until it loads, and a
    // blind write would clobber sibling preference branches.
    const stored = properties.data;
    if (stored === undefined) return;
    mutation.mutate({ ...input, siblings: stored.preferences });
  }

  return {
    preferences,
    isSaving: mutation.isPending,
    toggleDefault: (tabKey) => {
      const clearing = tabKey === preferences.defaultTab;
      save({
        navigation: {
          defaultTab: clearing ? DEFAULT_TAB_KEY : tabKey,
          hiddenTabs: preferences.hiddenTabs,
        },
        announce: clearing ? "cleared" : "saved",
      });
    },
    hideTab: (tabKey) => {
      save({
        navigation: {
          defaultTab: preferences.defaultTab,
          hiddenTabs: preferences.hiddenTabs.includes(tabKey)
            ? preferences.hiddenTabs
            : [...preferences.hiddenTabs, tabKey],
        },
        announce: "silent",
      });
    },
    showTab: (tabKey) => {
      save({
        navigation: {
          defaultTab: preferences.defaultTab,
          hiddenTabs: preferences.hiddenTabs.filter((key) => key !== tabKey),
        },
        announce: "silent",
      });
    },
  };
}
