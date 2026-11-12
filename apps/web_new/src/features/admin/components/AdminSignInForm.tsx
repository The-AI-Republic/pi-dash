// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Admin sign-in form (ADM-007-008, from parity/specs/admin.md §1.7-1.8):
// a native full-page POST carrying the hidden forgery token. Failures
// redirect back with codes that render as a generic banner (named
// codes, server message verbatim) or a mapped dismissible info banner
// (numeric codes, client copy); the mapped link returns to the entry
// (NEWFRONT-458 intent).

import { Button, Input } from "@pidash/kit";
import { Eye, EyeOff } from "lucide-react";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";
import { resolveSignInBanners } from "../adminErrors.js";
import { useAdminCsrfToken } from "../hooks.js";
import type { AdminEntrySearch } from "./AdminEntryGate.js";

export const SIGN_IN_POST_PATH = "/api/instances/admins/sign-in/";
export const SIGN_IN_TITLE = "Sign in — Pi Dash instance admin";

export function AdminSignInForm({ search }: { search: AdminEntrySearch }): React.ReactElement {
  useDocumentHead({ title: SIGN_IN_TITLE, robots: "noindex, nofollow" });
  const [email, setEmail] = React.useState(search.email ?? "");
  const [password, setPassword] = React.useState("");
  const [showPassword, setShowPassword] = React.useState(false);
  const [submitting, setSubmitting] = React.useState(false);
  const [noticeDismissed, setNoticeDismissed] = React.useState(false);

  const csrf = useAdminCsrfToken();
  const token = csrf.data?.csrf_token;
  const banners = React.useMemo(
    () => resolveSignInBanners(search.error_code, search.error_message),
    [search.error_code, search.error_message]
  );
  const mapped = noticeDismissed ? null : banners.mapped;
  const submittable = token !== undefined && email.length > 0 && password.length > 0;

  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
      <div className="flex w-full max-w-90 flex-col gap-(--space-6)">
        <div className="flex flex-col gap-(--space-1)">
          <h1 data-testid="admin-sign-in-heading" className="text-h2 text-(--text)">
            Sign in to instance admin
          </h1>
          <p className="text-body text-(--text-muted)">Manage this Pi Dash instance and its settings.</p>
        </div>
        {banners.generic !== null ? (
          <div
            role="alert"
            data-testid="admin-sign-in-error"
            className="rounded-(--radius-control) border border-(--danger) p-(--space-4)"
          >
            <p className="text-body text-(--text)">{banners.generic}</p>
          </div>
        ) : mapped !== null ? (
          <div
            role="status"
            data-testid="admin-sign-in-notice"
            className="rounded-(--radius-control) border border-(--border) bg-(--subtle) p-(--space-4)"
          >
            <p className="text-body text-(--text)">
              <strong className="font-medium">{mapped.title}.</strong> {mapped.message}{" "}
              {mapped.linkHref !== null ? (
                <a href={mapped.linkHref} className="font-medium text-(--accent) hover:underline">
                  Return to sign-in
                </a>
              ) : null}
            </p>
            <div className="mt-(--space-2) flex justify-end">
              <Button variant="ghost" size="small" onClick={() => setNoticeDismissed(true)} aria-label="Dismiss notice">
                Dismiss
              </Button>
            </div>
          </div>
        ) : null}
        <form
          method="post"
          action={SIGN_IN_POST_PATH}
          data-testid="admin-sign-in-form"
          className="flex flex-col gap-(--space-4)"
          onSubmit={() => setSubmitting(true)}
        >
          <input type="hidden" name="csrfmiddlewaretoken" value={token ?? ""} readOnly />
          <Input
            label="Email"
            id="email"
            name="email"
            type="email"
            autoComplete="email"
            autoFocus
            required
            value={email}
            onChange={(event) => setEmail(event.target.value)}
          />
          <Input
            label="Password"
            id="password"
            name="password"
            type={showPassword ? "text" : "password"}
            autoComplete="current-password"
            required
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            trailing={
              <button
                type="button"
                tabIndex={-1}
                data-testid="admin-sign-in-toggle-password"
                aria-label={showPassword ? "Hide password" : "Show password"}
                onClick={() => setShowPassword((current) => !current)}
                className="text-body cursor-pointer text-(--text-muted) hover:text-(--text)"
              >
                {showPassword ? <EyeOff size={16} aria-hidden="true" /> : <Eye size={16} aria-hidden="true" />}
              </button>
            }
          />
          <Button
            type="submit"
            variant="primary"
            loading={submitting}
            disabled={!submittable || submitting}
            data-testid="admin-sign-in-submit"
            className="w-full"
          >
            Sign in
          </Button>
        </form>
      </div>
    </main>
  );
}
