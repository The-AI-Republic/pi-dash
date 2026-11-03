// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Public spaces auth card (parity/specs/space.md SPC-058-064). Email step,
// address check, then the code or password step, all driven by the flow
// machine in ./spaceAuthMachine.ts; failures map through the catalog in
// ./spaceAuthErrors.ts. Submits are native CSRF-protected form posts the
// browser follows; the instance capabilities, return address, prefill and
// coded failure arrive as props, so the landing gate (slice B) and the
// board's sign-in entries (slice D) mount this without importing
// workspace-shell auth state. The card never validates the return address
// — its mounters hand it a value that already survived the shared in-app
// path check — and never navigates to it client-side; the server redirects
// the native posts.
//
// Gate DOM contract (for the area gate's new-app driver; stable by test):
// - section[data-testid="space-auth-card"] with data-step (email|code|
//   password) and data-mode (sign-in|sign-up); its aria-label is the mode
//   ("Sign in" / "Sign up").
// - Banner: [data-testid="space-auth-banner"] (role alert) with a
//   "Dismiss" button; absent when no banner shows.
// - Email step: form[data-testid="space-auth-email-form"], "Email address"
//   field, "Clear email" button while non-empty, "Next" submit.
// - Code step: form[data-testid="space-auth-code-form"]
//   (method post, action /auth/spaces/magic-sign-in|up/), "Email address"
//   locked row with "Clear email", "Email code" field,
//   button[data-testid="space-auth-resend"] ("Send another code", while
//   cooling down "Send another code in Ns", while sending "Sending…"),
//   "Verify code" submit.
// - Password step: form[data-testid="space-auth-password-form"]
//   (method post, action /auth/spaces/sign-in|up/), locked row as above,
//   "Password" (sign-in) or "Choose a password" (sign-up) field plus
//   "Confirm your password" in sign-up, per-field "Show/Hide …" toggles,
//   meter [data-testid="space-auth-strength"] in sign-up while a
//   non-empty password fails the rules, "Use a login code instead" in
//   sign-in with mail on, "Sign in"/"Get started" submit.
// - Providers: [data-testid="space-auth-providers"] with one anchor per
//   provider (data-provider, full invitation label off the password step,
//   monogram plus invitation as the accessible name on it) and
//   [data-testid="space-auth-providers-divider"]; the whole block is
//   absent when the instance enables none.
// - Header [data-testid="space-auth-header"] (heading + subline follow
//   the mode) and consent [data-testid="space-auth-consent"] with
//   "Terms of Service" / "Privacy Policy" links in both modes.
import { Button, IconButton, Input, Tooltip } from "@pidash/kit";
import { Eye, EyeOff, X } from "lucide-react";
import * as React from "react";

import type { SpaceAuthProviderFlags } from "./spaceAuthProviders.js";
import { resolveSpaceAuthProviders } from "./spaceAuthProviders.js";
import type { SpaceAuthCapabilities, SpaceAuthMachineEvent, SpaceAuthMode } from "./spaceAuthMachine.js";
import { initialSpaceAuthState, routeAfterSpaceEmailCheck, spaceAuthReducer } from "./spaceAuthMachine.js";
import type { SpaceAuthServices } from "./spaceAuthServices.js";
import { createDefaultSpaceAuthServices, spaceAuthFailureCode, useSpaceCsrfToken } from "./spaceAuthServices.js";
import {
  SPACE_PASSWORD_MIN_LENGTH,
  isPlausibleSpaceEmail,
  isStrongSpacePassword,
  spacePasswordCriteria,
} from "./spaceAuthValidation.js";

/** Instance capabilities the card routes and renders on; plain data. */
export interface SpaceAuthInstanceCapabilities extends SpaceAuthCapabilities, SpaceAuthProviderFlags {}

