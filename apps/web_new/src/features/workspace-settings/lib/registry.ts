// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Page registry seam for workspace settings (WSET-001/036). The shell
// owns the frame, gate and addresses; each page slice registers its body
// under its section key from its own chunk. A section without a body
// renders the shell's quiet placeholder — never a fake page or a dead
// route. Registration returns an unregister callback for slice unmounts.

import * as React from "react";

import type { WorkspaceSettingsSectionKey } from "./sections.js";

export interface WorkspaceSettingsPageProps {
  workspaceSlug: string;
  /** Navigation seam the shell owns; a page that leaves the workspace asks it to move on. */
  onLeaveWorkspace?: (() => void) | undefined;
}

export type WorkspaceSettingsPageComponent = (props: WorkspaceSettingsPageProps) => React.ReactNode;

export interface WorkspaceWebhookDetailPageProps extends WorkspaceSettingsPageProps {
  webhookId: string;
}

export type WorkspaceWebhookDetailPageComponent = (props: WorkspaceWebhookDetailPageProps) => React.ReactNode;

interface PageRegistry {
  pages: Partial<Record<WorkspaceSettingsSectionKey, WorkspaceSettingsPageComponent>>;
  webhookDetail: WorkspaceWebhookDetailPageComponent | null;
}

const EMPTY: PageRegistry = { pages: {}, webhookDetail: null };
let registry: PageRegistry = EMPTY;
const listeners = new Set<() => void>();

function emit(): void {
  for (const listener of listeners) listener();
}

export function registerWorkspaceSettingsPage(
  section: WorkspaceSettingsSectionKey,
  page: WorkspaceSettingsPageComponent
): () => void {
  registry = { ...registry, pages: { ...registry.pages, [section]: page } };
  emit();
  return () => {
    if (registry.pages[section] !== page) return;
    const pages = { ...registry.pages };
    delete pages[section];
    registry = { ...registry, pages };
    emit();
  };
}

export function registerWorkspaceWebhookDetailPage(page: WorkspaceWebhookDetailPageComponent): () => void {
  registry = { ...registry, webhookDetail: page };
  emit();
  return () => {
    if (registry.webhookDetail !== page) return;
    registry = { ...registry, webhookDetail: null };
    emit();
  };
}

export function getWorkspaceSettingsPage(section: WorkspaceSettingsSectionKey): WorkspaceSettingsPageComponent | null {
  return registry.pages[section] ?? null;
}

export function getWorkspaceWebhookDetailPage(): WorkspaceWebhookDetailPageComponent | null {
  return registry.webhookDetail;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The registered page for a section, re-rendering when it registers. */
export function useWorkspaceSettingsPage(section: WorkspaceSettingsSectionKey): WorkspaceSettingsPageComponent | null {
  return React.useSyncExternalStore(
    subscribe,
    () => getWorkspaceSettingsPage(section),
    () => getWorkspaceSettingsPage(section)
  );
}

export function useWorkspaceWebhookDetailPage(): WorkspaceWebhookDetailPageComponent | null {
  return React.useSyncExternalStore(subscribe, getWorkspaceWebhookDetailPage, getWorkspaceWebhookDetailPage);
}

/** Clears every registration. Tests only; production pages unregister themselves. */
export function resetWorkspaceSettingsPages(): void {
  registry = EMPTY;
  emit();
}
