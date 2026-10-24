// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Signed-in account menu (parity/specs/space.md SPC-014). The trigger
// carries the whole identity — avatar plus one name line — and the menu
// holds exactly one action: sign-out as a native form post carrying the
// CSRF token and the subtree-relative return position, which the server
// redirects back to. The board navbar and the landing guidance view
// share this menu; each call site resolves its own return position.
// The form renders only once the token has loaded; before that the menu
// is empty. No profile link, no settings, no workspace entries: none of
// those exist for a public visitor.
// Gate DOM contract: button[data-testid="space-account-trigger"]
// (avatar + name line) opens a popup holding exactly
// form[data-testid="space-signout-form"] (method post, action
// /auth/spaces/sign-out/) with input[name="csrfmiddlewaretoken"],
// input[name="next_path"] and a "Sign out" submit.
import type { Me } from "@pidash/api-client";
import { Avatar, Menu, type MenuEntry } from "@pidash/kit";
import * as React from "react";

import { spaceRuntime } from "../api/client.js";
import { createSpaceAuthServices, useSpaceCsrfToken, type SpaceAuthServices } from "../auth/index.js";
import { spaceAccountLabel } from "../lib/spaceAccount.js";
import { spaceMessage } from "../lib/spaceMessages.js";

/** Native sign-out post; the server clears the session and redirects. */
export const SPACE_SIGN_OUT_ACTION = "/auth/spaces/sign-out/";

export interface SpaceAccountMenuProps {
  viewer: Me;
  /** Subtree-relative position the server redirects back to after sign-out. */
  returnPath: string;
  /** CSRF reader; a public-safe default client when omitted. */
  services?: SpaceAuthServices | undefined;
}

export function SpaceAccountMenu({ viewer, returnPath, services }: SpaceAccountMenuProps): React.ReactElement {
  const resolved = React.useMemo(() => services ?? createSpaceAuthServices(spaceRuntime.client), [services]);
  const { token } = useSpaceCsrfToken(resolved);
  const label = spaceAccountLabel(viewer);
  const avatar = viewer.avatar_url ?? undefined;
  const items: MenuEntry[] =
    token === null
      ? []
      : [
          {
            custom: true,
            entryKey: "sign-out",
            node: (
              <form method="post" action={SPACE_SIGN_OUT_ACTION} data-testid="space-signout-form">
                <input type="hidden" name="csrfmiddlewaretoken" value={token} />
                <input type="hidden" name="next_path" value={returnPath} />
                <button
                  type="submit"
                  className="text-body flex w-full cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) px-(--space-4) py-(--space-3) text-(--text) hover:bg-(--subtle)"
                >
                  {spaceMessage("account.signOut")}
                </button>
              </form>
            ),
          },
        ];
  return (
    <Menu
      trigger={
        <button
          type="button"
          data-testid="space-account-trigger"
          className="flex max-w-56 cursor-pointer items-center gap-(--space-2) rounded-(--radius-control) border border-(--border) bg-(--bg) px-(--space-2) py-(--space-1) hover:bg-(--subtle)"
        >
          <Avatar name={label} size="small" {...(avatar === undefined ? {} : { src: avatar })} />
          <span className="text-body truncate text-(--text)">{label}</span>
        </button>
      }
      items={items}
    />
  );
}