export interface SpaceAuthCardProps {
  /** Instance flags; the card never reads signup enablement (server-side). */
  instance: SpaceAuthInstanceCapabilities;
  /** Return address the native posts and provider starts carry; opaque. */
  nextPath?: string | undefined;
  /** Prefill for the address field, from the landing address. */
  initialEmail?: string | undefined;
  /** Server error code from the landing address; consumed once on mount. */
  initialErrorCode?: string | number | null | undefined;
  /** Legal link targets; build-time defaults below. */
  termsHref?: string | undefined;
  privacyHref?: string | undefined;
  /** JSON calls; a public-safe default client when omitted. */
  services?: SpaceAuthServices | undefined;
}

const MAGIC_SIGN_IN_ACTION = "/auth/spaces/magic-sign-in/";
const MAGIC_SIGN_UP_ACTION = "/auth/spaces/magic-sign-up/";
const PASSWORD_SIGN_IN_ACTION = "/auth/spaces/sign-in/";
const PASSWORD_SIGN_UP_ACTION = "/auth/spaces/sign-up/";

const DEFAULT_TERMS_HREF = "https://airepublic.com/terms";
const DEFAULT_PRIVACY_HREF = "https://airepublic.com/privacy";

/** Client-side pause between code deliveries. */
export const SPACE_AUTH_RESEND_COOLDOWN_SECONDS = 5;

const EMAIL_LABEL = "Email address";
const EMAIL_INVALID_MESSAGE = "That email address doesn't look right.";
const EMAIL_SUBMIT_LABEL = "Next";
const CLEAR_EMAIL_LABEL = "Clear email";
const CODE_LABEL = "Email code";
const CODE_GUIDANCE = "Enter the code from the email we just sent.";
const CODE_SUBMIT_LABEL = "Verify code";
const RESEND_LABEL = "Send another code";
const RESEND_SENDING_LABEL = "Sending…";
const PASSWORD_LABEL_SIGN_IN = "Password";
const PASSWORD_LABEL_SIGN_UP = "Choose a password";
const CONFIRM_LABEL = "Confirm your password";
const MISMATCH_MESSAGE = "Confirmation doesn't match the password.";
const SHORT_MESSAGE = "Use 8 or more characters.";
const WEAK_MESSAGE = "Mix upper-case, lower-case, numbers and symbols.";
const PASSWORD_SUBMIT_SIGN_IN = "Sign in";
const PASSWORD_SUBMIT_SIGN_UP = "Get started";
const SWITCH_TO_CODE_LABEL = "Use a login code instead";
const DISMISS_BANNER_LABEL = "Dismiss";

const HEADINGS: Record<SpaceAuthMode, { heading: string; subline: string }> = {
  "sign-up": {
    heading: "Join the conversation",
    subline: "Create an account to vote and comment on public boards.",
  },
  "sign-in": {
    heading: "Welcome back",
    subline: "Sign in to vote and comment on public boards.",
  },
};

type Dispatch = React.Dispatch<SpaceAuthMachineEvent>;

function CardBanner({ message, onDismiss }: { message: string; onDismiss: () => void }): React.ReactElement {
  return (
    <div
      role="alert"
      data-testid="space-auth-banner"
      className="flex items-start gap-(--space-3) rounded-(--radius-control) border border-(--danger) bg-(--bg) p-(--space-4)"
    >
      <p className="text-body flex-1 text-(--text)">{message}</p>
      <Button variant="ghost" size="small" type="button" onClick={onDismiss} aria-label={DISMISS_BANNER_LABEL}>
        {DISMISS_BANNER_LABEL}
      </Button>
    </div>
  );
}

function CardHeader({ mode }: { mode: SpaceAuthMode }): React.ReactElement {
  const copy = HEADINGS[mode];
  return (
    <div data-testid="space-auth-header" className="flex flex-col gap-(--space-1)">
      <h1 className="text-h2 text-(--text)">{copy.heading}</h1>
      <p className="text-body text-(--text-muted)">{copy.subline}</p>
    </div>
  );
}

