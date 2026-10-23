// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// First-run setup form (ADM-004-006, from parity/specs/admin.md
// §1.4-1.6): the first user becomes the instance admin through a
// native full-page POST carrying the hidden forgery token. Prefill
// and server errors arrive as redirect query params; numeric codes
// place like their named counterparts and every echoed field —
// company included — prefills (NEWFRONT-457 intent).
//
// Both show/hide toggles swallow mousedown so pressing one never moves
// focus: the strength meter mounts and unmounts with the password
// field's focus, so a focus change on mousedown would reflow the form
// between press and release and the release would land off the button,
// silently dropping the toggle (ADM-005).

import { Button, Input } from "@pidash/kit";
import { Eye, EyeOff } from "lucide-react";
import * as React from "react";

import { secretCriteria } from "../../auth/index.js";
import { useDocumentHead } from "../../../shared/shell/index.js";
import { resolveSetupError } from "../adminErrors.js";
import { useAdminCsrfToken } from "../hooks.js";
import {
  acceptCompanyName,
  acceptPersonName,
  COMPANY_NAME_MAX_LENGTH,
  isMismatchVisible,
  isSetupSubmittable,
  isStrengthMeterVisible,
  PERSON_NAME_MAX_LENGTH,
} from "../setupValidation.js";
import type { AdminEntrySearch } from "./AdminEntryGate.js";

export const SETUP_POST_PATH = "/api/instances/admins/sign-up/";
export const SETUP_TITLE = "Create the administrator account — Pi Dash";
const TELEMETRY_DOC_URL = "https://github.com/The-AI-Republic/pi-dash#readme";
const WEAK_SECRET_MESSAGE = "Use 8 or more characters with upper and lower case, a number, and a symbol.";
const STRONG_SECRET_MESSAGE = "Strong password.";

export interface SetupPrefill {
  firstName: string;
  lastName: string;
  email: string;
  company: string;
  telemetryChecked: boolean;
}

/**
 * Prefill from the redirect params, verbatim (no validation: even an
 * over-long crafted echo shows, matching the prefill probes). The
 * company prefers the server's echo spelling and falls back to the
 * crafted one; telemetry stays on unless explicitly disabled.
 */
export function resolveSetupPrefill(search: AdminEntrySearch): SetupPrefill {
  return {
    firstName: search.first_name ?? "",
    lastName: search.last_name ?? "",
    email: search.email ?? "",
    company: search.company_name ?? search.company ?? "",
    telemetryChecked: search.is_telemetry_enabled !== "False",
  };
}

function StrengthMeter({ password, focused }: { password: string; focused: boolean }): React.ReactElement | null {
  if (!isStrengthMeterVisible(password, focused)) return null;
  const criteria = secretCriteria(password);
  const met = criteria.filter((criterion) => criterion.met).length;
  return (
    <div data-testid="admin-setup-strength" className="flex flex-col gap-(--space-1)">
      <div aria-hidden="true" className="flex w-full gap-(--space-1)">
        {criteria.map((criterion) => (
          <span
            key={criterion.id}
            data-met={criterion.met}
            className={
              criterion.met ? "h-1 flex-1 rounded-full bg-(--success)" : "h-1 flex-1 rounded-full bg-(--border)"
            }
          />
        ))}
      </div>
      {password.length === 0 ? null : (
        <p data-testid="admin-setup-strength-message" className="text-meta text-(--text-muted)">
          {met === criteria.length ? STRONG_SECRET_MESSAGE : WEAK_SECRET_MESSAGE}
        </p>
      )}
    </div>
  );
}

