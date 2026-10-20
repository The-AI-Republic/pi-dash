// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// GitLab provider card (parity/specs/workspace-settings.md §17, WSET-018).
// The workspace lists provider accounts; the first non-revoked GitLab row
// is the connected account. Registration verifies a personal token (and an
// optional self-hosted host) server-side; disconnecting removes that one
// account, keeps its project bindings and stops their sync.

import { Button, Input } from "@pidash/kit";
import * as React from "react";

import type { GitProviderAccount } from "@pidash/api-client";

import { accountIdentity, GITLAB_PROVIDER_NAME, selectActiveGitlabAccount } from "./integrationQueries.js";
import { ProviderCard } from "./ProviderCard.js";

export const GITLAB_EXPLAINER =
  "Connect a GitLab token with API access to bind repositories and mirror GitLab issues into your projects.";
export const GITLAB_TOKEN_LABEL = "Token";
export const GITLAB_TOKEN_PLACEHOLDER = "glpat-...";
export const GITLAB_HOST_LABEL = "GitLab host (optional)";
export const GITLAB_HOST_PLACEHOLDER = "https://gitlab.com";
export const GITLAB_TOKEN_HINT = "Leave the host blank for gitlab.com.";
export const GITLAB_LOADING_STATUS = "Loading GitLab accounts…";
export const GITLAB_READ_FAILED_STATUS = "We could not load the GitLab accounts.";
export const GITLAB_RETRY_LABEL = "Try again";
export const GITLAB_CONNECTED_STATUS_PREFIX = "Connected as ";
export const GITLAB_CONNECTED_HINT = "Bind repositories from each project's settings; syncs follow this account.";
export const GITLAB_CONNECTED_BADGE = "Verified";
export const GITLAB_CONNECTED_NOTICE = "Connected";
export const GITLAB_CONNECTED_NOTICE_BODY = "We verified the token with GitLab and registered the account.";
export const GITLAB_CONNECT_FAILED_NOTICE = "Connection failed";
export const GITLAB_CONNECT_FAILED_BODY = "GitLab rejected this token. Check its access and try again.";
export const GITLAB_DISCONNECTED_NOTICE = "Disconnected";
export const GITLAB_DISCONNECTED_NOTICE_BODY =
  "The GitLab account was removed. Project syncs that used it are disabled.";
export const GITLAB_DISCONNECT_FAILED_NOTICE = "Disconnect failed";
export const GITLAB_DISCONNECT_FAILED_BODY = "We could not disconnect this GitLab account. Try again.";

/** `Connected as {login} on {host}`, with the account identity fallbacks. */
export function gitlabConnectedStatus(account: GitProviderAccount): string {
  return `${GITLAB_CONNECTED_STATUS_PREFIX}${accountIdentity(account)} on ${account.host_url}`;
}

export interface GitlabCardProps {
  accounts: readonly GitProviderAccount[] | undefined;
  accountsPending: boolean;
  /** Resolves true when the account registered and the card should clear the token. */
  onConnect: (input: { token: string; hostUrl: string }) => Promise<boolean>;
  connectPending: boolean;
  onDisconnect: (accountId: string) => void;
  disconnectPending: boolean;
  readFailed?: boolean;
  onRetry?: (() => void) | undefined;
}

export function GitlabCard({
  accounts,
  accountsPending,
  onConnect,
  connectPending,
  onDisconnect,
  disconnectPending,
  readFailed = false,
  onRetry,
}: GitlabCardProps): React.ReactElement {
  const [token, setToken] = React.useState("");
  const [host, setHost] = React.useState("");
  const active = React.useMemo(() => selectActiveGitlabAccount(accounts ?? []), [accounts]);
  const [verifying, setVerifying] = React.useState(false);
  const busy = connectPending || verifying;
  const submit = (event: React.FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    if (busy || token.trim() === "") return;
    setVerifying(true);
    void onConnect({ token: token.trim(), hostUrl: host.trim() })
      .then((saved) => {
        if (saved) setToken("");
      })
      .finally(() => {
        setVerifying(false);
      });
  };

  if (readFailed) {
    return (
      <ProviderCard provider="gitlab" name={GITLAB_PROVIDER_NAME} status={GITLAB_READ_FAILED_STATUS}>
        <div>
          <Button onClick={() => onRetry?.()}>{GITLAB_RETRY_LABEL}</Button>
        </div>
      </ProviderCard>
    );
  }

  if (accountsPending || accounts === undefined) {
    return <ProviderCard provider="gitlab" name={GITLAB_PROVIDER_NAME} status={GITLAB_LOADING_STATUS} />;
  }

  if (active !== null) {
    return (
      <ProviderCard
        provider="gitlab"
        name={GITLAB_PROVIDER_NAME}
        status={gitlabConnectedStatus(active)}
        statusDetail={GITLAB_CONNECTED_HINT}
      >
        <div className="flex items-center gap-(--space-3)">
          <span className="text-meta rounded-(--radius-control) border border-(--border) px-(--space-2) py-(--space-1) text-(--text-muted)">
            {GITLAB_CONNECTED_BADGE}
          </span>
          <Button
            variant="danger"
            loading={disconnectPending}
            disabled={disconnectPending}
            onClick={() => onDisconnect(active.id)}
          >
            Disconnect
          </Button>
        </div>
      </ProviderCard>
    );
  }

  return (
    <ProviderCard provider="gitlab" name={GITLAB_PROVIDER_NAME} status={GITLAB_EXPLAINER}>
      <form className="flex flex-col gap-(--space-3)" onSubmit={submit}>
        <Input
          label={GITLAB_HOST_LABEL}
          hint={GITLAB_TOKEN_HINT}
          placeholder={GITLAB_HOST_PLACEHOLDER}
          value={host}
          inputMode="url"
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => {
            setHost(event.target.value);
          }}
        />
        <Input
          label={GITLAB_TOKEN_LABEL}
          placeholder={GITLAB_TOKEN_PLACEHOLDER}
          value={token}
          type="password"
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => {
            setToken(event.target.value);
          }}
        />
        <div>
          <Button type="submit" variant="primary" loading={busy} disabled={busy || token.trim() === ""}>
            Connect
          </Button>
        </div>
      </form>
    </ProviderCard>
  );
}