function ConsentLine({
  mode,
  termsHref,
  privacyHref,
}: {
  mode: SpaceAuthMode;
  termsHref: string;
  privacyHref: string;
}): React.ReactElement {
  return (
    <p data-testid="space-auth-consent" className="text-meta text-center text-(--text-muted)">
      {mode === "sign-up" ? "By creating an account, you agree to our " : "By signing in, you agree to our "}
      <a href={termsHref} target="_blank" rel="noopener noreferrer" className="font-medium underline">
        Terms of Service
      </a>
      {" and "}
      <a href={privacyHref} target="_blank" rel="noopener noreferrer" className="font-medium underline">
        Privacy Policy
      </a>
      {"."}
    </p>
  );
}

function LockedEmail({ email, onClear }: { email: string; onClear: () => void }): React.ReactElement {
  return (
    <Input
      label={EMAIL_LABEL}
      type="email"
      name="email"
      value={email}
      disabled
      {...(email === ""
        ? {}
        : {
            trailing: (
              <IconButton label={CLEAR_EMAIL_LABEL} size="small" onClick={onClear}>
                <X size={14} aria-hidden="true" />
              </IconButton>
            ),
          })}
    />
  );
}

function ProviderButtons({
  flags,
  mode,
  nextPath,
  compact,
}: {
  flags: SpaceAuthProviderFlags;
  mode: SpaceAuthMode;
  nextPath: string | undefined;
  compact: boolean;
}): React.ReactElement | null {
  const providers = resolveSpaceAuthProviders(flags, mode, nextPath);
  if (providers.length === 0) return null;
  return (
    <div data-testid="space-auth-providers" className="flex flex-col gap-(--space-3)">
      <div className={compact ? "flex flex-row gap-(--space-2)" : "flex flex-col gap-(--space-2)"}>
        {providers.map((provider) =>
          compact ? (
            <Tooltip key={provider.id} content={provider.name}>
              <a
                href={provider.href}
                aria-label={provider.label}
                data-provider={provider.id}
                className="text-emphasis flex size-(--control-height) items-center justify-center rounded-(--radius-control) border border-(--border) font-semibold text-(--text) hover:bg-(--subtle)"
              >
                {provider.monogram}
              </a>
            </Tooltip>
          ) : (
            <a
              key={provider.id}
              href={provider.href}
              data-provider={provider.id}
              className="text-body flex h-(--control-height) items-center justify-center rounded-(--radius-control) border border-(--border) font-medium text-(--text) hover:bg-(--subtle)"
            >
              {provider.label}
            </a>
          )
        )}
      </div>
      <div
        aria-hidden="true"
        data-testid="space-auth-providers-divider"
        className="text-meta flex items-center gap-(--space-3) text-(--text-muted)"
      >
        <span aria-hidden="true" className="h-px flex-1 bg-(--border)" />
        or
        <span aria-hidden="true" className="h-px flex-1 bg-(--border)" />
      </div>
    </div>
  );
}

interface EmailStepProps {
  initialValue: string;
  services: SpaceAuthServices;
  capabilities: SpaceAuthCapabilities;
  dispatch: Dispatch;
  runGenerate: (address: string) => Promise<boolean>;
}

