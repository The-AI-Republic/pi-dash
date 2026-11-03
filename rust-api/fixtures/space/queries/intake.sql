-- queries/intake.sql
-- IntakeIssuePublicViewSet views/intake.py:31-280. No get_permissions override:
-- ALL actions incl. public list require IsAuthenticated (base default) — PORT.
-- Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.

-- N1 get_queryset (:37-54): BUG-PORT — board lookup by slug/project_id kwargs
-- the routes never supply (urls/intake.py:15-29 give anchor/intake_id) (:38-41);
-- filter is Q(snoozed_till>=now OR snoozed_till NULL) AND project_id=<None>
-- AND workspace__slug=<None> AND intake_id=<intake_id>; select_related(issue,
-- workspace, project). list() bypasses get_queryset so list works; mixin
-- retrieve/update/destroy would break — PORT.

-- N2 list (:56-105): board=DeployBoard.objects.get(anchor, entity_name) (:57);
-- intake None -> {"error": "Intake is not enabled for this Project Board"} 400
-- (:58-62; identical :109-113 create, :177-181 partial_update, :238-242
-- retrieve, :260-264 destroy). Queryset:
-- Issue.objects.filter(issue_intake__intake_id, workspace_id, project_id)
--   .filter(**issue_filters(query_params,"GET")) (:64-71)
--   .annotate(bridge_id=F("issue_intake__id")) (:72)
--   .select_related(workspace, project, state, parent) (:73)
--   .prefetch_related(assignees, labels) (:74)
--   .order_by("issue_intake__snoozed_till", "issue_intake__status") (:75)
--   .annotate(sub_issues_count=Issue.issue_objects.filter(parent=OuterRef)...) (:76-81)
--   .annotate(link_count=IssueLink...) (:82-87)
--   .annotate(attachment_count=FileAsset...ISSUE_ATTACHMENT...) (:88-96)
--   .prefetch_related(Prefetch("issue_intake",
--     IntakeIssue.objects.only("status","duplicate_to","snoozed_till","source"))) (:97-102)
-- serializer IssueStateIntakeSerializer(many) (:104); 200 (:105). NO pagination.
SELECT "issues".*, "issue_intake"."id" AS "bridge_id",
  (SELECT COUNT(...) ...) AS "sub_issues_count",
  (SELECT COUNT(...) ...) AS "link_count",
  (SELECT COUNT(...) ...) AS "attachment_count"
  FROM "issues" INNER JOIN "intake_issues" "issue_intake"
    ON ("issues"."id" = "issue_intake"."issue_id")
  WHERE ("issues"."deleted_at" IS NULL
    AND "issue_intake"."intake_id" = %(intake_id)s
    AND "issues"."workspace_id" = %(workspace_id)s
    AND "issues"."project_id" = %(project_id)s AND <issue_filters Q>)
  ORDER BY "issue_intake"."snoozed_till" ASC, "issue_intake"."status" ASC;

-- N3 create (:107-173): name missing -> {"error": "Name is required"} 400 (:115-116);
-- priority not in [low,medium,high,urgent,none] (default "none") ->
-- {"error": "Invalid priority"} 400 (:119-126). Triage:
-- State.triage_objects.filter(project, workspace).first() else
-- State.objects.create(name="Triage", group=TRIAGE, project, workspace,
-- color="#4E5355", sequence=65000, default=False) (:129-142).
-- INSERT Issue(name, description_json or {}, description_html or "<p></p>",
--   priority or "low" — NOTE default "low" vs validated default "none", PORT;
--   project_id, state_id=triage) with NO workspace_id (:145-152) — PORT.
-- issue_activity type="issue.activity.created" (:155-163).
-- INSERT IntakeIssue(intake_id=<intake_id URL kwarg — NOT board.intake>,
--   project_id, issue, source=IN_APP) with NO workspace_id (:165-170) — PORT.
-- Response IssueStateIntakeSerializer(issue), 200 NOT 201 (:172-173) — PORT.
INSERT INTO "issues" ("id", "created_at", "updated_at", "name", "description_json",
  "description_html", "description_stripped", "priority", "project_id", "state_id",
  "sequence_id", "sort_order", "is_draft", ...)
  VALUES (..., %(name)s, COALESCE(%(description_json)s, '{}'),
    COALESCE(%(description_html)s, '<p></p>'), COALESCE(%(priority)s, 'low'), ...);
INSERT INTO "intake_issues" ("id", "created_at", "updated_at", "intake_id",
  "project_id", "issue_id", "source", "status", ...)
  VALUES (..., %(intake_id)s, %(project_id)s, %(issue_id)s, 'IN_APP', -2);

-- N4 partial_update (:175-234): intake None 400 (:177-181);
-- IntakeIssue.objects.get(pk, workspace_id, project_id, intake_id) (:183-188);
-- creator check str(created_by_id) != str(request.user.id) ->
-- {"error": "You cannot edit intake issues"} 400 (:190-194) — PORT wording
-- ("edit" here vs "delete" in destroy). issue_data=request.data.pop("issue")
-- (:197) — MUTATES request.data; KeyError bubbles if absent (no .get default
-- beyond False) — PORT. Subset to {name, description_html, description_json}
-- with per-key fallbacks (:205-209); IssueCreateSerializer(issue, data,
-- partial, ctx={project_id, allow_triage_state: True}) (:211-216);
-- activity type="issue.activity.updated", requested_data=the 3-key subset JSON,
-- current_instance=pre-save APP IssueSerializer snapshot (:219-231); save (:232);
-- 200 (:233); errors 400 (:234).

-- N5 retrieve (:236-256): intake None 400; same gets; APP IssueStateIntakeSerializer
-- (no bridge_id — only list/create carry bridge_id via annotation); 200.

-- N6 destroy (:258-280): intake None 400; same get; creator check ->
-- {"error": "You cannot delete intake issue"} 400 (:273-277) — PORT wording;
-- intake_issue.delete() (:279); 204 (:280). NOTE: deletes the INTAKE row only,
-- the Issue survives — PORT.
