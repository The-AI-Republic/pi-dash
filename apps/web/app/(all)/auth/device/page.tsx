/**
 * Copyright (c) 2023-present Pi Dash Software, Inc. and contributors
 * SPDX-License-Identifier: AGPL-3.0-only
 * See the LICENSE file for details.
 */

import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router";
import { API_BASE_URL } from "@pi-dash/constants";
import { AuthenticationWrapper } from "@/lib/wrappers/authentication-wrapper";
import { EPageTypes } from "@/helpers/authentication.helper";
import { APIService } from "@/services/api.service";

class DeviceAuthService extends APIService {
  constructor() {
    super(API_BASE_URL);
  }

  approve(userCode: string) {
    return this.post(`/api/v1/auth/device/approve/`, { user_code: userCode }).then((r) => r.data);
  }
}

const deviceAuthService = new DeviceAuthService();

type Status = "idle" | "submitting" | "approved" | "error";

function normalizeUserCode(raw: string): string {
  return raw.replace(/[^A-Z0-9]/gi, "").toUpperCase();
}

function formatUserCode(raw: string): string {
  const clean = normalizeUserCode(raw).slice(0, 8);
  if (clean.length <= 4) return clean;
  return `${clean.slice(0, 4)}-${clean.slice(4)}`;
}

function DeviceAuthPage() {
  const [params] = useSearchParams();
  const [code, setCode] = useState<string>(() => formatUserCode(params.get("code") ?? ""));
  const [status, setStatus] = useState<Status>("idle");
  const [errorMessage, setErrorMessage] = useState<string>("");
  const [approvedFor, setApprovedFor] = useState<{ email: string; workspace: string | null } | null>(null);

  const normalized = useMemo(() => normalizeUserCode(code), [code]);
  const submittable = normalized.length === 8 && status !== "submitting";

  useEffect(() => {
    // If the CLI deep-linked with ?code=XXXX-YYYY, auto-submit on mount
    // so the human only has to click "Approve". We still let them edit
    // and resubmit if the auto-attempt errors.
    const prefilled = formatUserCode(params.get("code") ?? "");
    if (prefilled && normalizeUserCode(prefilled).length === 8) {
      void submit(prefilled);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function submit(value: string) {
    setStatus("submitting");
    setErrorMessage("");
    try {
      const resp = await deviceAuthService.approve(value);
      setApprovedFor({ email: resp.user_email, workspace: resp.workspace_slug ?? null });
      setStatus("approved");
    } catch (err: unknown) {
      const detail =
        typeof err === "object" && err !== null && "response" in err
          ? // @ts-expect-error axios shape
            (err.response?.data?.error ?? "")
          : "";
      setErrorMessage(detail || "Could not approve this code. Check it and try again.");
      setStatus("error");
    }
  }

  function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!submittable) return;
    void submit(code);
  }

  return (
    <div className="bg-custom-background-100 flex min-h-[80vh] items-center justify-center p-8">
      <div className="border-custom-border-200 bg-custom-background-90 shadow-sm w-full max-w-md rounded-lg border p-8">
        <h1 className="text-xl text-custom-text-100 mb-2 font-semibold">Authorize Pi Dash CLI</h1>
        <p className="text-sm text-custom-text-300 mb-6">
          Enter the code shown on your terminal to grant this device access to your account.
        </p>

        {status === "approved" && approvedFor ? (
          <div className="space-y-3">
            <div className="border-green-500/30 bg-green-500/10 text-sm text-green-700 dark:text-green-300 rounded-md border p-3">
              <strong>Approved.</strong> Your CLI should pick up the token within a few seconds.
            </div>
            <dl className="text-sm text-custom-text-200 space-y-1">
              <div className="flex justify-between">
                <dt className="text-custom-text-400">Account</dt>
                <dd>{approvedFor.email}</dd>
              </div>
              {approvedFor.workspace && (
                <div className="flex justify-between">
                  <dt className="text-custom-text-400">Workspace</dt>
                  <dd>{approvedFor.workspace}</dd>
                </div>
              )}
            </dl>
            <p className="text-xs text-custom-text-400 pt-2">You can close this tab.</p>
          </div>
        ) : (
          <form onSubmit={onSubmit} className="space-y-4">
            <label className="block">
              <span className="text-xs text-custom-text-400 mb-1 block font-medium tracking-wide uppercase">
                Device code
              </span>
              <input
                ref={(el) => {
                  // Auto-focus on mount without using the `autoFocus` prop,
                  // which oxlint's jsx-a11y/no-autofocus disallows.
                  el?.focus();
                }}
                type="text"
                inputMode="text"
                autoComplete="off"
                spellCheck={false}
                value={code}
                onChange={(e) => setCode(formatUserCode(e.target.value))}
                placeholder="XXXX-YYYY"
                className="border-custom-border-200 bg-custom-background-100 font-mono text-lg tracking-widest text-custom-text-100 focus:border-custom-primary-100 w-full rounded-md border px-3 py-2 uppercase focus:outline-none"
                maxLength={9}
              />
            </label>

            {status === "error" && errorMessage && (
              <div className="border-red-500/30 bg-red-500/10 text-sm text-red-600 dark:text-red-300 rounded-md border p-3">
                {errorMessage}
              </div>
            )}

            <button
              type="submit"
              disabled={!submittable}
              className="bg-custom-primary-100 text-sm hover:bg-custom-primary-200 w-full rounded-md px-4 py-2 font-medium text-white transition disabled:cursor-not-allowed disabled:opacity-50"
            >
              {status === "submitting" ? "Approving…" : "Approve"}
            </button>
          </form>
        )}

        <p className="text-xs text-custom-text-400 mt-6">
          Only approve a code that you started by running <code className="font-mono">pidash auth login</code> on a
          device you control.
        </p>
      </div>
    </div>
  );
}

export default function DeviceAuthRoute() {
  return (
    <AuthenticationWrapper pageType={EPageTypes.AUTHENTICATED}>
      <DeviceAuthPage />
    </AuthenticationWrapper>
  );
}
