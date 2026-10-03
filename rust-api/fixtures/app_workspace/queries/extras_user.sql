-- queries/extras_user.sql
-- D-24 extras+user list queries + writes record: SQL shape per endpoint.
-- Base: apps/api/pi_dash/app/. Views: views/workspace/{label,state,estimate,
-- module,cycle,favorite,draft,quick_link,sticky,home,user_preference,
-- recent_visit,base,user,member,invite}.py; views/user/base.py; views/api.py;
-- views/timezone/base.py. Roles ADMIN=20 MEMBER=15 GUEST=5 (permissions/base.py:13-16).
--
-- R1 labels (label.py:22-30; perm WorkspaceViewerPermission :18; cache 2h :21):
--   workspace__slug=:slug AND project__member=:user AND is_active AND
--   project.archived_at IS NULL (:23-28). Serialized LabelSerializer many (:29).
SELECT labels.* FROM labels
  INNER JOIN projects ON (labels.project_id = projects.id)
  INNER JOIN project_members pm ON (pm.project_id = projects.id
    AND pm.member_id = :user AND pm.is_active AND projects.archived_at IS NULL)
WHERE labels.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug);
-- R2 states (state.py:21-41; perm WorkspaceEntityPermission :18; NO cache):
--   same 4 filters as R1 + is_triage=false (:22-28). In-memory regroup (:30-38):
--   per group, state.order = index/count (MUTATED, never saved; serializer :40
--   reads mutated values). No ORDER BY in SQL.
SELECT states.* FROM states
  INNER JOIN projects ON (states.project_id = projects.id)
  INNER JOIN project_members pm ON (pm.project_id = projects.id
    AND pm.member_id = :user AND pm.is_active AND projects.archived_at IS NULL)
WHERE states.workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND states.is_triage = false;
-- R3 estimates (estimate.py:22-33; perm Entity :18; cache 2h :21). TWO queries.
--   Q1 (:23-25) NOTE: NO member scoping — any project in workspace:
SELECT estimate_id FROM projects
WHERE workspace_id = (SELECT id FROM workspaces WHERE slug = :slug)
  AND estimate_id IS NOT NULL;
--   Q2 (:26-30) prefetch points + select workspace,project:
SELECT estimates.* FROM estimates
WHERE estimates.id IN (<Q1>) AND estimates.workspace_id = <ws>;
-- R4 modules (module.py:22-111; perm Viewer :20; NO member/project scoping — only
--   workspace__slug :24 + archived_at null :29). 6 Count annotates (:36-106),
--   each filter issue.archived_at null + is_draft=false + link deleted null,
--   distinct=True. Order BUG (:107): .order_by(self.kwargs.get("order_by",
--   "-created_at")) — kwargs are URL kwargs, never hold order_by, so ?order_by=
--   is IGNORED and order is ALWAYS -created_at. PORT.
SELECT modules.*,
  COUNT(DISTINCT issue_module.id) FILTER (WHERE issue.archived_at IS NULL
    AND issue.is_draft = false AND issue_module.deleted_at IS NULL) AS total_issues,
  COUNT(DISTINCT ...) FILTER (WHERE ... AND issue.state_group='completed') AS completed_issues,
  ... (cancelled/started/unstarted/backlog identical shape :60-106)
FROM modules WHERE modules.workspace_id = <ws> AND modules.archived_at IS NULL
ORDER BY modules.created_at DESC;
-- R5 cycles (cycle.py:22-104; perm Viewer :20; same no-scoping shape :24-28).
--   6 annotates (:29-99) WITHOUT distinct (vs R4 distinct=True) and counting
--   "issue_cycle__issue__state__group" for the 5 group counts (:42,:54,:66,:78,:90)
--   vs "issue_cycle" for total (:31) — PORT asymmetry. Extra issue.deleted_at
--   null filter (:36 etc, absent in R4). Same order_by-kwargs BUG :100, then
--   .distinct() :101.
SELECT DISTINCT cycles.*, COUNT(...) AS total_issues, ... FROM cycles
WHERE cycles.workspace_id = <ws> AND cycles.archived_at IS NULL
ORDER BY cycles.created_at DESC;
-- R6 favorites (favorite.py; all actions [ADMIN,MEMBER] :23,:37,:69,:78,:86).
--   GET (:26-33): user + workspace__slug + parent null, AND
--     (project null AND entity_type != 'page') OR (project set AND member active).
--     OPERATOR QUIRK (:27-28): & binds tighter than | so pages WITH a project
--     pass via the right branch — only project-less page favs excluded. PORT.
--   POST dup check (:44-49): workspace+user+entity_type+entity_identifier
--     .first(); hit -> 200 existing (:52-54). Else create (:57-64):
--     save(user_id, workspace, project_id=data.get("project_id", None) :62).
--     IntegrityError -> 400 {"error":"Favorite already exists"} (:66-67).
--   PATCH (:71-76): get(user, slug, pk) + partial save. DELETE (:80-82):
--     get + delete(soft=False) -> 204.
--   GROUP GET (:88-95): parent_id=favorite_id + project-null-or-member-active;
--     NOTE: no page exclusion here (vs top-level). PORT.
SELECT * FROM user_favorites uf LEFT JOIN projects p ON (uf.project_id = p.id)
WHERE uf.user_id = :user AND uf.workspace_id = <ws> AND uf.parent_id IS NULL
  AND ((uf.project_id IS NULL AND uf.entity_type <> 'page')
    OR (uf.project_id IS NOT NULL AND EXISTS(SELECT 1 FROM project_members pm
      WHERE pm.project_id = p.id AND pm.member_id = :user AND pm.is_active)));
