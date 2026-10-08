// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Sign-in card (first vertical slice). Email first: the server decides
// whether the address signs in with a password or an emailed code, then
// the card shows that step. Failures render the server code as a banner
// and land on the step that can fix them. Sign-up, providers and password
// recovery arrive with their own epics (NEWFRONT-69).

import { Button, Input } from "@pidash/kit";
import * as React from "react";

import { useEmailCheck, useMagicGenerate, useMagicSignIn, usePasswordSignIn } from "../api.js";
import { describeSignInError, type SignInStep } from "../authErrors.js";

export interface SignInCardProps {
  /** Where the server sends the user after sign-in (as next_path). */
  next?: string | undefined;
  initialEmail?: string | undefined;
  /** A server error_code carried in the URL after a native-form failure. */
  initialErrorCode?: string | null | undefined;
}

const RESEND_COOLDOWN_S = 30;

function Banner({ message, onDismiss }: { message: string; onDismiss: () => void }): React.ReactElement {
  return (
    <div
      role="alert"
      className="flex items-start gap-(--space-2) rounded-(--radius-control) border border-(--danger) p-(--space-4)"
    >
      <p className="text-body flex-1 text-(--text)">{message}</p>
      <Button variant="ghost" size="small" onClick={onDismiss} aria-label="Dismiss error">
        Dismiss
      </Button>
    </div>
  );
}

export function SignInCard({ next, initialEmail = "", initialErrorCode }: SignInCardProps): React.ReactElement {
  const initialError = React.useMemo(() => describeSignInError(initialErrorCode), [initialErrorCode]);
  const [email, setEmail] = React.useState(initialEmail);
  const [step, setStep] = React.useState<SignInStep>(initialError?.step ?? "email");
  const [banner, setBanner] = React.useState<string | null>(initialError?.message ?? null);
  const [password, setPassword] = React.useState("");
  const [code, setCode] = React.useState("");
  const [lastSentAt, setLastSentAt] = React.useState<number | null>(null);
  const [now, setNow] = React.useState(() => Date.now());

  const emailCheck = useEmailCheck();
  const magicGenerate = useMagicGenerate();
  const passwordSignIn = usePasswordSignIn();
  const magicSignIn = useMagicSignIn();
  const busy = emailCheck.isPending || magicGenerate.isPending || passwordSignIn.isPending || magicSignIn.isPending;

  React.useEffect(() => {
    if (lastSentAt === null) return;
    const timer = globalThis.setInterval(() => setNow(Date.now()), 1000);
    return () => globalThis.clearInterval(timer);
  }, [lastSentAt]);

  const resendIn = lastSentAt === null ? 0 : Math.max(0, RESEND_COOLDOWN_S - Math.floor((now - lastSentAt) / 1000));

  const fail = (message: string, nextStep: SignInStep) => {
    setBanner(message);
    setStep(nextStep);
  };

  const submitEmail = (event: React.FormEvent) => {
    event.preventDefault();
    setBanner(null);
    emailCheck.mutate(email.trim(), {
      onSuccess: (answer) => {
        if (!answer.existing) {
          fail("No account uses this address yet.", "email");
          return;
        }
        if (answer.status === "MAGIC_CODE") {
          magicGenerate.mutate(email.trim(), {
            onSuccess: () => {
              setLastSentAt(Date.now());
              setNow(Date.now());
              setStep("code");
            },
            onError: () => fail("Could not send a code. Try again in a moment.", "email"),
          });
        } else {
          setStep("password");
        }
      },
      onError: () => fail("Could not reach the server. Check your connection and try again.", "email"),
    });
  };

  const submitPassword = (event: React.FormEvent) => {
    event.preventDefault();
    setBanner(null);
    passwordSignIn.mutate(
      { email: email.trim(), password, ...(next === undefined ? {} : { nextPath: next }) },
      {
        onSuccess: (result) => {
          if (result.ok) {
            globalThis.window.location.assign(result.location);
          } else {
            const mapped = describeSignInError(result.code);
            fail(mapped?.message ?? result.message, mapped?.step ?? "password");
          }
        },
        onError: () => fail("Could not reach the server. Check your connection and try again.", "password"),
      }
    );
  };

  const submitCode = (event: React.FormEvent) => {
    event.preventDefault();
    setBanner(null);
    magicSignIn.mutate(
      { email: email.trim(), code: code.trim(), ...(next === undefined ? {} : { nextPath: next }) },
      {
        onSuccess: (result) => {
          if (result.ok) {
            globalThis.window.location.assign(result.location);
          } else {
            const mapped = describeSignInError(result.code);
            fail(mapped?.message ?? result.message, mapped?.step ?? "code");
          }
        },
        onError: () => fail("Could not reach the server. Check your connection and try again.", "code"),
      }
    );
  };

  const resendCode = () => {
    if (resendIn > 0) return;
    setBanner(null);
    magicGenerate.mutate(email.trim(), {
      onSuccess: () => {
        setLastSentAt(Date.now());
        setNow(Date.now());
      },
      onError: () => fail("Could not send a code. Try again in a moment.", "code"),
    });
  };

  const restart = () => {
    setStep("email");
    setBanner(null);
    setPassword("");
    setCode("");
  };

  return (
    <section aria-label="Sign in" className="flex w-full max-w-90 flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-1)">
        <h1 className="text-h2 text-(--text)">Sign in to Pi Dash</h1>
        {step === "email" ? (
          <p className="text-body text-(--text-muted)">Enter your work address to continue.</p>
        ) : (
          <p className="text-body text-(--text-muted)">
            {step === "password" ? "Enter your password" : "Enter the code from your email"} for {email}.
          </p>
        )}
      </div>
      {banner ? <Banner message={banner} onDismiss={() => setBanner(null)} /> : null}
      {step === "email" ? (
        <form onSubmit={submitEmail} className="flex flex-col gap-(--space-4)">
          <Input
            label="Work email"
            type="email"
            autoComplete="email"
            required
            autoFocus
            value={email}
            onChange={(event) => setEmail(event.target.value)}
          />
          <Button type="submit" variant="primary" loading={busy} disabled={email.trim().length === 0}>
            Continue
          </Button>
        </form>
      ) : null}
      {step === "password" ? (
        <form onSubmit={submitPassword} className="flex flex-col gap-(--space-4)">
          <Input
            label="Password"
            type="password"
            autoComplete="current-password"
            required
            autoFocus
            value={password}
            onChange={(event) => setPassword(event.target.value)}
          />
          <Button type="submit" variant="primary" loading={busy}>
            Sign in
          </Button>
          <Button variant="ghost" onClick={restart}>
            Use a different address
          </Button>
        </form>
      ) : null}
      {step === "code" ? (
        <form onSubmit={submitCode} className="flex flex-col gap-(--space-4)">
          <Input
            label="One-time code"
            inputMode="numeric"
            autoComplete="one-time-code"
            required
            autoFocus
            value={code}
            onChange={(event) => setCode(event.target.value)}
          />
          <Button type="submit" variant="primary" loading={busy}>
            Verify and sign in
          </Button>
          <div className="flex items-center justify-between">
            <Button variant="ghost" size="small" onClick={restart}>
              Use a different address
            </Button>
            <Button
              variant="ghost"
              size="small"
              onClick={resendCode}
              disabled={resendIn > 0 || magicGenerate.isPending}
            >
              {resendIn > 0 ? `Resend code in ${resendIn}s` : "Resend code"}
            </Button>
          </div>
        </form>
      ) : null}
    </section>
  );
}
