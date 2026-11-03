// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Auth card. Email first: the server decides whether the address signs
// in with a password or an emailed code, and whether the card stays in
// its home mode or flips into the other one. Provider buttons resolve
// through the edition intersected with the instance flags; failures
// render the server code as a banner and land on the step that can fix
// them. Written from parity/specs/auth.md AUTH-001-011; the sign-up card
// is this same machine with the sign-up home (see SignUpCard), and later
// slices mount recovery pages and guards on its step, error, and
// return-path conventions.

import { Button, IconButton, Input, Popover, Spinner } from "@pidash/kit";
import type { SignInResult } from "@pidash/api-client";
import { X } from "lucide-react";
import * as React from "react";

import { getAppClient } from "../../../core/api/client.js";
import { edition } from "../../../core/edition/index.js";
import { platform } from "../../../core/platform/index.js";
import { useInstanceConfig } from "../../../core/session/hooks.js";
import {
  useEmailCheck,
  useMagicGenerate,
  useMagicSignIn,
  useMagicSignUp,
  usePasswordSignIn,
  usePasswordSignUp,
} from "../api.js";
import {
  describeAuthError,
  describeSubmitFailure,
  type AuthErrorAction,
  type AuthMode,
  type SignInStep,
} from "../authErrors.js";
import { isStrongSecret } from "../passwordStrength.js";
import { currentPageTarget, startCloudHandoff } from "../cloudLogin.js";
import { resolveSignInProviders, type SignInProvider } from "../providers.js";
import { useWorkspaceInvitation } from "../invitations.js";
import { PasswordStrengthHint } from "./PasswordStrengthHint.js";

export interface SignInCardProps {
  /** Where the server sends the user after sign-in (as next_path). */
  next?: string | undefined;
  /**
   * Raw return path from the URL (AUTH-016 absolute leg). Forwarded to the
   * auth POSTs for the server to sanitize; never navigated to client-side.
   * Present only when no safe `next` resolved.
   */
  unsafeNext?: string | undefined;
  initialEmail?: string | undefined;
  /** A server error_code carried in the URL after a native-form failure. */
  initialErrorCode?: string | null | undefined;
  /** Invitation link params for the invitation-aware header. */
  invitationId?: string | undefined;
  invitationSlug?: string | undefined;
  /** The card's home mode and URL; clearing the email resets to it. */
  homeMode?: AuthMode | undefined;
}

const EMAIL_PATTERN = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/** Client-side email validity for the continue gate and the invalid hint. */
export function isValidEmail(value: string): boolean {
  return EMAIL_PATTERN.test(value.trim());
}

const RESEND_COOLDOWN_S = 5;
const SIGN_IN_HOME = "/";
const SIGN_UP_HOME = "/sign-up";
const NETWORK_MESSAGE = "Could not reach the server. Check your connection and try again.";
const CODE_SEND_MESSAGE = "Could not send a code. Try again in a moment.";
const WEAK_SECRET_MESSAGE = "Choose a strong password to create your account.";
const MISMATCH_MESSAGE = "The two passwords don't match.";

export interface AuthBannerProps {
  message: string;
  action?: AuthErrorAction | null | undefined;
  /** Absent on banners that replace their form (nothing to dismiss back to). */
  onDismiss?: (() => void) | undefined;
}

/** Shared auth failure banner. Recovery pages reuse it for their token banners (AUTH-013). */
export function AuthBanner({ message, action = null, onDismiss }: AuthBannerProps): React.ReactElement {
  return (
    <div
      role="alert"
      className="flex items-start gap-(--space-2) rounded-(--radius-control) border border-(--danger) p-(--space-4)"
    >
      <p className="text-body flex-1 text-(--text)">
        {message}{" "}
        {action ? (
          <a href={action.href} className="font-medium underline">
            {action.label}
          </a>
        ) : null}
      </p>
      {onDismiss ? (
        <Button variant="ghost" size="small" onClick={onDismiss} aria-label="Dismiss error">
          Dismiss
        </Button>
      ) : null}
    </div>
  );
}

function LockedEmail({ email, onClear }: { email: string; onClear: () => void }): React.ReactElement {
  return (
    <div className="flex items-center justify-between gap-(--space-2)">
      <p className="text-body truncate text-(--text-muted)">{email}</p>
      <IconButton label="Clear email" size="small" onClick={onClear}>
        <X size={14} aria-hidden="true" />
      </IconButton>
    </div>
  );
}