function EmailStep({
  initialValue,
  services,
  capabilities,
  dispatch,
  runGenerate,
}: EmailStepProps): React.ReactElement {
  const [value, setValue] = React.useState(initialValue);
  const [focused, setFocused] = React.useState(true);
  const [checking, setChecking] = React.useState(false);
  const plausible = isPlausibleSpaceEmail(value);
  const showError = !focused && value.trim() !== "" && !plausible;

  const submit = async (event: React.FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();
    if (!isPlausibleSpaceEmail(value)) return;
    const address = value.trim().toLowerCase();
    setChecking(true);
    try {
      const answer = await services.checkEmail(address);
      const route = routeAfterSpaceEmailCheck(answer, capabilities);
      dispatch({ type: "checkResolved", email: address, answer, capabilities });
      if (route.step === "code") await runGenerate(address);
    } catch (error) {
      dispatch({ type: "requestFailed", code: spaceAuthFailureCode(error) });
    } finally {
      setChecking(false);
    }
  };

  return (
    <form onSubmit={submit} data-testid="space-auth-email-form" className="flex flex-col gap-(--space-4)">
      <Input
        label={EMAIL_LABEL}
        type="email"
        name="email"
        autoComplete="email"
        autoFocus
        required
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        {...(showError ? { error: EMAIL_INVALID_MESSAGE } : {})}
        {...(value === ""
          ? {}
          : {
              trailing: (
                <IconButton label={CLEAR_EMAIL_LABEL} size="small" tabIndex={-1} onClick={() => setValue("")}>
                  <X size={14} aria-hidden="true" />
                </IconButton>
              ),
            })}
      />
      <Button
        type="submit"
        variant="primary"
        size="large"
        className="w-full"
        loading={checking}
        disabled={!plausible || checking}
      >
        {EMAIL_SUBMIT_LABEL}
      </Button>
    </form>
  );
}

interface CodeStepProps {
  email: string;
  mode: SpaceAuthMode;
  nextPath: string | undefined;
  csrfToken: string | null;
  generating: boolean;
  runGenerate: (address: string) => Promise<boolean>;
  onClearEmail: () => void;
}

function CodeStep({
  email,
  mode,
  nextPath,
  csrfToken,
  generating,
  runGenerate,
  onClearEmail,
}: CodeStepProps): React.ReactElement {
  const [code, setCode] = React.useState("");
  const [cooldown, setCooldown] = React.useState(0);
  const [submitting, setSubmitting] = React.useState(false);

  React.useEffect(() => {
    if (cooldown <= 0) return;
    const timer = setInterval(() => setCooldown((left) => left - 1), 1000);
    return () => clearInterval(timer);
  }, [cooldown]);

  const resend = async (): Promise<void> => {
    const sent = await runGenerate(email);
    if (sent) setCooldown(SPACE_AUTH_RESEND_COOLDOWN_SECONDS);
  };

  const resendLabel =
    cooldown > 0 ? `${RESEND_LABEL} in ${cooldown}s` : generating ? RESEND_SENDING_LABEL : RESEND_LABEL;
  const ready = csrfToken !== null && !generating && code.trim() !== "" && !submitting;

  return (
    <form
      method="post"
      action={mode === "sign-in" ? MAGIC_SIGN_IN_ACTION : MAGIC_SIGN_UP_ACTION}
      onSubmit={() => setSubmitting(true)}
      data-testid="space-auth-code-form"
      className="flex flex-col gap-(--space-4)"
    >
      <input type="hidden" name="csrfmiddlewaretoken" value={csrfToken ?? ""} readOnly />
      <input type="hidden" name="email" value={email} readOnly />
      {nextPath === undefined || nextPath === "" ? null : (
        <input type="hidden" name="next_path" value={nextPath} readOnly />
      )}
      <LockedEmail email={email} onClear={onClearEmail} />
      <Input
        label={CODE_LABEL}
        name="code"
        autoComplete="one-time-code"
        autoFocus
        required
        value={code}
        onChange={(event) => setCode(event.target.value)}
        hint={CODE_GUIDANCE}
      />
      <div className="flex justify-end">
        <Button
          type="button"
          variant="ghost"
          size="small"
          onClick={resend}
          disabled={generating || cooldown > 0}
          data-testid="space-auth-resend"
        >
          {resendLabel}
        </Button>
      </div>
      <Button type="submit" variant="primary" size="large" className="w-full" loading={submitting} disabled={!ready}>
        {CODE_SUBMIT_LABEL}
      </Button>
    </form>
  );
}

