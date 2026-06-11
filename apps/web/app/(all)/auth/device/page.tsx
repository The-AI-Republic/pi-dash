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
    <div className="flex min-h-[80vh] items-center justify-center bg-canvas p-8">
      <div className="w-full max-w-md rounded-lg border border-subtle bg-surface-1 p-8 shadow-raised-200">
        <h1 className="text-xl mb-2 font-semibold text-primary">Authorize Pi Dash CLI</h1>
        <p className="text-sm mb-6 text-tertiary">
          Enter the code shown on your terminal to grant this device access to your account.
        </p>

        {status === "approved" && approvedFor ? (
          <div className="space-y-3">
            <div className="text-sm rounded-md border border-success-subtle bg-success-subtle p-3 text-success-primary">
              <strong>Approved.</strong> Your CLI should pick up the token within a few seconds.
            </div>
            <dl className="text-sm space-y-1 text-secondary">
              <div className="flex justify-between">
                <dt className="text-placeholder">Account</dt>
                <dd>{approvedFor.email}</dd>
              </div>
              {approvedFor.workspace && (
                <div className="flex justify-between">
                  <dt className="text-placeholder">Workspace</dt>
                  <dd>{approvedFor.workspace}</dd>
                </div>
              )}
            </dl>
            <p className="text-xs pt-2 text-placeholder">You can close this tab.</p>
          </div>
        ) : (
          <form onSubmit={onSubmit} className="space-y-4">
            <label className="block">
              <span className="text-xs mb-1 block font-medium tracking-wide text-placeholder uppercase">
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
                className="font-mono text-lg tracking-widest w-full rounded-md border border-subtle bg-canvas px-3 py-2 text-primary uppercase focus:border-accent-strong focus:outline-none"
                maxLength={9}
              />
            </label>

            {status === "error" && errorMessage && (
              <div className="text-sm rounded-md border border-danger-subtle bg-danger-subtle p-3 text-danger-primary">
                {errorMessage}
              </div>
            )}

            <button
              type="submit"
              disabled={!submittable}
              className="text-sm w-full rounded-md bg-accent-primary px-4 py-2 font-medium text-on-color transition hover:bg-accent-primary-hover disabled:cursor-not-allowed disabled:opacity-50"
            >
              {status === "submitting" ? "Approving…" : "Approve"}
            </button>
          </form>
        )}

        <p className="text-xs mt-6 text-placeholder">
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
