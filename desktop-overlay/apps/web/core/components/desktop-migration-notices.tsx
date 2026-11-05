/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useEffect, useState } from "react";
import { Button } from "@pi-dash/propel/button";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import {
  dismissNotices,
  folderKey,
  hasNotices,
  loadNotices,
  podKey,
  setPodWork,
  subscribeNotices,
  type Notices,
} from "@/services/desktop-notices";

/**
 * The one-time notices after moving to Pi Dash on this computer (design §13):
 * the older builds' task folders left on disk, and the projects whose
 * built-in engine now takes pod work (PO-11), each with a switch to turn that
 * off. "Got it" dismisses what was shown, for good.
 */
export function DesktopMigrationNotices(props: { account: string }) {
  const { account } = props;
  const [notices, setNotices] = useState<Notices | null>(null);
  /** Pod work per runner id, as the switches show it (on until turned off). */
  const [podWork, setPodWorkState] = useState<Record<string, boolean>>({});
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const view = await loadNotices(account);
      setNotices(hasNotices(view.notices) ? view.notices : null);
    } catch {
      setNotices(null);
    }
  }, [account]);

  useEffect(() => {
    void refresh();
    return subscribeNotices(() => void refresh());
  }, [refresh]);

  const toggle = async (runnerId: string, on: boolean) => {
    setPending(runnerId);
    setError(null);
    try {
      await setPodWork(runnerId, on);
      setPodWorkState((s) => ({ ...s, [runnerId]: on }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setPending(null);
    }
  };

  const dismiss = async () => {
    if (!notices) return;
    const keys = [...notices.legacy_folders.map(folderKey), ...notices.pod_member_projects.map(podKey)];
    setNotices(null);
    await dismissNotices(account, keys).catch(() => {});
  };

  return (
    <ModalCore
      isOpen={notices !== null}
      handleClose={() => void dismiss()}
      position={EModalPosition.CENTER}
      width={EModalWidth.XXL}
    >
      {notices && (
        <div data-prevent-outside-click className="flex flex-col gap-4 p-5">
          <h3 className="text-16 font-medium text-primary">This app now uses Pi Dash on this computer</h3>
          {notices.pod_member_projects.length > 0 && (
            <section className="flex flex-col gap-2">
              <p className="text-13 text-secondary">
                The built-in engine on this computer now takes work from these projects&apos; pods. Your issues set to
                “Runner” there can go to it and spend your wallet, also while the app is closed. Turn pod work off to
                have it run only the issues pinned to it.
              </p>
              <ul className="flex flex-col gap-1">
                {notices.pod_member_projects.map((n) => {
                  const on = podWork[n.runner_id] ?? true;
                  return (
                    <li key={n.runner_id} className="flex items-center justify-between gap-2 text-13 text-primary">
                      <span>
                        {n.workspace} / {n.project}
                      </span>
                      <label className="flex items-center gap-2 text-12 text-secondary">
                        <input
                          type="checkbox"
                          role="switch"
                          aria-label={`Pod work in ${n.project}`}
                          checked={on}
                          disabled={pending === n.runner_id}
                          onChange={(e) => void toggle(n.runner_id, e.target.checked)}
                        />
                        Pod work
                      </label>
                    </li>
                  );
                })}
              </ul>
            </section>
          )}
          {notices.legacy_folders.length > 0 && (
            <section className="flex flex-col gap-2">
              <p className="text-13 text-secondary">
                These task folders from the older app are no longer used — Pi Dash on this computer already had its own
                for the project. They were left in place; copy anything you need, then delete them.
              </p>
              <ul className="flex flex-col gap-1">
                {notices.legacy_folders.map((n) => (
                  <li key={n.path} className="text-13 text-primary">
                    {n.workspace} / {n.project}: <code className="text-12 break-all">{n.path}</code>
                  </li>
                ))}
              </ul>
            </section>
          )}
          {error && <p className="text-13 text-danger-primary">Could not change pod work: {error}</p>}
          <div className="flex justify-end">
            <Button onClick={() => void dismiss()} disabled={pending !== null}>
              Got it
            </Button>
          </div>
        </div>
      )}
    </ModalCore>
  );
}