interface PasswordStepProps {
  email: string;
  mode: SpaceAuthMode;
  nextPath: string | undefined;
  csrfToken: string | null;
  mailOn: boolean;
  switching: boolean;
  onClearEmail: () => void;
  onSwitchToCode: () => void;
}

function PasswordStep({
  email,
  mode,
  nextPath,
  csrfToken,
  mailOn,
  switching,
  onClearEmail,
  onSwitchToCode,
}: PasswordStepProps): React.ReactElement {
  const [password, setPassword] = React.useState("");
  const [confirm, setConfirm] = React.useState("");
  const [showPassword, setShowPassword] = React.useState(false);
  const [showConfirm, setShowConfirm] = React.useState(false);
  const [confirmFocused, setConfirmFocused] = React.useState(false);
  const [submitting, setSubmitting] = React.useState(false);

  const signUp = mode === "sign-up";
  const strong = isStrongSpacePassword(password);
  const showMeter = signUp && password !== "" && !strong;
  const mismatch = confirm !== "" && confirm !== password && (!confirmFocused || confirm.length >= password.length);
  const ready = csrfToken !== null && !submitting && password !== "" && (!signUp || (strong && confirm === password));

  return (
    <form
      method="post"
      action={mode === "sign-in" ? PASSWORD_SIGN_IN_ACTION : PASSWORD_SIGN_UP_ACTION}
      onSubmit={() => setSubmitting(true)}
      data-testid="space-auth-password-form"
      className="flex flex-col gap-(--space-4)"
    >
      <input type="hidden" name="csrfmiddlewaretoken" value={csrfToken ?? ""} readOnly />
      <input type="hidden" name="email" value={email} readOnly />
      {nextPath === undefined || nextPath === "" ? null : (
        <input type="hidden" name="next_path" value={nextPath} readOnly />
      )}
      <LockedEmail email={email} onClear={onClearEmail} />
      <Input
        label={signUp ? PASSWORD_LABEL_SIGN_UP : PASSWORD_LABEL_SIGN_IN}
        type={showPassword ? "text" : "password"}
        name="password"
        autoComplete={signUp ? "new-password" : "current-password"}
        autoFocus
        required
        value={password}
        onChange={(event) => setPassword(event.target.value)}
        trailing={
          <IconButton
            label={showPassword ? "Hide password" : "Show password"}
            size="small"
            onClick={() => setShowPassword((shown) => !shown)}
          >
            {showPassword ? <EyeOff size={14} aria-hidden="true" /> : <Eye size={14} aria-hidden="true" />}
          </IconButton>
        }
      />
      {showMeter ? <StrengthMeter password={password} /> : null}
      {signUp ? (
        <Input
          label={CONFIRM_LABEL}
          type={showConfirm ? "text" : "password"}
          name="confirm_password"
          autoComplete="new-password"
          required
          value={confirm}
          onChange={(event) => setConfirm(event.target.value)}
          onFocus={() => setConfirmFocused(true)}
          onBlur={() => setConfirmFocused(false)}
          {...(mismatch ? { error: MISMATCH_MESSAGE } : {})}
          trailing={
            <IconButton
              label={showConfirm ? "Hide confirmation" : "Show confirmation"}
              size="small"
              onClick={() => setShowConfirm((shown) => !shown)}
            >
              {showConfirm ? <EyeOff size={14} aria-hidden="true" /> : <Eye size={14} aria-hidden="true" />}
            </IconButton>
          }
        />
      ) : null}
      <div className="flex flex-col gap-(--space-2)">
        <Button type="submit" variant="primary" size="large" className="w-full" loading={submitting} disabled={!ready}>
          {signUp ? PASSWORD_SUBMIT_SIGN_UP : PASSWORD_SUBMIT_SIGN_IN}
        </Button>
        {!signUp && mailOn ? (
          <Button
            type="button"
            variant="secondary"
            size="large"
            className="w-full"
            onClick={onSwitchToCode}
            disabled={switching}
          >
            {SWITCH_TO_CODE_LABEL}
          </Button>
        ) : null}
      </div>
    </form>
  );
}

