// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Cross-app link (F-09). Screens that have not migrated yet live in the
// old app behind the same proxy, so they render as plain anchors (a
// full-page load keeps the session cookie on the same origin). Migrated
// screens render as router links for client-side navigation. Both read
// the same migrated-route list, so a link flips the moment its prefix
// joins the list — no call site changes.

import { Link } from "@tanstack/react-router";
import * as React from "react";

import { MIGRATED_ROUTE_ENTRIES, resolveLinkKind } from "./migrated-routes.js";

export interface CrossAppLinkProps extends Omit<React.AnchorHTMLAttributes<HTMLAnchorElement>, "href"> {
  /** App path such as "/sign-in" or "/acme/projects/1/issues". */
  to: string;
  /** Deep-link anchor without the leading "#", such as "voice-dictation". */
  hash?: string | undefined;
  /** Prefix override for tests and previews; defaults to the bundled list. */
  prefixes?: readonly string[] | undefined;
  children: React.ReactNode;
  /** React 19 ref-as-prop: forwarded to the anchor (or the router link). */
  ref?: React.Ref<HTMLAnchorElement> | undefined;
}

// The installed router's prop types are anchored on its own copies of the
// shared UI types (pnpm installs several), which are structurally
// incompatible with the app's copies even though the runtime React is a
// single copy. Props cross that boundary in exactly one place, asserted
// here — never widened, never spread piecemeal.
type RouterLinkProps = Parameters<typeof Link>[0];

export function CrossAppLink({ to, hash, prefixes, children, ...rest }: CrossAppLinkProps): React.ReactElement {
  if (resolveLinkKind(to, prefixes ?? MIGRATED_ROUTE_ENTRIES) === "anchor") {
    return (
      <a href={hash === undefined ? to : `${to}#${hash}`} {...rest}>
        {children}
      </a>
    );
  }
  const linkProps = { to, ...(hash === undefined ? {} : { hash }), ...rest, children } as RouterLinkProps;
  return <Link {...linkProps} />;
}