-- R7 drafts get_queryset (draft.py:49-95): workspace__slug :51 + select
--   workspace,project,state,parent :52 + prefetch assignees,labels,
--   draft_issue_module__module :53 + cycle Subquery [:1] :55-60 + 3 Coalesce
--   ArrayAggs :62-93 (label_ids NOT NULL + label-link alive; assignee_ids NOT
--   NULL + member_project active + assignee-link alive; module_ids NOT NULL +
--   module unarchived + link alive) + distinct :95.
SELECT d.*,
  (SELECT dic.cycle_id FROM draft_issue_cycles dic
    WHERE dic.draft_issue_id = d.id AND dic.deleted_at IS NULL LIMIT 1) AS cycle_id,
  COALESCE(ARRAY_AGG(DISTINCT l.id) FILTER (WHERE l.id IS NOT NULL
    AND dli.deleted_at IS NULL), '{}') AS label_ids,
  COALESCE(ARRAY_AGG(DISTINCT a.id) FILTER (WHERE a.id IS NOT NULL
    AND pm.is_active AND dia.deleted_at IS NULL), '{}') AS assignee_ids,
  COALESCE(ARRAY_AGG(DISTINCT dim.module_id) FILTER (WHERE dim.module_id IS NOT NULL
    AND m.archived_at IS NULL AND dim.deleted_at IS NULL), '{}') AS module_ids