function StrengthMeter({ password }: { password: string }): React.ReactElement {
  const unmet = spacePasswordCriteria(password).filter((criterion) => !criterion.met);
  return (
    <div data-testid="space-auth-strength" className="flex flex-col gap-(--space-2)">
      <p className="text-body font-medium text-(--text)">
        {password.length < SPACE_PASSWORD_MIN_LENGTH ? SHORT_MESSAGE : WEAK_MESSAGE}
      </p>
      <ul className="flex flex-col gap-(--space-1)">
        {unmet.map((criterion) => (
          <li key={criterion.id} className="text-meta text-(--text-muted)">
            {criterion.label}
          </li>
        ))}
      </ul>
    </div>
  );
}

export function SpaceAuthCard(props: SpaceAuthCardProps): React.ReactElement {
  const {
    instance,
    nextPath,
    initialEmail,
    initialErrorCode,
    termsHref = DEFAULT_TERMS_HREF,
    privacyHref = DEFAULT_PRIVACY_HREF,
    services: injectedServices,
  } = props;
  const services = React.useMemo(() => injectedServices ?? createDefaultSpaceAuthServices(), [injectedServices]);
  const [machine, dispatch] = React.useReducer(
    spaceAuthReducer,
    { email: initialEmail ?? "", errorCode: initialErrorCode ?? null },
    initialSpaceAuthState
  );
  const [generating, setGenerating] = React.useState(false);
  const csrf = useSpaceCsrfToken(services);

  React.useEffect(() => {
    if (csrf.failed) dispatch({ type: "requestFailed", code: "network" });
  }, [csrf.failed, dispatch]);

  const runGenerate = React.useCallback(
    async (address: string): Promise<boolean> => {
      setGenerating(true);
      try {
        await services.generateCode(address);
        return true;
      } catch (error) {
        dispatch({ type: "requestFailed", code: spaceAuthFailureCode(error) });
      } finally {
        setGenerating(false);
      }
      return false;
    },
    [services]
  );

  const switchToCode = React.useCallback(() => {
    dispatch({ type: "switchToCode" });
    void runGenerate(machine.email);
  }, [machine.email, runGenerate]);

  const clearEmail = React.useCallback(() => {
    dispatch({ type: "clearEmail" });
  }, []);

  return (
    <section
      aria-label={machine.mode === "sign-up" ? "Sign up" : "Sign in"}
      data-testid="space-auth-card"
      data-step={machine.step}
      data-mode={machine.mode}
      className="flex w-full flex-col gap-(--space-6)"
    >
      {machine.banner === null ? null : (
        <CardBanner message={machine.banner.message} onDismiss={() => dispatch({ type: "dismissBanner" })} />
      )}
      <CardHeader mode={machine.mode} />
      <ProviderButtons flags={instance} mode={machine.mode} nextPath={nextPath} compact={machine.step === "password"} />
      {machine.step === "email" ? (
        <EmailStep
          initialValue={machine.email}
          services={services}
          capabilities={instance}
          dispatch={dispatch}
          runGenerate={runGenerate}
        />
      ) : machine.step === "code" ? (
        <CodeStep
          email={machine.email}
          mode={machine.mode}
          nextPath={nextPath}
          csrfToken={csrf.token}
          generating={generating}
          runGenerate={runGenerate}
          onClearEmail={clearEmail}
        />
      ) : (
        <PasswordStep
          email={machine.email}
          mode={machine.mode}
          nextPath={nextPath}
          csrfToken={csrf.token}
          mailOn={instance.smtpConfigured}
          switching={generating}
          onClearEmail={clearEmail}
          onSwitchToCode={switchToCode}
        />
      )}
      <ConsentLine mode={machine.mode} termsHref={termsHref} privacyHref={privacyHref} />
    </section>
  );
}
