/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

/**
 * Turning this computer into a Pi Dash runner host, from inside the app.
 *
 * Installing the desktop app should be enough to host a runner — including for
 * a user who brings their own agent (Claude Code, Codex, Cursor) rather than
 * the built-in one. That path runs through the `pidash` CLI, which the app
 * already ships and can install from its own bundle, and which then needs a
 * credential.
 *
 * Signing the CLI in is a device-code grant, and the app is already signed in,
 * so it can play both halves of the web `/auth/device/` page: start the grant,
 * then approve it with its own session. The approved code goes to the CLI,
 * which exchanges it and writes its own config — the machine token, the
 * workspace binding and the file format stay in the runner where they belong.
 *
 * What is deliberately *not* here: `pidash runner add`. Registering a runner
 * picks a project, a working directory and an agent, which is a UI flow of its
 * own rather than a credential step.
 */

import { API_BASE_URL } from "@pi-dash/constants";
import { APIService } from "@pi-dash/services";
import { CSRF_TOKEN_PATH } from "@/pi-dash-web/components/desktop/agent-runtime-edition";
import { isDesktop } from "@/services/agent-runtime";

class CliAPI extends APIService {}
const cliAPI = new CliAPI(API_BASE_URL);

type Native = { core: { invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> } };

function invoke<T>(command: string, args?: Record<string, unknown>) {
  return (window as unknown as { __TAURI__: Native }).__TAURI__.core.invoke<T>(command, args);
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const csrf = await cliAPI.request<{ csrf_token: string }>({ url: CSRF_TOKEN_PATH, method: "GET" });
  const response = await cliAPI.request<T>({
    url: path,
    method: "POST",
    headers: { "Content-Type": "application/json", "X-CSRFTOKEN": csrf.data.csrf_token },
    data: body,
  });
  return response.data;
}

export interface CliStatus {
  installed: boolean;
  path: string | null;
  version: string | null;
}

/** Is the CLI on this machine, and which version? */
export async function detectCli(): Promise<CliStatus> {
  if (!isDesktop()) return { installed: false, path: null, version: null };
  return invoke<CliStatus>("detect_pidash_cli");
}

/**
 * Install the CLI from the app's own bundle (no download), then sign it in
 * with a grant this app approves. Progress arrives on the `pidash-install-log`
 * Tauri event, which the caller can subscribe to.
 */
export async function setUpCliForThisMachine(workspaceSlug: string): Promise<void> {
  if (!isDesktop()) return;
  const status = await detectCli();
  if (!status.installed) await invoke<void>("install_pidash_cli");

  // Both halves of the web `/auth/device/` page, run by the app.
  const grant = await post<{ device_code: string; user_code: string }>("/api/v1/auth/device/start/", {});
  await post("/api/v1/auth/device/approve/", { user_code: grant.user_code });

  // The CLI finishes the exchange and owns everything it writes.
  await invoke<void>("pidash_cli_login", {
    deviceCode: grant.device_code,
    cloudUrl: API_BASE_URL,
    workspace: workspaceSlug,
  });
}
