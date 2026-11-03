-- queries/assets.sql
-- EntityAssetEndpoint / AssetRestoreEndpoint / EntityBulkAssetEndpoint
-- views/asset.py:26-226. GET is AllowAny; all other methods IsAuthenticated
-- (get_permissions views/asset.py:27-32; restore/bulk inherit BaseAPIView
-- default IsAuthenticated). Ported-from: 01a93e17216faea7bfc156b0f864cbbe420d1c52.

-- A1 get (:34-66): board=DeployBoard.objects.filter(anchor).first() — NO
-- entity_name scoping (:36) — PORT. No board ->
-- {"error": "Requested resource could not be found."} 404 (:39-42, trailing
-- period). asset=FileAsset.objects.get(workspace_id, pk,
-- entity_type__in=[ISSUE_DESCRIPTION, COMMENT_DESCRIPTION]) (:45-52).
-- NOT is_uploaded -> {"error": "The requested asset could not be found."} 404
-- (:55-59). Success: S3Storage presigned URL + HttpResponseRedirect — NOT a
-- DRF Response (:62-66) — PORT (302 + Location, no JSON body).
SELECT ... FROM "file_assets"
  WHERE ("file_assets"."deleted_at" IS NULL
    AND "file_assets"."workspace_id" = %(workspace_id)s
    AND "file_assets"."id" = %(pk)s
    AND "file_assets"."entity_type" IN ('ISSUE_DESCRIPTION', 'COMMENT_DESCRIPTION'));

-- A2 post (:68-133): board .first() unscoped (:70); no board ->
-- {"error": "Project is not published"} 404 (:72-73). Inputs: name,
-- type default "image/jpeg" (:77), size=int(data.get("size",
-- settings.FILE_SIZE_LIMIT)) — unguarded int(), non-numeric -> ValueError 500;
-- NO max check (default only) (:78) — PORT. entity_type default "" (:79),
-- entity_identifier (:80). entity_type not in EntityTypeContext.values ->
-- {"error": "Invalid entity type.", "status": False} 400 (:83-87, extra
-- status:false + trailing period). type not in [jpeg,png,webp,jpg,gif] ->
-- {"error": "Invalid file type. Only JPEG, PNG, WebP, JPG and GIF files are allowed.",
-- "status": False} 400 (:90-104). asset_key=f"{workspace_id}/{uuid4hex}-{name}"
-- (:107). INSERT FileAsset(attributes={name,type,size}, asset=asset_key,
-- size, workspace, created_by=user, entity_type, project_id,
-- comment_id=entity_identifier) — entity_identifier stuffed into comment_id
-- UNCONDITIONALLY, even for non-comment types (:110-119) — PORT. 200
-- {upload_data: presigned_post, asset_id: str(id), asset_url} (:126-133).
INSERT INTO "file_assets" ("id", "created_at", "updated_at", "created_by_id",
  "attributes", "asset", "size", "workspace_id", "entity_type", "project_id",
  "comment_id", "is_uploaded", ...)
  VALUES (..., %(attributes_json)s, %(asset_key)s, %(size)s, ...,
    %(entity_type)s, %(project_id)s, %(entity_identifier)s, false, ...);

-- A3 patch (:135-154): board .first() unscoped (:137); no board 404 (:139-140).
-- asset=FileAsset.objects.get(id=pk, workspace) (:143); is_uploaded=True (:145);
-- if not storage_metadata: get_asset_object_metadata.delay(str(asset.id)) (:147-148);
-- attributes=request.data.get("attributes", current) (:151);
-- save(update_fields=["attributes","is_uploaded"]) (:153); 204 (:154).

-- A4 delete (:156-169): board filter anchor+entity_name="project" (:158); no
-- board 404 (:160-161). asset get id+workspace+project_id (:163); soft-delete
-- is_deleted=True, deleted_at=now (:165-166); save (:168); 204 (:169).
UPDATE "file_assets" SET "is_deleted" = true, "deleted_at" = %(now)s
  WHERE "id" = %(pk)s;

-- A5 restore (:175-187): board anchor+entity_name (:177); no board 404 (:179-180).
-- asset=FileAsset.all_objects.get(id, workspace) — UNFILTERED manager sees
-- soft-deleted rows (:183) — PORT the manager choice. is_deleted=False,
-- deleted_at=None (:184-185); save (:186); 204 (:187).
UPDATE "file_assets" SET "is_deleted" = false, "deleted_at" NULL
  WHERE "id" = %(pk)s;

-- A6 bulk (:193-226): board anchor+entity_name (:195); no board 404 (:197-198).
-- asset_ids=data.get("asset_ids", []) (:200); empty ->
-- {"error": "No asset ids provided."} 400 (:203-204). assets=filter(id__in,
-- workspace, project_id) (:207-211); first() None ->
-- {"error": "The requested asset could not be found."} 404 (:216-220).
-- if asset.entity_type == COMMENT_DESCRIPTION: assets.update(comment_id=entity_id)
-- (:223-225) — ALL other entity types SILENTLY NO-OP yet still 204 (:226) — PORT.
UPDATE "file_assets" SET "comment_id" = %(entity_id)s
  WHERE ("id" IN (...) AND "workspace_id" = %(workspace_id)s
    AND "project_id" = %(project_id)s);
-- (executes ONLY when the first asset's entity_type == 'COMMENT_DESCRIPTION')
