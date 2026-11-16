// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Authentication landing page content (ADM-023–026, ADM-036, from
// parity/specs/admin.md §4.1–4.4, §4.14): the open-sign-up switch plus
// the six method cards over the shared configurations fetch. Every
// flip writes its single key with a progress notice that settles into
// success or failure; switching off the lone enabled method is refused
// with an error notice and no request. All switches lock while a save
// flies, and a five-bar skeleton stands in until config loads. Notice
// wording is pinned by the parity scenarios; headings are fresh.
import { showToast, Skeleton, Switch } from "@pidash/kit";
import * as React from "react";

import { getToastManager, notifyError, notifySuccess, useDocumentHead } from "../../../shared/shell/index.js";
import {
  AUTH_METHODS,
  canDisableAuthMethod,
  ENABLE_SIGNUP_KEY,
  isConfigFlagOn,
  isMethodConfigured,
  toConfigMap,
} from "../authMethods.js";
import { useInstanceConfigurations, useUpdateInstanceConfigurations } from "../hooks.js";
import { AdminAuthMethodCard } from "./AdminAuthMethodCard.js";

export const AUTH_LANDING_TITLE = "Authentication — Pi Dash instance admin";

/** Skeleton bars while configuration loads, pinned by the parity scenario. */
export const AUTH_LANDING_SKELETON_BARS = 5;

export const AUTH_SAVE_PROGRESS_TITLE = "Saving configuration";
export const AUTH_SAVE_SUCCESS_TITLE = "Success";
export const AUTH_SAVE_SUCCESS_MESSAGE = "Configuration saved successfully";
export const AUTH_SAVE_FAILED_TITLE = "Error";
export const AUTH_SAVE_FAILED_MESSAGE = "Failed to save configuration";
export const AUTH_GUARD_TITLE = "Cannot disable authentication";
export const AUTH_GUARD_MESSAGE =
  "At least one authentication method must remain enabled. Please enable another method before disabling this one.";

export function AdminAuthLanding(): React.ReactElement {
  useDocumentHead({ title: AUTH_LANDING_TITLE, robots: "noindex, nofollow" });
  const configurations = useInstanceConfigurations();
  const save = useUpdateInstanceConfigurations();
  const config = React.useMemo(
    () => (configurations.data === undefined ? null : toConfigMap(configurations.data)),
    [configurations.data]
  );

  if (config === null) {
    return (
      <div
        data-testid="admin-auth-landing"
        className="mx-auto flex w-full max-w-3xl flex-col gap-(--space-6) p-(--space-6)"
      >
        <div className="flex flex-col gap-(--space-1)">
          <h1 className="text-h2 text-(--text)">Authentication</h1>
          <p className="text-body text-(--text-muted)">Choose how members sign in to this Pi Dash instance.</p>
        </div>
        <div
          data-testid="admin-auth-skeleton"
          role="status"
          aria-label="Loading authentication settings"
          className="flex flex-col gap-(--space-3)"
        >
          {Array.from({ length: AUTH_LANDING_SKELETON_BARS }, (_, index) => (
            <Skeleton key={index} height="50px" width={index % 2 === 0 ? "75%" : "40%"} />
          ))}
        </div>
      </div>
    );
  }

  const flip = (enableKey: string, next: boolean) => {
    if (!next && !canDisableAuthMethod(enableKey, config)) {
      notifyError(AUTH_GUARD_TITLE, AUTH_GUARD_MESSAGE);
      return;
    }
    const manager = getToastManager();
    const progressId = showToast(manager, { title: AUTH_SAVE_PROGRESS_TITLE, timeout: 0 });
    save.mutate(
      { [enableKey]: next ? "1" : "0" },
      {
        onSuccess: () => {
          manager.close(progressId);
          notifySuccess(AUTH_SAVE_SUCCESS_TITLE, AUTH_SAVE_SUCCESS_MESSAGE);
        },
        onError: () => {
          manager.close(progressId);
          notifyError(AUTH_SAVE_FAILED_TITLE, AUTH_SAVE_FAILED_MESSAGE);
        },
      }
    );
  };

  const saving = save.isPending;
  return (
    <div
      data-testid="admin-auth-landing"
      className="mx-auto flex w-full max-w-3xl flex-col gap-(--space-6) p-(--space-6)"
    >
      <div className="flex flex-col gap-(--space-1)">
        <h1 className="text-h2 text-(--text)">Authentication</h1>
        <p className="text-body text-(--text-muted)">Choose how members sign in to this Pi Dash instance.</p>
      </div>
      <div
        aria-busy={saving}
        className={saving ? "flex flex-col gap-(--space-3) opacity-70" : "flex flex-col gap-(--space-3)"}
      >
        <div className="flex w-full items-center gap-(--space-4) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-4) py-(--space-3)">
          <div className="min-w-0 flex-1">
            <p id="admin-auth-signup-label" className="text-emphasis font-medium text-(--text)">
              Open sign-up
            </p>
            <p className="text-body text-(--text-muted)">When this is off, joining this instance requires an invite.</p>
          </div>
          <Switch
            checked={isConfigFlagOn(config[ENABLE_SIGNUP_KEY])}
            disabled={saving}
            onCheckedChange={(next) => flip(ENABLE_SIGNUP_KEY, next)}
            aria-labelledby="admin-auth-signup-label"
            data-testid="admin-auth-signup"
          />
        </div>
        <h2 className="text-h3 pt-(--space-3) text-(--text)">Sign-in methods</h2>
        {AUTH_METHODS.map((method) => (
          <AdminAuthMethodCard
            key={method.key}
            method={method}
            enabled={isConfigFlagOn(config[method.enableKey])}
            configured={isMethodConfigured(method, config)}
            saving={saving}
            onToggle={(next) => flip(method.enableKey, next)}
          />
        ))}
      </div>
    </div>
  );
}
