// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// GitHub provider card (parity/specs/workspace-settings.md §16, WSET-017).
// The disconnected form asks for a fine-grained personal token and verifies
// it server-side; the connected form reports the verified login and offers
// a disconnect that leaves project bindings in place but stops their sync.
// A verified rejection keeps the typed token and shows the failure inline
// rather than expiring the session (NEWFRONT-421).

import { Button, Input } from "@pidash/kit";
import * as React from "react";

import { canDisconnectGithub, canSubmitToken, githubIdentity } from "./integrationQueries.js";
import { ProviderCard } from "./ProviderCard.js";

export const GITHUB_EXPLAINER =
  "Connect a fine-grained PAT (personal access token) with issue read access. Pi Dash mirrors GitHub issues into your projects.";
export const GITHUB_TOKEN_LABEL = "Personal access token";
export const GITHUB_TOKEN_PLACEHOLDER = "github_pat_...";
export const GITHUB_HELP_HREF = "https://github.com/settings/tokens?type=beta";
export const GITHUB_HELP_LABEL = "Create a fine-grained token";
export const GITHUB_LOADING_STATUS = "Loading GitHub connection…";
export const GITHUB_READ_FAILED_STATUS = "We could not load the GitHub connection.";
export const GITHUB_RETRY_LABEL = "Try again";
export const GITHUB_CONNECTED_STATUS_PREFIX = "Connected as ";
export const GITHUB_CONNECTED_HINT = "Bind repositories from each project's settings to mirror issues.";
export const GITHUB_CONNECTED_BADGE = "Verified";
export const GITHUB_CONNECTED_NOTICE = "Connected";
export const GITHUB_CONNECTED_NOTICE_BODY = "We verified the token against GitHub and saved the credential.";
export const GITHUB_CONNECT_FAILED_NOTICE = "Connection failed";
export const GITHUB_CONNECT_FAILED_BODY = "GitHub rejected this token. Check its access and try again.";
export const GITHUB_DISCONNECTED_NOTICE = "Disconnected";
export const GITHUB_DISCONNECTED_NOTICE_BODY =
  "The GitHub connection was removed. Project syncs that used it are disabled.";
export const GITHUB_DISCONNECT_FAILED_NOTICE = "Disconnect failed";
export const GITHUB_DISCONNECT_FAILED_BODY = "We could not disconnect GitHub. Try again.";

export interface GithubCardProps {
  /** Resolved status, or undefined while the read is in flight. */
  status: { connected: boolean; github_user_login?: string | null | undefined } | undefined;
  statusPending: boolean;
  /** Resolves true when the token verified and the card should clear it. */
  onConnect: (token: string) => Promise<boolean>;
  connectPending: boolean;
  onDisconnect: () => void;
  disconnectPending: boolean;
  /** Set when the status read failed; the card offers a retry instead of forms. */
  readFailed?: boolean;
  onRetry?: (() => void) | undefined;
}

export function GithubCard({
  status,
  statusPending,
  onConnect,
  connectPending,
  onDisconnect,
  disconnectPending,
  readFailed = false,
  onRetry,
}: GithubCardProps): React.ReactElement {
  const [token, setToken] = React.useState("");
  const connected = status?.connected === true;

  const [verifying, setVerifying] = React.useState(false);
  const submit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (!canSubmitToken(token, connectPending || verifying)) return;
    setVerifying(true);
    void onConnect(token)
      .then((saved) => {
        if (saved) setToken("");
      })
      .finally(() => {
        setVerifying(false);
      });
  };

  if (readFailed) {
    return (
      <ProviderCard provider="github" name="GitHub" status={GITHUB_READ_FAILED_STATUS}>
        <div>
          <Button onClick={() => onRetry?.()}>{GITHUB_RETRY_LABEL}</Button>
        </div>
      </ProviderCard>
    );
  }

  if (statusPending || status === undefined) {
    return <ProviderCard provider="github" name="GitHub" status={GITHUB_LOADING_STATUS} />;
  }

  if (connected) {
    return (
      <ProviderCard
        provider="github"
        name="GitHub"
        status={`${GITHUB_CONNECTED_STATUS_PREFIX}${githubIdentity(status)}`}
        statusDetail={GITHUB_CONNECTED_HINT}
      >
        <div className="flex items-center gap-(--space-3)">
          <span className="text-meta rounded-(--radius-control) border border-(--border) px-(--space-2) py-(--space-1) text-(--text-muted)">
            {GITHUB_CONNECTED_BADGE}
          </span>
          <Button
            variant="danger"
            loading={disconnectPending}
            disabled={!canDisconnectGithub(status, disconnectPending)}
            onClick={onDisconnect}
          >
            Disconnect
          </Button>
        </div>
      </ProviderCard>
    );
  }

  return (
    <ProviderCard provider="github" name="GitHub" status={GITHUB_EXPLAINER}>
      <form className="flex flex-col gap-(--space-3)" onSubmit={submit}>
        <Input
          label={GITHUB_TOKEN_LABEL}
          placeholder={GITHUB_TOKEN_PLACEHOLDER}
          value={token}
          type="password"
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => {
            setToken(event.target.value);
          }}
        />
        <div className="flex flex-wrap items-center gap-(--space-3)">
          <Button
            type="submit"
            variant="primary"
            loading={connectPending || verifying}
            disabled={!canSubmitToken(token, connectPending || verifying)}
          >
            Connect
          </Button>
          <a
            href={GITHUB_HELP_HREF}
            target="_blank"
            rel="noreferrer"
            className="text-body text-(--accent) underline-offset-2 hover:underline"
          >
            {GITHUB_HELP_LABEL}
          </a>
        </div>
      </form>
    </ProviderCard>
  );
}
