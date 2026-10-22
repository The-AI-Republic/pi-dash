// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Personal access token page (parity/specs/workspace-settings.md §25,
// WSET-025). Lists the signed-in user's own tokens with an Add control,
// shows a loader until the list resolves, and offers an illustrated empty
// state whose action opens the same create dialog. The page is a
// self-sufficient leaf: the profile settings shell mounts it later, but it
// renders and proves its content without that parent layout.

import { Button, EmptyState, Spinner } from "@pidash/kit";
import { KeyRound } from "lucide-react";
import * as React from "react";

import { useMe } from "../../../core/session/index.js";
import { usePersonalTokens } from "./api.js";
import { CreateTokenDialog } from "./CreateTokenDialog.js";
import {
  TOKEN_ADD_LABEL,
  TOKEN_EMPTY_ACTION,
  TOKEN_EMPTY_DESCRIPTION,
  TOKEN_EMPTY_TITLE,
  TOKEN_HEADING,
  TOKEN_LOAD_FAILED,
  TOKEN_LOADING_LABEL,
} from "./model.js";
import { TokenList } from "./TokenList.js";

export const TOKEN_PAGE_INTRO =
  "Personal access tokens let external scripts and integrations act as you. Treat every secret like a password.";

/** A small inline illustration in the kit's illustrated empty-state slot. */
function TokenEmptyArt(): React.ReactElement {
  return (
    <div className="grid size-24 place-items-center rounded-full bg-(--accent-subtle) text-(--accent)">
      <KeyRound size={40} strokeWidth={1.25} aria-hidden="true" />
    </div>
  );
}

export interface PersonalAccessTokensPageProps {
  now?: Date | undefined;
}

export function PersonalAccessTokensPage({ now = new Date() }: PersonalAccessTokensPageProps): React.ReactElement {
  const [createOpen, setCreateOpen] = React.useState(false);
  const tokens = usePersonalTokens();
  const me = useMe();
  const timeZone = me.data?.user_timezone ?? "UTC";

  const add = (
    <Button variant="primary" onClick={() => setCreateOpen(true)}>
      {TOKEN_ADD_LABEL}
    </Button>
  );

  return (
    <section aria-labelledby="personal-access-tokens-heading" className="flex flex-col gap-(--space-6)">
      <div className="flex items-start justify-between gap-(--space-4)">
        <div className="flex flex-col gap-(--space-2)">
          <h1 id="personal-access-tokens-heading" className="text-h2 text-(--text)">
            {TOKEN_HEADING}
          </h1>
          <p className="text-body max-w-2xl text-(--text-muted)">{TOKEN_PAGE_INTRO}</p>
        </div>
        {add}
      </div>

      {tokens.isPending ? (
        <div data-testid="api-tokens-loader" className="grid place-items-center py-(--space-8)">
          <Spinner label={TOKEN_LOADING_LABEL} />
        </div>
      ) : tokens.isError ? (
        <p role="alert" className="text-body text-(--text)">
          {TOKEN_LOAD_FAILED}
        </p>
      ) : tokens.data.length === 0 ? (
        <div className="flex flex-col items-center gap-(--space-4)">
          <EmptyState
            variant="illustrated"
            art={<TokenEmptyArt />}
            title={TOKEN_EMPTY_TITLE}
            description={TOKEN_EMPTY_DESCRIPTION}
          />
          <Button variant="primary" onClick={() => setCreateOpen(true)}>
            {TOKEN_EMPTY_ACTION}
          </Button>
        </div>
      ) : (
        <TokenList tokens={tokens.data} now={now} timeZone={timeZone} />
      )}

      <CreateTokenDialog open={createOpen} onClose={() => setCreateOpen(false)} timeZone={timeZone} />
    </section>
  );
}
