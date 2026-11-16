// Copyright (c) Pi Dash contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.
// Single-invitation emailed-link screen (AUTH-027/028). Written from
// parity/specs/auth.md §§AUTH-027–028: a pending invite renders the
// accept/ignore actions, while answered or broken links render the
// already-member, inactive, or not-found states instead. Accepting lands
// in the joined workspace on an address match and home otherwise;
// ignoring always lands home.

import { useNavigate } from "@tanstack/react-router";
import { Button, Spinner } from "@pidash/kit";
import * as React from "react";

import { useDocumentHead } from "../../../shared/shell/index.js";
import { useAnswerWorkspaceInvitation } from "../api.js";
import { decideSingleInvitationView, INVITATION_IGNORE_LANDING, resolveSingleAcceptLanding } from "../invitations.js";
import { useWorkspaceInvitation } from "../invitations.js";
import { useViewerEmail } from "../queries.js";

export const SINGLE_INVITATION_TITLE = "Workspace invitation";
export const GITHUB_STAR_URL = "https://github.com/The-AI-Republic/pi-dash";
export const COMMUNITY_URL = "https://github.com/The-AI-Republic/pi-dash/discussions";

export interface SingleInvitationProps {
  invitationId?: string | undefined;
  slug?: string | undefined;
  token?: string | undefined;
}

export function SingleInvitation({ invitationId, slug, token }: SingleInvitationProps): React.ReactElement {
  const detail = useWorkspaceInvitation(slug, invitationId);
  const viewerEmail = useViewerEmail();
  const answer = useAnswerWorkspaceInvitation();
  const navigate = useNavigate();
  useDocumentHead({ title: SINGLE_INVITATION_TITLE, robots: "noindex, nofollow" });
  const view = decideSingleInvitationView({ invitation: detail.data ?? null, failed: detail.isError });

  if (view.state === "loading") {
    return (
      <main className="flex min-h-screen items-center justify-center bg-(--bg)">
        <Spinner label="Loading invitation" />
      </main>
    );
  }

  if (view.state === "pending") {
    return (
      <PendingInvitation
        workspaceName={view.invitation.workspace.name}
        inviteeEmail={view.invitation.email}
        answering={answer.isPending}
        onAccept={() => {
          answer.mutate(
            {
              slug: view.invitation.workspace.slug,
              invitationId: view.invitation.id,
              accepted: true,
              token: token ?? "",
            },
            {
              onSuccess: () => {
                void navigate({
                  href: resolveSingleAcceptLanding({
                    inviteeEmail: view.invitation.email,
                    viewerEmail,
                    slug: view.invitation.workspace.slug,
                  }),
                });
              },
            }
          );
        }}
        onIgnore={() => {
          // Ignoring needs the emailed token; without it the action stays
          // inert instead of answering a link that cannot be verified.
          if (token === undefined) return;
          answer.mutate(
            {
              slug: view.invitation.workspace.slug,
              invitationId: view.invitation.id,
              accepted: false,
              token,
            },
            {
              onSuccess: () => {
                void navigate({ href: INVITATION_IGNORE_LANDING });
              },
            }
          );
        }}
      />
    );
  }

  if (view.state === "already-member") {
    return (
      <InvitationMessage
        heading={`You are already a member of ${view.invitation.workspace.name}.`}
        body="This invitation was answered before your account existed, so there is nothing left to accept."
      >
        <Button
          variant="primary"
          onClick={() => {
            void navigate({ href: "/" });
          }}
        >
          Continue to home
        </Button>
      </InvitationMessage>
    );
  }

  if (view.state === "not-found") {
    return (
      <InvitationMessage
        heading="Invitation not found"
        body="No invitation matches this link. It may have been deleted."
      >
        <Button
          variant="primary"
          onClick={() => {
            void navigate({ href: "/" });
          }}
        >
          Back to home
        </Button>
      </InvitationMessage>
    );
  }

  return (
    <InvitationMessage
      heading="This invitation link is not active anymore."
      body="It was declined or withdrawn, or the link is broken. Sign in or head home to continue."
    >
      <Button
        variant="primary"
        onClick={() => {
          void navigate({ href: "/" });
        }}
      >
        {viewerEmail === undefined ? "Sign in to continue" : "Continue to home"}
      </Button>
      <a className="text-body text-(--text-muted) underline" href={GITHUB_STAR_URL}>
        Star us on GitHub
      </a>
      <a className="text-body text-(--text-muted) underline" href={COMMUNITY_URL}>
        Join our community of active creators
      </a>
    </InvitationMessage>
  );
}

function PendingInvitation({
  workspaceName,
  inviteeEmail,
  answering,
  onAccept,
  onIgnore,
}: {
  workspaceName: string;
  inviteeEmail: string;
  answering: boolean;
  onAccept: () => void;
  onIgnore: () => void;
}): React.ReactElement {
  const [action, setAction] = React.useState<"accept" | "ignore" | null>(null);
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-xl flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center">
      <h1 className="text-h2 text-(--text)">{workspaceName}</h1>
      <p className="text-body text-(--text-muted)">
        You have been invited to join this workspace. Accept to become a member, or ignore to dismiss it.
      </p>
      <p className="text-meta text-(--text-muted)">Sent to {inviteeEmail}.</p>
      <div className="flex items-center gap-(--space-3)">
        <Button
          variant="primary"
          size="large"
          loading={answering && action === "accept"}
          disabled={answering}
          onClick={() => {
            setAction("accept");
            onAccept();
          }}
        >
          Accept
        </Button>
        <Button
          variant="secondary"
          size="large"
          loading={answering && action === "ignore"}
          disabled={answering}
          onClick={() => {
            setAction("ignore");
            onIgnore();
          }}
        >
          Ignore
        </Button>
      </div>
    </main>
  );
}

function InvitationMessage({
  heading,
  body,
  children,
}: {
  heading: string;
  body: string;
  children: React.ReactNode;
}): React.ReactElement {
  return (
    <main className="mx-auto flex min-h-screen w-full max-w-xl flex-col items-center justify-center gap-(--space-4) bg-(--bg) p-(--space-8) text-center">
      <h1 className="text-h2 text-(--text)">{heading}</h1>
      <p className="text-body text-(--text-muted)">{body}</p>
      <div className="flex flex-col items-center gap-(--space-3)">{children}</div>
    </main>
  );
}