FROM draft_issues d WHERE d.workspace_id = <ws> GROUP BY d.id;
--   list (:99-109): + created_by=user + issue_filters(GET) + -created_at,
--     paginate default 1000 (gzip :97). create (:115-154): DraftIssueCreateSerializer
--     (ctx workspace_id, project_id?None :117-120) -> save -> re-fetch .values(
--     19 cols incl cycle_id/module_ids/label_ids/assignee_ids :127-149) -> 201.
--   partial_update (:162-184): get(slug,pk,created_by) else 404 "Issue not
--     found" :166; ctx project_id=data-or-issue :168, cycle_id=data-or-
--     "not_provided" :176 -> save -> 204. retrieve (:187-197): same fetch else
--     404 "The required object does not exist." :192 -> DetailSerializer 200.
--   destroy (:199-203): DraftIssue.objects.get(slug,pk).delete() HARD -> 204.
--   draft-to-issue (:206-312): fetch pk=draft_id :207; no project -> 400
--     "Project is required to create an issue." :211; IssueCreateSerializer(ctx
--     request, project_id, workspace_id, default_assignee_id :217-222) -> save;
--     issue_activity x3 (see tasks goldens); cycle link via CycleIssue.create
--     (created_by_id=draft creator, NOT requester :246-247) iff data.cycle_id
--     :240; ModuleIssue bulk_create batch 10 (draft creator ids :276-277) iff
--     data.module_ids :267; FileAsset re-point (issue_id, entity ISSUE_
--     DESCRIPTION, draft_issue_id=None :300-305); draft.delete() :308 -> 201.
-- R8 quick-links (quick_link.py; all [A,M,G] :23,:33,:45,:54,:60):
--   create: Workspace.get(slug) :25 + save(workspace_id, owner_id=user :29) 201.
--   partial_update: filter(pk,slug,owner).first() :35 else 404 {"detail":
--     "Quick link not found."} :43 (key "detail"); else partial save 200.
--   retrieve: get(pk,slug,owner) :48 except DoesNotExist -> 404 {"error":
--     "Quick link not found."} :52 (key "error" — INCONSISTENT with :43). PORT.
--   destroy: get + delete() :56-57 -> 204. list: filter(slug,owner) :62 -> 200.
-- R9 stickies (sticky.py): get_queryset filter(slug)+filter(owner_id=user :26)
--   + select workspace,owner + distinct (:21-29). create [A,M,G] :31-38:
--   get(slug) + save(workspace_id, owner_id) 201 (created_by auto-set by
--   BaseModel.save from request user — creator guard reads created_by while
--   list scopes owner_id; diverge only if ever created for another user).
--   list [A,M,G] :41-52: order -sort_order :43; ?query= filters
--   description_stripped icontains :44-45; paginate default_per_page=20 :51.
--   partial_update/destroy creator-only (roles=[] model=Sticky :54,:58) via
--   super() :56,:60. NOTE: retrieve NOT overridden -> ModelViewSet default,
--   IsAuthenticated only, NO workspace/role check (routed W44). PORT.
-- R10 home prefs (home.py): GET [A,M,G] :24-65: Workspace.get :25; existing
--   filter(user, ws) :27; keys = HomeWidgetKeys minus quick_tutorial,
--   new_at_pi_dash :31-35 (db/models/workspace.py:439-444) i.e. [quick_links,
--   recents, my_stickies]; LOOP BUG (:39-58): bulk_create INSIDE the per-key
--   loop over the GROWING list + ignore_conflicts, values_list re-queried per
--   key (N+1); effective rows sort_order 1000-counter (999,998,...) via
--   first-insert-wins. PORT. Response values(key,is_enabled,config,sort_order)
--   :60-64. PATCH :68-79: filter(key,slug,user).first() :69 else 400 {"detail":
--   "Preference not found"} :79 (400 + "detail", not 404). PORT.
-- R11 sidebar prefs (user_preference.py): GET [A,M,G] :26-79: same autocreate
--   loop shape (:35-61); keys = all 7 UserPreferenceKeys (:33;
--   db/models/workspace.py:482-489: views,active_cycles,analytics,drafts,
--   your_work,archives,stickies); sort 65535+i*10000 :45; is_pinned iff
--   drafts/your_work/stickies :46-55. Response {key:{is_pinned,sort_order}}
--   ordered sort_order :63-79. PATCH [A,M,G] :82-101: for data in request.data
--   (:83): pop key else skip (:84-86); filter(key,slug).first() (:88) — NO
--   user filter (vs R10 :69): member can rewrite ANOTHER user's row. PORT BUG.
--   Sets is_pinned/sort_order if present, save(update_fields both :99).
--   Always 200 {"message":"Successfully updated"} :101.
-- R12 recent visits (recent_visit.py list [A,M,G] :25-36): filter(slug,user)
--   :26; ?entity_name= narrows :28-31; then HARD filter entity_name IN
--   [issue,page,project] :33 (a non-listed ?entity_name= yields []); [:20]
--   slice, model ordering -created_at :35.
-- R13 user account (views/user/base.py):
--   retrieve/settings/Profile.get: serializer-only reads (no extra SQL beyond
--   request.user / Profile.get(user) :427). instance_admin :87-90:
--   Instance.first + InstanceAdmin.exists(instance,user) -> {is_instance_admin}.
--   deactivate :252-356: InstanceAdmin.exists(user) -> 400 "You cannot
--   deactivate your account since you are an instance admin" :257-261;
--   ProjectMember+WorkspaceMember active annotated other_admin_exists
--   (Count Case role=20 active NOT me :267-275,:288-296) + total_members;
--   loop: admin-elsewhere-or-solo -> collect else 400 "You cannot deactivate
--   account as you are the only admin in some projects|workspaces." (:283,
--   :304); bulk_update is_active batch 100 (:308,:310); delete invites by
--   email :313 + sessions by user :316; Profile reset (last_workspace_id=None,
--   tour/onboard False, onboarding_step all False :322-330; update_fields
--   +updated_at :331-339); user autoset-pw uuid4hex :342-343, is_active=False,
--   last_logout ip/time :346-348, save; user_deactivation_email.delay(host,
--   user.id) :352; logout; 204.
--   activity :393-403: IssueActivity filter(actor) select actor,workspace,
--   issue,project, order ?order_by=|-created_at, paginate. accounts get
--   :407-415: pk? get(pk,user) :409 : filter(user) :413; delete :417-420 get
--   + delete 204. onboard :374-381 / tour :385-389: Profile.get(user) + single
--   flag save(update_fields flag+updated_at) -> {"message":"Updated
--   successfully"}. profile patch :431-462: settings validated via
--   ee.user_settings else 400 {"settings":[msg]} :444; select_for_update get
--   :455 + serializer + merge_settings + save :456-461.
--   session (AllowAny :360): authed ? {is_authenticated True,user:me} : {False}
--   (:362-370; extra User.get :364).
-- R14 api tokens (views/api.py): POST :21-39 label=data-or-uuid4hex :22,
--   desc "", expired None; user_type=1-if-bot :27; create(label,desc,user,
--   user_type,expired) :29-35 -> 201 full serializer (token visible once :38).
--   GET :41-49: list filter(user,is_service=False) ReadSerializer many :43;
--   detail get(user,pk) :47 NOTE no is_service filter (vs DELETE/PATCH). PORT.
--   DELETE :51-54 get(user,pk,is_service=False) + delete 204. PATCH :56-72
--   filter(user,pk,is_service=False).first() :63-65 else BARE 404 empty body
--   :67; partial save 200/400.
-- R15 timezone (views/timezone/base.py:29-215): NO DB. Static 147-row table
--   :30-178 (dupes: Caracas x2 :68/:70 same America/Caracas; Lagos x2 :100/:102;
--   Karachi x2 :124/:125; Kolkata x4 :130-133 — PORT duplicates in output);
--   per-row offset/utc_offset/gmt_offset/value/label :184-204; unknown tz
--   skipped :205-206; sort (offset,label) :209; strip offset :212-213;
--   200 {"timezones":[...]} :215. AllowAny :24 + AuthenticationThrottle :26 +
--   cache_page 2h :28.
-- R16 workspace/user-scoped reads (views/workspace/base.py,user.py):
--   WorkSpaceViewSet.get_queryset :65-81: filter member-active :76-79 +
--   total_members Subquery (non-bot active :66-71) + order name :75; search
--   name, filter owner (:60-61). UserWorkSpacesEndpoint.get :209-240: fields?
--   :210; role+total_members annotates (:211-220) + prefetch member-me :223-228
--   + distinct :231 + serializer fields-or-all many :234-238. slug-check
--   :244-254: slug? else 400 "Workspace Slug is required" :249; exists OR in
--   RESTRICTED_WORKSPACE_SLUGS (utils/constants.py:5) -> {"status":bool} :254.
--   dashboard :262-348: 9 subqueries (activities 3mo Cast date :264-274;
--   completed by month param default 1 :276-290; assigned/pending/completed
--   counts :292-302; due-week isocalendar :304-309; state_distribution
--   :311-317; overdue :319-325; upcoming :327-333). themes :356-365: filter
--   slug :357; create get(slug)+save(workspace,actor) 201/400.
--   export :379-420: date? else 400 "Date is required" :381; IssueActivity
--   ~field in [comment,vote,reaction,draft] + slug + date + member-active +
--   actor=user_id, select_related, [:10000] :383-390; CSV QUOTE_ALL sanitized
--   rows, header 9 cols :392-417; text/csv attachment
--   "workspace-user-activity.csv" :418-419.
--   last-visited :69-96: User.get :71 then user.last_workspace_id :73 — FIELD
--   LIVES ON Profile (db/models/user.py:236 in class Profile :200), NOT User
--   (:56) -> AttributeError -> 500 {"error":"Something went wrong please try
--     again later"} (views/base.py:244-248). PINNED BUG — PORT. (None branch
--   :75-79 and workspace+project serialization :81-96 unreachable.)
--   profile-issues :136-250: triple-OR id-subquery (assignee|creator|
--   subscriber :140-148) + member-active; filterset+legacy filters; annotates
--   cycle Subquery + link/attachment/sub counts (:105-134); order/group/sub-
--   group incl 400 "Group by and sub group by cannot have same parameters"
--   :179. user-properties :253-278: get_or_create(user,ws) PATCH :259 / GET
--   :273. profile :281-368: User.get :283; requester WorkspaceMember.get
--   active :285-287; projects+4 counts iff requester role>=15 :289-351 else []
--   (user_data always); 7 user_data fields :356-365. activity :374-394:
--   ~field-in-4 + slug + member-active + unarchived + actor, ?project= multi
--   :375-387, paginate. stats :397-521: state/priority dists, created/
--   assigned/pending/completed/subscribed counts, present/upcoming cycles.
--   graphs: activity 6mo :524-538; completed week%4 :541-559.