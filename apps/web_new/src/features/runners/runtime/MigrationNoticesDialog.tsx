// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// The one-time notices after moving to Pi Dash on this computer (design
// §13): the older builds' task folders left on disk, and the projects whose
// built-in engine now takes pod work (PO-11), each with a switch to turn
// that off. "Got it" dismisses what was shown, for good.

import { Button, Dialog, Switch } from "@pidash/kit";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { platform } from "../../../core/platform/index.js";
import {
  dismissNotices,
  folderKey,
  getServerRunner,
  hasNotices,
  loadNotices,
  podKey,
  setPodWork,
  subscribeNotices,
  type DesktopNotices,
} from "./desktop-notices.js";
import { replacePromptOpen, subscribeReplacePromptOpen } from "./desktop-replace.js";

export function MigrationNoticesDialog(props: { account: string }): React.ReactElement {
  const { account } = props;
  const runtime = platform.agentRuntime;
  const client = React.useMemo(() => getAppClient(), []);
  const [notices, setNotices] = React.useState<DesktopNotices | null>(null);
  /** Pod work per runner id, as the server has it; absent until read. */
  const [podWork, setPodWorkState] = React.useState<Record<string, boolean>>({});
  /** Wait while the "Replace it" dialog is open rather than stack on it. */
  const [replaceOpen, setReplaceOpen] = React.useState(replacePromptOpen());
  React.useEffect(() => subscribeReplacePromptOpen(() => setReplaceOpen(replacePromptOpen())), []);
  const [pending, setPending] = React.useState<string | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  const refresh = React.useCallback(async (): Promise<void> => {
    if (!runtime) {
      setNotices(null);
      return;
    }
    try {
      const view = await loadNotices(account, runtime);
      setNotices(hasNotices(view.notices) ? view.notices : null);
    } catch {
      setNotices(null);
    }
  }, [account, runtime]);

  React.useEffect(() => {
    void refresh();
    if (!runtime) return undefined;
    return subscribeNotices(() => void refresh());
  }, [refresh, runtime]);

  // The switches show the server's current pod work, not an assumed "on".
  const podRunnerIds = notices?.pod_member_projects.map((n) => n.runner_id).join(",") ?? "";
  React.useEffect(() => {
    if (!podRunnerIds) return;
    let current = true;
    for (const runnerId of podRunnerIds.split(",")) {
      void getServerRunner(client, runnerId)
        .then((row) => {
          if (current) setPodWorkState((state) => ({ ...state, [runnerId]: row.pod_dispatch === "pod_member" }));
        })
        // Unreadable: the switch stays as the notice recorded it (on).
        .catch(() => {
          if (current) setPodWorkState((state) => (runnerId in state ? state : { ...state, [runnerId]: true }));
        });
    }
    return () => {
      current = false;
    };
  }, [podRunnerIds, client]);

  const toggle = async (runnerId: string, on: boolean): Promise<void> => {
    setPending(runnerId);
    setError(null);
    try {
      await setPodWork(client, runnerId, on);
      setPodWorkState((state) => ({ ...state, [runnerId]: on }));
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      setPending(null);
    }
  };

  const dismiss = async (): Promise<void> => {
    if (!notices || !runtime) return;
    const keys = [...notices.legacy_folders.map(folderKey), ...notices.pod_member_projects.map(podKey)];
    setNotices(null);
    await dismissNotices(account, keys, runtime).catch(() => undefined);
  };

  return (
    <Dialog
      title="This app now uses Pi Dash on this computer"
      open={notices !== null && !replaceOpen}
      onOpenChange={(open) => {
        if (!open) void dismiss();
      }}
    >
      {notices ? (
        <div className="flex flex-col gap-(--space-4)" data-testid="migration-notices-dialog">
          {notices.pod_member_projects.length > 0 ? (
            <section className="flex flex-col gap-(--space-2)">
              <p className="text-body text-(--text)">
                The built-in engine on this computer now takes work from these projects&apos; pods. Your issues set to
                “Runner” there can go to it and spend your wallet, also while the app is closed. Turn pod work off to
                have it run only the issues pinned to it.
              </p>
              <ul className="flex flex-col gap-(--space-1)">
                {notices.pod_member_projects.map((notice) => {
                  const known = notice.runner_id in podWork;
                  const on = podWork[notice.runner_id] ?? true;
                  return (
                    <li
                      key={notice.runner_id}
                      className="text-body flex items-center justify-between gap-(--space-2) text-(--text)"
                    >
                      <span>
                        {notice.workspace} / {notice.project}
                      </span>
                      <span className="text-caption flex items-center gap-(--space-2) text-(--text-muted)">
                        <Switch
                          checked={on}
                          disabled={!known || pending === notice.runner_id}
                          label={`Pod work in ${notice.project}`}
                          onCheckedChange={(next) => void toggle(notice.runner_id, next)}
                        />
                        Pod work
                      </span>
                    </li>
                  );
                })}
              </ul>
            </section>
          ) : null}
          {notices.legacy_folders.length > 0 ? (
            <section className="flex flex-col gap-(--space-2)">
              <p className="text-body text-(--text)">
                These task folders from the older app are no longer used — Pi Dash on this computer already had its own
                for the project. They were left in place; copy anything you need, then delete them.
              </p>
              <ul className="flex flex-col gap-(--space-1)">
                {notices.legacy_folders.map((notice) => (
                  <li key={notice.path} className="text-body text-(--text)">
                    {notice.workspace} / {notice.project}: <code className="text-caption break-all">{notice.path}</code>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
          {error ? <p className="text-caption text-(--danger)">Could not change pod work: {error}</p> : null}
          <div className="flex justify-end">
            <Button onClick={() => void dismiss()} disabled={pending !== null}>
              Got it
            </Button>
          </div>
        </div>
      ) : null}
    </Dialog>
  );
}
