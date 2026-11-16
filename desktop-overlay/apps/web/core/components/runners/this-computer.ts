/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Desktop override of the "this computer" seam: the computer's own Pi Dash
 * daemon, as the app last discovered it (design §12.2).
 */

import { useMemo, useSyncExternalStore } from "react";
import useSWR from "swr";
import { turnOnEngineForProject } from "@/services/agent-runtime";
import {
  compatibleHello,
  currentDiscovery,
  invoke,
  isDesktopHost,
  subscribeDiscovery,
  type LocalRunner,
} from "@/services/desktop-daemon";
import { requestDaemonSetup } from "@/services/pidash-cli";

/** Same shape as the web seam (`apps/web/core/components/runners/this-computer.ts`). */
export interface ThisComputer {
  machineId?: string;
  localEngineRunnerIds: ReadonlySet<string>;
  pickFolder?: (defaultPath?: string) => Promise<string | null>;
  addEngineRunner?: (input: { workspaceSlug: string; project: string; workingDir?: string }) => Promise<string>;
  setUp?: (workspaceSlug: string) => Promise<boolean>;
}

interface TauriDialog {
  dialog?: { open(options: { directory: boolean; multiple: boolean; defaultPath?: string }): Promise<string | null> };
}

/** The native folder picker (`dialog:allow-open`). */
export async function pickFolder(defaultPath?: string): Promise<string | null> {
  const tauri = (window as unknown as { __TAURI__: TauriDialog }).__TAURI__;
  if (!tauri.dialog) throw new Error("The folder picker is not available in this build.");
  const picked = await tauri.dialog.open({ directory: true, multiple: false, defaultPath });
  return typeof picked === "string" ? picked : null;
}

/** The current discovery, re-rendering when it changes. */
export function useDiscovery() {
  return useSyncExternalStore(subscribeDiscovery, currentDiscovery, () => null);
}

const NONE: ThisComputer = { localEngineRunnerIds: new Set() };

export function useThisComputer(): ThisComputer {
  const discovery = useDiscovery();
  const hello = compatibleHello(discovery);
  const { data: runners } = useSWR<LocalRunner[]>(
    isDesktopHost() && hello ? ["desktop-daemon-runners", hello.machine_id ?? ""] : null,
    () => invoke<LocalRunner[]>("daemon_runners"),
    { refreshInterval: 30_000 }
  );
  return useMemo(() => {
    if (!isDesktopHost()) return NONE;
    return {
      machineId: hello?.machine_id,
      localEngineRunnerIds: new Set((runners ?? []).filter((r) => r.agent === "engine").map((r) => r.runner_id)),
      pickFolder,
      addEngineRunner: async ({ workspaceSlug, project, workingDir }) => {
        const added = await turnOnEngineForProject(workspaceSlug, project, workingDir);
        return "runner" in added ? added.runner : added.name;
      },
      setUp: (workspaceSlug) => requestDaemonSetup("runner", workspaceSlug),
    };
  }, [hello?.machine_id, runners]);
}