export function AdminSetupForm({ search }: { search: AdminEntrySearch }): React.ReactElement {
  useDocumentHead({ title: SETUP_TITLE, robots: "noindex, nofollow" });
  const prefill = React.useMemo(() => resolveSetupPrefill(search), [search]);
  const [firstName, setFirstName] = React.useState(prefill.firstName);
  const [lastName, setLastName] = React.useState(prefill.lastName);
  const [email, setEmail] = React.useState(prefill.email);
  const [company, setCompany] = React.useState(prefill.company);
  const [password, setPassword] = React.useState("");
  const [confirmPassword, setConfirmPassword] = React.useState("");
  const [telemetry, setTelemetry] = React.useState(prefill.telemetryChecked);
  const [showPassword, setShowPassword] = React.useState(false);
  const [showConfirm, setShowConfirm] = React.useState(false);
  const [passwordFocused, setPasswordFocused] = React.useState(false);
  const [confirmFocused, setConfirmFocused] = React.useState(false);
  const [submitting, setSubmitting] = React.useState(false);

  const csrf = useAdminCsrfToken();
  const token = csrf.data?.csrf_token;
  const error = React.useMemo(
    () => resolveSetupError(search.error_code, search.error_message),
    [search.error_code, search.error_message]
  );
  const submittable = isSetupSubmittable({
    firstName,
    email,
    password,
    confirmPassword,
    csrfReady: token !== undefined,
  });
  const mismatch = isMismatchVisible({ password, confirmPassword, confirmFocused });

  return (
    <main className="flex min-h-screen items-center justify-center bg-(--bg) p-(--space-8)">
      <div className="flex w-full max-w-105 flex-col gap-(--space-6)">
        <div className="flex flex-col gap-(--space-1)">
          <h1 data-testid="admin-setup-heading" className="text-h2 text-(--text)">
            Create the administrator account
          </h1>
          <p className="text-body text-(--text-muted)">
            The first account on a new instance becomes its administrator.
          </p>
        </div>
        {error.placement === "banner" ? (
          <div
            role="alert"
            data-testid="admin-setup-banner"
            className="rounded-(--radius-control) border border-(--danger) p-(--space-4)"
          >
            <p className="text-body text-(--text)">{error.text}</p>
          </div>
        ) : null}
        <form
          method="post"
          action={SETUP_POST_PATH}
          data-testid="admin-setup-form"
          className="flex flex-col gap-(--space-4)"
          onSubmit={() => setSubmitting(true)}
        >
          <input type="hidden" name="csrfmiddlewaretoken" value={token ?? ""} readOnly />
          <input type="hidden" name="is_telemetry_enabled" value={telemetry ? "True" : "False"} readOnly />
          <Input
            label="First name"
            id="first_name"
            name="first_name"
            type="text"
            autoComplete="off"
            autoFocus
            required
            maxLength={PERSON_NAME_MAX_LENGTH}
            value={firstName}
            onChange={(event) => {
              if (acceptPersonName(event.target.value)) setFirstName(event.target.value);
            }}
          />
          <Input
            label="Last name"
            id="last_name"
            name="last_name"
            type="text"
            autoComplete="off"
            maxLength={PERSON_NAME_MAX_LENGTH}
            value={lastName}
            onChange={(event) => {
              if (acceptPersonName(event.target.value)) setLastName(event.target.value);
            }}
          />
          <Input
            label="Email"
            id="email"
            name="email"
            type="email"
            autoComplete="email"
            required
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            {...(error.placement === "inline-email" ? { error: error.text } : {})}
          />
          <Input
            label="Company"
            id="company_name"
            name="company_name"
            type="text"
            autoComplete="off"
            maxLength={COMPANY_NAME_MAX_LENGTH}
            value={company}
            onChange={(event) => {
              if (acceptCompanyName(event.target.value)) setCompany(event.target.value);
            }}
          />
          <Input
            label="Password"
            id="password"
            name="password"
            type={showPassword ? "text" : "password"}
            autoComplete="new-password"
            required
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            onFocus={() => setPasswordFocused(true)}
            onBlur={() => setPasswordFocused(false)}
            trailing={
              <button
                type="button"
                tabIndex={-1}
                data-testid="admin-setup-toggle-password"
                aria-label={showPassword ? "Hide password" : "Show password"}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => setShowPassword((current) => !current)}
                className="text-body cursor-pointer text-(--text-muted) hover:text-(--text)"
              >
                {showPassword ? <EyeOff size={16} aria-hidden="true" /> : <Eye size={16} aria-hidden="true" />}
              </button>
            }
            {...(error.placement === "inline-password" ? { error: error.text } : {})}
          />
          <StrengthMeter password={password} focused={passwordFocused} />
          <Input
            label="Confirm password"
            id="confirm_password"
            name="confirm_password"
            type={showConfirm ? "text" : "password"}
            autoComplete="new-password"
            required
            value={confirmPassword}
            onChange={(event) => setConfirmPassword(event.target.value)}
            onFocus={() => setConfirmFocused(true)}
            onBlur={() => setConfirmFocused(false)}
            trailing={
              <button
                type="button"
                tabIndex={-1}
                data-testid="admin-setup-toggle-confirm"
                aria-label={showConfirm ? "Hide password confirmation" : "Show password confirmation"}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => setShowConfirm((current) => !current)}
                className="text-body cursor-pointer text-(--text-muted) hover:text-(--text)"
              >
                {showConfirm ? <EyeOff size={16} aria-hidden="true" /> : <Eye size={16} aria-hidden="true" />}
              </button>
            }
          />
          {mismatch ? (
            <p data-testid="admin-setup-mismatch" role="alert" className="text-meta text-(--danger)">
              The passwords do not match.
            </p>
          ) : null}
          <div className="flex items-start gap-(--space-2)">
            <input
              type="checkbox"
              id="is_telemetry_enabled"
              checked={telemetry}
              onChange={() => setTelemetry((current) => !current)}
              className="mt-1 size-4 shrink-0 accent-(--accent)"
            />
            <label htmlFor="is_telemetry_enabled" className="text-body cursor-pointer text-(--text-muted)">
              Share anonymous usage data to help improve Pi Dash.{" "}
              <a
                href={TELEMETRY_DOC_URL}
                target="_blank"
                rel="noreferrer"
                className="font-medium text-(--accent) hover:underline"
              >
                Learn more
              </a>
            </label>
          </div>
          <Button
            type="submit"
            variant="primary"
            loading={submitting}
            disabled={!submittable || submitting}
            data-testid="admin-setup-submit"
            className="w-full"
          >
            Continue
          </Button>
        </form>
      </div>
    </main>
  );
}
