-- queries/comments_reactions_votes.sql
-- Social viewsets views/issue.py:214-591. Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.

-- C1 comment list queryset IssueCommentPublicViewSet.get_queryset (:228-255):
-- DeployBoard.objects.get(anchor, entity_name="project") (:230);
-- is_comments_enabled gate; super().get_queryset() = model.objects.all() +
--   filter_queryset (filterset_fields=["issue__id","workspace__id"] :218,
--   SearchFilter, no search_fields) restricted to:
--   workspace_id=board.workspace_id AND issue_id=<issue_id> AND access="EXTERNAL"
--   select_related(project, workspace, issue)
--   annotate(is_member=Exists(ProjectMember: workspace, project,
--     member_id=request.user.id, is_active=True)) (:241-250)
--   NOTE: request.user.id is evaluated even for AllowAny anonymous list
--   (AnonymousUser.id=None -> member_id=None clause, silently False) — PORT.
--   .distinct() then filter_queryset(...) AGAIN, .order_by("created_at") (:252).
-- Disabled board -> IssueComment.objects.none() (:253);
-- DeployBoard.DoesNotExist -> .none() (:254-255) — NO 404 here.
SELECT "issue_comments".*, EXISTS(SELECT 1 FROM "project_members" U0
    WHERE (U0."deleted_at" IS NULL
      AND U0."workspace_id" = %(workspace_id)s AND U0."project_id" = %(project_id)s
      AND U0."member_id" = %(user_id_or_None)s AND U0."is_active" = true)
  ) AS "is_member"
  FROM "issue_comments"
  WHERE ("issue_comments"."deleted_at" IS NULL
    AND "issue_comments"."workspace_id" = %(workspace_id)s
    AND "issue_comments"."issue_id" = %(issue_id)s
    AND "issue_comments"."access" = 'EXTERNAL')
  ORDER BY "issue_comments"."created_at" ASC;

-- C2 comment create: IssueCommentSerializer(data).save(project_id, issue_id,
-- actor=request.user, access="EXTERNAL") (:266-273); 201 (:293); disabled ->
-- {"error": "Comments are not enabled for this project"} 400 (:260-264;
-- identical :299-303 partial_update, :323-327 destroy).

-- C3 comment partial_update/destroy scoping: IssueComment.objects.get(pk=pk,
-- actor=request.user) (:304, :328) — owner-only; DoesNotExist bubbles to
-- handle_exception envelope (no inline 404). destroy emits activity BEFORE
-- delete (:329-338) with pre-delete serializer snapshot as current_instance.

-- C4 IssueReactionPublicViewSet.get_queryset (:346-364): BUG-PORT — board lookup
-- DeployBoard.objects.get(workspace__slug=<slug kwarg>, project_id=<project_id
-- kwarg>) (:348-351) but routes supply only anchor/issue_id (urls/issue.py:32-41),
-- so both kwargs are None and the lookup ALWAYS misses -> .none(). Intended
-- (per create :367) anchor+entity_name scoping. List filter would have been
-- workspace__slug=<None> AND project_id=<None> AND issue_id, order
-- -created_at, distinct (:353-361). No get_permissions override -> list/retrieve
-- require IsAuthenticated (class default views/base.py:48) — PORT (B14).

-- C5 CommentReactionPublicViewSet.get_queryset (:434-449): anchor+entity_name
-- board lookup (:436); filter workspace_id AND project_id AND
-- comment_id=<comment_id>; order -created_at; distinct. Same IsAuthenticated
-- default for list — PORT.

-- C6 IssueVotePublicViewSet.get_queryset (:526-541): BUG-PORT — board lookup
-- DeployBoard.objects.get(workspace__slug=<anchor VALUE>, entity_name) (:528-530):
-- the anchor string is passed to the workspace__slug field. Always misses
-- (unless a workspace slug literally equals an anchor) -> .none(). Intended
-- anchor=<anchor> per create (:544). Would-be filter: issue_id AND workspace_id
-- AND project_id (:532-537, no ordering, no distinct).