export function SignInCard({
  next,
  unsafeNext,
  initialEmail = "",
  initialErrorCode,
  invitationId,
  invitationSlug,
  homeMode = "sign-in",
}: SignInCardProps): React.ReactElement {
  const trimmedInitialEmail = initialEmail.trim();
  const initialError = React.useMemo(
    () => describeAuthError(initialErrorCode, homeMode, trimmedInitialEmail),
    [initialErrorCode, homeMode, trimmedInitialEmail]
  );
  const [email, setEmail] = React.useState(trimmedInitialEmail);
  const [emailTouched, setEmailTouched] = React.useState(false);
  const [step, setStep] = React.useState<SignInStep>(initialError?.step ?? "email");
  const [mode, setMode] = React.useState<AuthMode>(initialError?.mode ?? homeMode);
  const [banner, setBanner] = React.useState<{ message: string; action: AuthErrorAction | null } | null>(
    initialError ? { message: initialError.message, action: initialError.action ?? null } : null
  );
  const [password, setPassword] = React.useState("");
  const [confirm, setConfirm] = React.useState("");
  const [confirmFocused, setConfirmFocused] = React.useState(false);
  const [showPassword, setShowPassword] = React.useState(false);
  const [code, setCode] = React.useState("");
  const [lastSentAt, setLastSentAt] = React.useState<number | null>(null);
  const [now, setNow] = React.useState(() => Date.now());

  const instanceConfig = useInstanceConfig();
  const invitation = useWorkspaceInvitation(invitationSlug, invitationId);
  const emailCheck = useEmailCheck();
  const magicGenerate = useMagicGenerate();
  const passwordSignIn = usePasswordSignIn();
  const magicSignIn = useMagicSignIn();
  const passwordSignUp = usePasswordSignUp();
  const magicSignUp = useMagicSignUp();

  React.useEffect(() => {
    if (lastSentAt === null) return;
    const timer = globalThis.setInterval(() => setNow(Date.now()), 1000);
    return () => globalThis.clearInterval(timer);
  }, [lastSentAt]);

  const signingUpHome = homeMode === "sign-up";
  const cardLabel = signingUpHome ? "Sign up" : "Sign in";
  const loadedConfig = instanceConfig.data;
  if (loadedConfig === undefined) {
    if (instanceConfig.isError) {
      return (
        <section aria-label={cardLabel} className="flex w-full max-w-90 flex-col gap-(--space-6)">
          <div className="flex flex-col gap-(--space-1)">
            <h1 className="text-h2 text-(--text)">{signingUpHome ? "Create your account" : "Sign in to Pi Dash"}</h1>
            <p className="text-body text-(--text-muted)">
              {signingUpHome ? "Could not load sign-up." : "Could not load sign-in."} Check your connection and try
              again.
            </p>
          </div>
          <Button
            variant="secondary"
            onClick={() => {
              void instanceConfig.refetch();
            }}
          >
            Try again
          </Button>
        </section>
      );
    }
    return (
      <section aria-label={cardLabel} className="flex w-full max-w-90 flex-col items-center gap-(--space-6)">
        <Spinner label={signingUpHome ? "Loading sign-up options" : "Loading sign-in options"} />
      </section>
    );
  }

  const smtpConfigured = loadedConfig.is_smtp_configured;
  const emailMethods = loadedConfig.is_email_password_enabled || loadedConfig.is_magic_login_enabled;
  const postNext = unsafeNext ?? next;
  const nextPathInput = postNext === undefined ? {} : { nextPath: postNext };
  const providers = resolveSignInProviders(edition.auth, loadedConfig, {
    mode,
    apiOrigin: getAppClient().baseUrl,
    returnToPage: currentPageTarget(globalThis.window.location),
    browserOrigin: globalThis.window.location.origin,
    ...nextPathInput,
  });

  function startProviderSignIn(provider: SignInProvider): void {
    // The organizational provider starts the desktop-aware cloud handoff
    // (CLOUD-017); native providers navigate their auth URL directly.
    if (provider.cloudHandoff === undefined) {
      globalThis.window.location.assign(provider.href);
      return;
    }
    void startCloudHandoff({
      shell: platform.shellSignIn,
      apiBase: getAppClient().baseUrl,
      browserOrigin: globalThis.window.location.origin,
      returnTo: provider.cloudHandoff.returnTo,
      navigate: (url) => globalThis.window.location.assign(url),
    });
  }
  const noMethods = providers.length === 0 && !emailMethods;
  const providerCompact = step === "password";

  const invitationData = invitation.data;
  const inviteHeader =
    invitationData !== undefined && invitationData.email === email.trim() && invitationData.workspace !== null
      ? invitationData
      : null;

  const resendIn = lastSentAt === null ? 0 : Math.max(0, RESEND_COOLDOWN_S - Math.floor((now - lastSentAt) / 1000));
  const showInvalid = emailTouched && email.length > 0 && !isValidEmail(email);
  const signingUp = mode === "sign-up";
  const secretMismatch = confirm.length > 0 && confirm !== password;
  const showMismatch = signingUp && secretMismatch && (!confirmFocused || confirm.length >= password.length);
  const showStrengthHint = signingUp && password.length > 0 && !isStrongSecret(password);
  const signUpSubmitDisabled = signingUp && (password.length === 0 || password !== confirm);
  const continueDisabled = email.trim().length === 0 || !isValidEmail(email) || emailCheck.isPending;
  const codeSubmitPending = mode === "sign-up" ? magicSignUp.isPending : magicSignIn.isPending;
  const codeSubmitDisabled = code.trim().length === 0 || magicGenerate.isPending || codeSubmitPending;
  const busy = [emailCheck, magicGenerate, passwordSignIn, magicSignIn, passwordSignUp, magicSignUp].some(
    (mutation) => mutation.isPending
  );

  const fail = (message: string, nextStep: SignInStep, nextMode: AuthMode, action: AuthErrorAction | null = null) => {
    setBanner({ message, action });
    setStep(nextStep);
    setMode(nextMode);
  };

  const submitEmail = (event: React.FormEvent) => {
    event.preventDefault();
    setBanner(null);
    const address = email.trim();
    emailCheck.mutate(address, {
      onSuccess: (answer) => {
        const nextMode: AuthMode = answer.existing ? "sign-in" : "sign-up";
        if (answer.status === "MAGIC_CODE" && smtpConfigured) {
          magicGenerate.mutate(address, {
            onSuccess: () => {
              setMode(nextMode);
              setStep("code");
            },
            onError: () => fail(CODE_SEND_MESSAGE, "email", homeMode),
          });
        } else {
          setMode(nextMode);
          setStep("password");
        }
      },
      onError: () => fail(NETWORK_MESSAGE, "email", homeMode),
    });
  };

  const submitPassword = (event: React.FormEvent) => {
    event.preventDefault();
    setBanner(null);
    if (mode === "sign-up" && !isStrongSecret(password)) {
      setBanner({ message: WEAK_SECRET_MESSAGE, action: null });
      return;
    }
    const address = email.trim();
    const onSuccess = (result: SignInResult) => {
      if (result.ok) {
        globalThis.window.location.assign(result.location);
      } else {
        const failure = describeSubmitFailure(result, address, { step: "password", mode });
        fail(failure.message, failure.step, failure.mode, failure.action);
      }
    };
    const onError = () => fail(NETWORK_MESSAGE, "password", mode);
    if (mode === "sign-up") {
      passwordSignUp.mutate(
        { email: address, password, confirmPassword: confirm, ...nextPathInput },
        { onSuccess, onError }
      );
    } else {
      passwordSignIn.mutate({ email: address, password, ...nextPathInput }, { onSuccess, onError });
    }
  };

  const submitCode = (event: React.FormEvent) => {
    event.preventDefault();
    setBanner(null);
    const address = email.trim();
    const onSuccess = (result: SignInResult) => {
      if (result.ok) {
        globalThis.window.location.assign(result.location);
      } else {
        const failure = describeSubmitFailure(result, address, { step: "code", mode });
        fail(failure.message, failure.step, failure.mode, failure.action);
      }
    };
    const onError = () => fail(NETWORK_MESSAGE, "code", mode);
    if (mode === "sign-up") {
      magicSignUp.mutate({ email: address, code: code.trim(), ...nextPathInput }, { onSuccess, onError });
    } else {
      magicSignIn.mutate({ email: address, code: code.trim(), ...nextPathInput }, { onSuccess, onError });
    }
  };

  const resendCode = () => {
    if (resendIn > 0) return;
    setBanner(null);
    magicGenerate.mutate(email.trim(), {
      onSuccess: () => {
        setLastSentAt(Date.now());
        setNow(Date.now());
      },
      onError: () => fail(CODE_SEND_MESSAGE, "code", mode),
    });
  };

  const requestUniqueCode = () => {
    setBanner(null);
    magicGenerate.mutate(email.trim(), {
      onSuccess: () => setStep("code"),
      onError: () => fail(CODE_SEND_MESSAGE, "password", mode),
    });
  };

  const restart = () => {
    setEmail("");
    setEmailTouched(false);
    setMode(homeMode);
    setStep("email");
    setBanner(null);
    setPassword("");
    setConfirm("");
    setConfirmFocused(false);
    setCode("");
    globalThis.window.location.assign(signingUpHome ? SIGN_UP_HOME : SIGN_IN_HOME);
  };

  const passwordPrimaryLabel = mode === "sign-up" ? "Create account" : smtpConfigured ? "Sign in" : "Go to workspace";

  return (
    <section aria-label={cardLabel} className="flex w-full max-w-90 flex-col gap-(--space-6)">
      {banner ? <AuthBanner message={banner.message} action={banner.action} onDismiss={() => setBanner(null)} /> : null}
      {invitation.isFetching ? (
        <div className="flex flex-col items-center gap-(--space-1)">
          <Spinner label="Loading invitation" />
        </div>
      ) : inviteHeader ? (
        <div className="flex flex-col gap-(--space-1)">
          {inviteHeader.workspace.logo_url ? (
            <img src={inviteHeader.workspace.logo_url} alt="" className="size-10 rounded-(--radius-control)" />
          ) : null}
          <h1 className="text-h2 text-(--text)">Join {inviteHeader.workspace.name}</h1>
          <p className="text-body text-(--text-muted)">
            {mode === "sign-up" ? "Create an account to work with the team." : "Log in to work with the team."}
          </p>
        </div>
      ) : mode === "sign-up" ? (
        <div className="flex flex-col gap-(--space-1)">
          <h1 className="text-h2 text-(--text)">Create your account</h1>
          {/* Parity pin (AUTH-004): the scenario reads "Create your Pi Dash account." */}
          <p className="text-body text-(--text-muted)">Set a password to create your Pi Dash account.</p>
        </div>
      ) : (
        <div className="flex flex-col gap-(--space-1)">
          <h1 className="text-h2 text-(--text)">Sign in to Pi Dash</h1>
          <p className="text-body text-(--text-muted)">Welcome back to Pi Dash. Enter your work address to continue.</p>
        </div>
      )}
      {noMethods ? (
        <div className="flex flex-col gap-(--space-1)">
          <h2 className="text-emphasis font-medium text-(--text)">Sign-in is unavailable</h2>
          <p className="text-body text-(--text-muted)">
            This instance has no sign-in methods enabled. Contact your administrator to enable one.
          </p>
        </div>
      ) : (
        <>
          {providers.length > 0 ? (
            <div className="flex flex-col gap-(--space-2)">
              {providers.map((provider) => (
                <Button
                  key={provider.id}
                  variant="secondary"
                  size={providerCompact ? "small" : "medium"}
                  className={providerCompact ? "" : "w-full"}
                  onClick={() => startProviderSignIn(provider)}
                >
                  {provider.label}
                </Button>
              ))}
              <hr className="border-(--border)" />
            </div>
          ) : null}
          {emailMethods ? (
            <>
              {step === "email" ? (
                <form onSubmit={submitEmail} className="flex flex-col gap-(--space-4)">
                  <Input
                    label="Work email"
                    type="email"
                    autoComplete="email"
                    required
                    autoFocus
                    placeholder="name@company.com"
                    value={email}
                    onChange={(event) => setEmail(event.target.value)}
                    onBlur={() => setEmailTouched(true)}
                    {...(showInvalid ? { error: "Enter a valid email address." } : {})}
                    {...(email.length > 0
                      ? {
                          trailing: (
                            <IconButton
                              label="Clear"
                              size="small"
                              onMouseDown={(event) => event.preventDefault()}
                              onClick={() => setEmail("")}
                            >
                              <X size={14} aria-hidden="true" />
                            </IconButton>
                          ),
                        }
                      : {})}
                  />
                  <Button type="submit" variant="primary" loading={busy} disabled={continueDisabled}>
                    Continue
                  </Button>
                </form>
              ) : null}
              {step === "password" ? (
                <form onSubmit={submitPassword} className="flex flex-col gap-(--space-4)">
                  <LockedEmail email={email} onClear={restart} />
                  {next === undefined ? null : <input type="hidden" name="next_path" value={next} readOnly />}
                  <Input
                    label="Password"
                    type={showPassword ? "text" : "password"}
                    autoComplete="current-password"
                    required
                    autoFocus
                    placeholder="Enter password"
                    value={password}
                    onChange={(event) => setPassword(event.target.value)}
                    trailing={
                      <Button
                        variant="ghost"
                        size="small"
                        onClick={() => setShowPassword((current) => !current)}
                        aria-label={showPassword ? "Hide password" : "Show password"}
                      >
                        {showPassword ? "Hide" : "Show"}
                      </Button>
                    }
                  />
                  {showStrengthHint ? <PasswordStrengthHint secret={password} /> : null}
                  {mode === "sign-up" ? (
                    <Input
                      label="Confirm password"
                      type="password"
                      autoComplete="new-password"
                      required
                      value={confirm}
                      onChange={(event) => setConfirm(event.target.value)}
                      onFocus={() => setConfirmFocused(true)}
                      onBlur={() => setConfirmFocused(false)}
                      {...(showMismatch ? { error: MISMATCH_MESSAGE } : {})}
                    />
                  ) : null}
                  {mode === "sign-in" ? (
                    smtpConfigured ? (
                      <a
                        href={`/accounts/forgot-password?email=${encodeURIComponent(email.trim())}`}
                        className="text-body text-(--accent) hover:underline"
                      >
                        Forgot your password?
                      </a>
                    ) : (
                      <Popover
                        trigger={
                          <Button variant="ghost" size="small">
                            Forgot your password?
                          </Button>
                        }
                      >
                        <p className="text-body text-(--text-muted)">
                          Password resets need email, but this server has no SMTP mail configured. Contact your
                          administrator.
                        </p>
                      </Popover>
                    )
                  ) : null}
                  <Button
                    type="submit"
                    variant="primary"
                    loading={busy}
                    {...(signingUp ? { disabled: signUpSubmitDisabled } : {})}
                  >
                    {passwordPrimaryLabel}
                  </Button>
                  {mode === "sign-in" && smtpConfigured ? (
                    <Button variant="secondary" onClick={requestUniqueCode} loading={magicGenerate.isPending}>
                      Sign in with unique code
                    </Button>
                  ) : null}
                </form>
              ) : null}
              {step === "code" ? (
                <form onSubmit={submitCode} className="flex flex-col gap-(--space-4)">
                  <LockedEmail email={email} onClear={restart} />
                  {next === undefined ? null : <input type="hidden" name="next_path" value={next} readOnly />}
                  <Input
                    label="One-time code"
                    inputMode="numeric"
                    autoComplete="one-time-code"
                    required
                    autoFocus
                    placeholder="123456"
                    value={code}
                    onChange={(event) => setCode(event.target.value)}
                  />
                  <Button type="submit" variant="primary" loading={busy} disabled={codeSubmitDisabled}>
                    {mode === "sign-up" ? "Verify and create account" : "Verify and sign in"}
                  </Button>
                  <div className="flex justify-start">
                    <Button
                      variant="ghost"
                      size="small"
                      onClick={resendCode}
                      disabled={resendIn > 0 || magicGenerate.isPending}
                    >
                      {resendIn > 0
                        ? `Resend in ${resendIn} ${resendIn === 1 ? "second" : "seconds"}`
                        : magicGenerate.isPending
                          ? "Requesting code…"
                          : "Resend code"}
                    </Button>
                  </div>
                </form>
              ) : null}
            </>
          ) : null}
        </>
      )}
    </section>
  );
}
