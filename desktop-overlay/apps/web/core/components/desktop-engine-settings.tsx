/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useCallback, useEffect, useState } from "react";
import { observer } from "mobx-react";
import { useParams } from "react-router";
import useSWR from "swr";
import { Button } from "@pi-dash/propel/button";
import { EModalPosition, EModalWidth, ModalCore } from "@pi-dash/ui";
import { useUser } from "@/hooks/store/user";
import { pickFolder, useDiscovery } from "@/components/runners/this-computer";
import {
  compatibleHello,
  discoverDaemon,
  discoveryProblem,
  engineActionFor,
  engineSignInState,
  invoke,
  pauseControl,
  setEnginePaused,
  signInEngineExplicitly,
  signOutEngine,
  signInWorkspace,
  type Discovery,
  type EngineSettings,
  type LocalRunner,
} from "@/services/desktop-daemon";
import { requestDaemonSetup } from "@/services/pidash-cli";

let openSettings: (() => void) | null = null;

/** Open the Built-in engine settings from anywhere in the app. */
export function openEngineSettings() {
  openSettings?.();
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const withoutTrailingSeparator = (p: string) => p.replace(/[\\/]+$/, "");

/** Whether `folder` is inside `root` (both absolute, as the daemon reports them). */
export function isUnder(folder: string, root: string): boolean {
  const f = withoutTrailingSeparator(folder);
  const r = withoutTrailingSeparator(root);
  return f === r || f.startsWith(`${r}/`) || f.startsWith(`${r}\\`);
}

const SIGN_IN_TEXT: Record<string, string> = {
  never: "Never signed in",
  active: "Signed in",
  expiring: "Signed in, expiring soon",
  expired: "Sign-in expired",
  signed_out: "Signed out",
  revoked: "Signed out by an administrator or by removing this computer",
};

/** `DevMachine.engine_auto_pause_reason`, in the user's words. */
const AUTO_PAUSE_TEXT: Record<string, string> = {
  engine_sign_in_required: "the built-in engine needs to be signed in",
  gateway_session_revoked: "its sign-in was revoked",
  wallet_empty: "your wallet is empty",
  profile_unavailable: "its settings could not be loaded",
  engine_disabled: "the built-in engine is turned off on this server",
};

/** The daemon section: what is wrong, and what the user can do about it. */
function DaemonStatus(props: {
  discovery: Discovery | null;
  workspaceSlug: string;
  busy: (p: Promise<unknown>) => void;
}) {
  const { discovery, workspaceSlug, busy } = props;
  const { signOut } = useUser();
  const problem = discoveryProblem(discovery);
  if (!discovery) return <p className="text-13 text-secondary">{problem}</p>;
  switch (discovery.kind) {
    case "compatible": {
      const { hello } = discovery;
      return (
        <p className="text-13 text-secondary">
          Pi Dash {hello.daemon_version} is running on this computer
          {hello.account?.email ? `, signed in as ${hello.account.email}` : ""}. Workspaces:{" "}
          {hello.workspaces.map((w) => `${w.slug}${w.signed_in ? "" : " (signed out)"}`).join(", ") || "none"}.
        </p>
      );
    }
    case "no_daemon":
    case "signed_out":
      return (
        <div className="flex flex-col gap-2">
          <p className="text-13 text-secondary">{problem}</p>
          <div>
            <Button onClick={() => busy(requestDaemonSetup("engine", workspaceSlug))}>
              {discovery.kind === "signed_out" ? "Sign in Pi Dash on this computer" : "Set up Pi Dash on this computer"}
            </Button>
          </div>
        </div>
      );
    case "too_old":
      return (
        <div className="flex flex-col gap-2">
          <p className="text-13 text-secondary">{problem}</p>
          {discovery.cli.updatable ? (
            <div>
              <Button onClick={() => busy(invoke("daemon_update_cli").then(() => discoverDaemon()))}>
                Update Pi Dash on this computer
              </Button>
            </div>
          ) : (
            <p className="text-12 text-tertiary">{discovery.cli.update_hint ?? "Run `pidash update` in a terminal."}</p>
          )}
        </div>
      );
    case "app_too_old":
    case "not_answering":
      return <p className="text-13 text-secondary">{problem}</p>;
    case "account_mismatch":
      return (
        <div className="flex flex-col gap-2">
          <p className="text-13 text-secondary">
            {problem} The built-in engine and local chat are off until both use the same account. Everything else in the
            app keeps working.
          </p>
          <div className="flex flex-wrap gap-2">
            <Button variant="secondary" onClick={() => busy(signOut())}>
              Sign in to the app as {discovery.daemon_account}
            </Button>
            {discovery.hosts_runners ? (
              <p className="text-12 text-tertiary">
                Pi Dash on this computer hosts runners. To use it with this account, run <code>pidash auth logout</code>{" "}
                in a terminal first.
              </p>
            ) : (
              <Button
                onClick={() =>
                  busy(
                    invoke("pidash_cli_logout", { engine: false }).then(() =>
                      signInWorkspace(workspaceSlug, { force: true })
                    )
                  )
                }
              >
                Sign Pi Dash on this computer in as me
              </Button>
            )}
          </div>
        </div>
      );
  }
}

/**
 * Built-in engine settings — the app's settings page for this computer
 * (design §10, §4.3, §8): the daemon's state, the engine's sign-in, pause and
 * resume, the task root with a folder picker, and each project's folder with
 * "Use the default location" / "Switch all". Every change goes to the daemon,
 * which validates it; its refusal is shown as is.
 */
export const DesktopEngineSettings = observer(function DesktopEngineSettings() {
  const { workspaceSlug = "" } = useParams<{ workspaceSlug?: string }>();
  const [isOpen, setIsOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const discovery = useDiscovery();
  const hello = compatibleHello(discovery);

  useEffect(() => {
    openSettings = () => {
      setError(null);
      setIsOpen(true);
      void discoverDaemon().catch(() => null);
    };
    return () => {
      openSettings = null;
    };
  }, []);

  const { data: settings, mutate: mutateSettings } = useSWR<EngineSettings>(
    isOpen && hello ? "desktop-engine-settings" : null,
    () => invoke<EngineSettings>("daemon_engine_settings_get")
  );
  const { data: runners, mutate: mutateRunners } = useSWR<LocalRunner[]>(
    isOpen && hello ? "desktop-engine-runners" : null,
    () => invoke<LocalRunner[]>("daemon_runners")
  );
  const engineRunners = (runners ?? []).filter((r) => r.agent === "engine");

  const busy = useCallback(
    (work: Promise<unknown>) => {
      setWorking(true);
      setError(null);
      void work
        .catch((e: unknown) => setError(message(e)))
        .finally(() => {
          setWorking(false);
          void discoverDaemon().catch(() => null);
          void mutateSettings();
          void mutateRunners();
        });
    },
    [mutateSettings, mutateRunners]
  );

  const chooseRoot = async () => {
    const picked = await pickFolder(settings?.task_root).catch((e: unknown) => {
      setError(message(e));
      return null;
    });
    if (picked) busy(invoke("daemon_engine_settings_set", { taskRoot: picked }));
  };

  const rebind = (runner: string) => invoke("daemon_engine_runner_rebind", { runner });
  const switchAll = async () => {
    const root = settings?.task_root ?? "";
    for (const r of engineRunners) {
      // eslint-disable-next-line no-await-in-loop -- the daemon reconfigures one runner at a time
      if (!isUnder(r.working_dir, root)) await rebind(r.name);
    }
  };

  const state = engineSignInState(hello);
  const action = engineActionFor(state);
  const detail = hello?.engine_detail;
  const pause = pauseControl(detail);

  return (
    <ModalCore
      isOpen={isOpen}
      handleClose={() => setIsOpen(false)}
      position={EModalPosition.CENTER}
      width={EModalWidth.XXXL}
    >
      <div data-prevent-outside-click className="flex flex-col gap-5 p-5">
        <h3 className="text-16 font-medium text-primary">Built-in engine on this computer</h3>

        <section className="flex flex-col gap-2">
          <h4 className="text-13 font-medium text-primary">Pi Dash on this computer</h4>
          <DaemonStatus discovery={discovery} workspaceSlug={workspaceSlug} busy={busy} />
        </section>

        {hello && (
          <>
            <section className="flex flex-col gap-2">
              <h4 className="text-13 font-medium text-primary">Built-in engine</h4>
              <p className="text-13 text-secondary">
                {hello.engine === "unavailable"
                  ? `Unavailable${detail?.reason_code ? ` (${detail.reason_code})` : ""}.`
                  : `${SIGN_IN_TEXT[state ?? "active"] ?? state}${
                      detail?.credential_expires_at
                        ? ` until ${new Date(detail.credential_expires_at).toLocaleDateString()}`
                        : ""
                    }.`}
                {detail?.model ? ` Model: ${detail.model}.` : ""}
              </p>
              <div className="flex flex-wrap gap-2">
                {action === "turn_on" && (
                  <Button onClick={() => busy(signInEngineExplicitly())} loading={working}>
                    Turn on the built-in engine
                  </Button>
                )}
                {action === "sign_in" && (
                  <Button onClick={() => busy(signInEngineExplicitly())} loading={working}>
                    Sign in the built-in engine
                  </Button>
                )}
                {(state === "active" || state === "expiring" || state === "expired") && (
                  <Button variant="secondary" onClick={() => busy(signOutEngine())}>
                    Sign out the built-in engine
                  </Button>
                )}
                {pause.action === "pause" && (
                  <Button variant="secondary" onClick={() => busy(setEnginePaused(true))} disabled={working}>
                    Pause
                  </Button>
                )}
                {pause.action === "resume" && (
                  <Button variant="secondary" onClick={() => busy(setEnginePaused(false))} disabled={working}>
                    Resume
                  </Button>
                )}
              </div>
              {pause.action === "resume" && (
                <p className="text-13 text-secondary">You paused the built-in engine on this computer.</p>
              )}
              {pause.autoReason && (
                <p className="text-13 text-warning-primary">
                  Pi Dash paused the built-in engine: {AUTO_PAUSE_TEXT[pause.autoReason] ?? pause.autoReason}.
                </p>
              )}
              <p className="text-12 text-tertiary">
                Pausing stops the built-in engine on this computer from taking new work. Your own agents keep working.
              </p>
            </section>

            <section className="flex flex-col gap-2">
              <h4 className="text-13 font-medium text-primary">Task folders</h4>
              <p className="text-13 text-secondary">
                New projects get a folder under <code className="text-12">{settings?.task_root ?? "…"}</code>
                {settings?.task_root_is_default ? " (the default)" : ""}. Changing it moves nothing: projects already
                set up keep their folder.
              </p>
              {settings?.warnings?.map((w) => (
                <p key={w} className="text-12 text-warning-primary">
                  {w}
                </p>
              ))}
              <div className="flex flex-wrap gap-2">
                <Button variant="secondary" onClick={() => void chooseRoot()} disabled={working}>
                  Choose folder…
                </Button>
                {settings && !settings.task_root_is_default && (
                  <Button
                    variant="secondary"
                    onClick={() => busy(invoke("daemon_engine_settings_set", { taskRoot: null }))}
                    disabled={working}
                  >
                    Use the default folder
                  </Button>
                )}
              </div>
              {engineRunners.length > 0 && (
                <div className="flex flex-col gap-1">
                  <div className="flex items-center justify-between">
                    <span className="text-12 font-medium text-primary">Projects</span>
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => busy(switchAll())}
                      disabled={
                        working || engineRunners.every((r) => isUnder(r.working_dir, settings?.task_root ?? ""))
                      }
                    >
                      Switch all
                    </Button>
                  </div>
                  <ul className="flex flex-col divide-y divide-subtle rounded border border-subtle">
                    {engineRunners.map((r) => (
                      <li key={r.runner_id} className="flex items-center gap-2 px-3 py-2 text-12">
                        <span className="w-40 shrink-0 truncate text-primary">
                          {r.workspace_slug}/{r.project_slug}
                        </span>
                        <code className="min-w-0 flex-1 truncate text-secondary" title={r.working_dir}>
                          {r.working_dir}
                        </code>
                        {settings && !isUnder(r.working_dir, settings.task_root) && (
                          <Button variant="secondary" size="sm" onClick={() => busy(rebind(r.name))} disabled={working}>
                            Use the default location
                          </Button>
                        )}
                      </li>
                    ))}
                  </ul>
                  <p className="text-12 text-tertiary">
                    “Use the default location” gives the project a fresh folder under the current root; the old one
                    stays on disk.
                  </p>
                </div>
              )}
            </section>
          </>
        )}

        {error && <p className="text-13 text-danger-primary">{error}</p>}
        <div className="flex justify-end">
          <Button variant="secondary" onClick={() => setIsOpen(false)}>
            Close
          </Button>
        </div>
      </div>
    </ModalCore>
  );
});
