// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Workspace join-by-email sub-view (AUTH-036). An autofocused admin-email
// field with a validity-gated submit; posting parks the user on the
// holding view, except the already-member shortcut, which finishes
// directly. A rejected submit toasts and stays. Written from
// parity/specs/auth.md.

import { Button, Input } from "@pidash/kit";
import * as React from "react";

import { notifyError } from "../../../shared/shell/index.js";
import { useCreateJoinRequest } from "../api.js";
import { isValidEmail } from "./SignInCard.js";

export const JOIN_FAILED_TITLE = "Could not send your request";

export interface WorkspaceJoinByEmailViewProps {
  /** Called with the requested address after it is recorded; the caller parks on the holding view. */
  onRequested: (adminEmail: string) => void;
  /** Called when the address resolves to a workspace the caller already belongs to. */
  onAlreadyMember: () => void;
  onBack: () => void;
}

export function WorkspaceJoinByEmailView({
  onRequested,
  onAlreadyMember,
  onBack,
}: WorkspaceJoinByEmailViewProps): React.ReactElement {
  const request = useCreateJoinRequest();
  const [email, setEmail] = React.useState("");
  const pending = request.isPending;
  const valid = isValidEmail(email);

  const submit = async (event: React.FormEvent): Promise<void> => {
    event.preventDefault();
    const trimmed = email.trim();
    try {
      const result = await request.mutateAsync(trimmed);
      if (result.workspace_slug !== undefined) {
        onAlreadyMember();
      } else {
        onRequested(trimmed);
      }
    } catch (error) {
      notifyError(JOIN_FAILED_TITLE, (error as Error).message);
    }
  };

  return (
    <section aria-labelledby="join-by-email-heading" className="flex flex-col gap-(--space-6)">
      <div className="flex flex-col gap-(--space-2)">
        <h1 id="join-by-email-heading" className="text-h2 text-(--text)">
          Request workspace access
        </h1>
        <p className="text-body text-(--text-muted)">Enter a workspace admin&apos;s email to ask for access.</p>
      </div>

      <form onSubmit={(event) => void submit(event)} className="flex flex-col gap-(--space-4)">
        <Input
          label="Admin email address"
          type="email"
          value={email}
          autoFocus
          onChange={(event) => setEmail(event.target.value)}
        />
        <div className="flex flex-col gap-(--space-2)">
          <Button type="submit" variant="primary" size="large" loading={pending} disabled={!valid || pending}>
            Send join request
          </Button>
          <Button type="button" variant="ghost" size="large" onClick={onBack} disabled={pending}>
            Back to creating a workspace
          </Button>
        </div>
      </form>
    </section>
  );
}
