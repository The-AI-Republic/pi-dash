//! Relation row-id reads for the relate/unrelate activity legs.
//!
//! The MCP tools emit per-target activity whose `model_id` is the
//! relation row id (`pi_dash_cloud/mcp/tools.py:686,753`), but the
//! operations report identifiers. This query returns the live row ids
//! so the caller can name the row the operation just wrote (relate,
//! post-op head) or is about to delete (unrelate, pre-op walk).
//! (Moved from the MCP read tools by PIDASHCONV-1661; Rule 5: queries
//! on OSS tables live here.)

use sqlx::PgPool;
use uuid::Uuid;

use super::{DbError, Error};

/// Live row ids for one stored pair, newest-first (`Meta.ordering`
/// `-created_at`, like the MCP queryset's `.first()`): the relate
/// created-row lookup takes the head; the unrelate claim walk takes the
/// head outside its claimed set, once per target occurrence, so
/// duplicate targets claim successive rows.
///
/// `stored_issue_id` / `stored_related_id` are the storage-oriented
/// endpoints (after the swap mapping); `canonical_type` is the stored
/// `relation_type`; `symmetric` adds the mirrored orientation for
/// symmetric types (`tools.py:726-732`).
pub async fn fetch_live_relation_ids(
    pool: &PgPool,
    stored_issue_id: Uuid,
    stored_related_id: Uuid,
    canonical_type: &str,
    symmetric: bool,
) -> Result<Vec<Uuid>, Error> {
    let sql = if symmetric {
        "SELECT id FROM issue_relations \
         WHERE ((issue_id = $1 AND related_issue_id = $2) \
            OR (issue_id = $2 AND related_issue_id = $1)) \
           AND relation_type = $3 AND deleted_at IS NULL \
         ORDER BY created_at DESC"
    } else {
        "SELECT id FROM issue_relations \
         WHERE issue_id = $1 AND related_issue_id = $2 \
           AND relation_type = $3 AND deleted_at IS NULL \
         ORDER BY created_at DESC"
    };
    let ids = sqlx::query_scalar(sql)
        .bind(stored_issue_id)
        .bind(stored_related_id)
        .bind(canonical_type)
        .fetch_all(pool)
        .await
        .map_err(|error| DbError::new("live-relation-ids", error))?;
    Ok(ids)
}

#[cfg(test)]
mod live {
    use super::*;
    use crate::read::test_support::{pool, seed_world};

    async fn seed_issue(
        pool: &PgPool,
        world: &crate::read::test_support::World,
        sequence_id: i32,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issues (id, created_at, updated_at, name, description_json, priority, \
             sequence_id, project_id, state_id, workspace_id, description_html, sort_order, \
             is_draft, git_work_branch, workpad, complexity_score) \
             VALUES ($1, now(), now(), 'Other', '{}', 'none', $2, $3, $4, $5, '', 65536, \
             false, '', '', 0)",
        )
        .bind(id)
        .bind(sequence_id)
        .bind(world.project_id)
        .bind(world.state_id)
        .bind(world.workspace_id)
        .execute(pool)
        .await
        .expect("seed issue");
        id
    }

    async fn seed_relation(
        pool: &PgPool,
        world: &crate::read::test_support::World,
        issue_id: Uuid,
        related_id: Uuid,
        relation_type: &str,
        created_secs_ago: i32,
    ) -> Uuid {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO issue_relations \
             (id, relation_type, issue_id, project_id, related_issue_id, workspace_id, \
             created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, now() - make_interval(secs => $7), now())",
        )
        .bind(id)
        .bind(relation_type)
        .bind(issue_id)
        .bind(world.project_id)
        .bind(related_id)
        .bind(world.workspace_id)
        .bind(created_secs_ago)
        .execute(pool)
        .await
        .expect("seed relation");
        id
    }

    #[tokio::test]
    async fn live_relation_ids_orientation_and_newest_first() {
        let Some(db) = pool().await else { return };
        let world = seed_world(&db).await;
        // Live rows are unique per oriented pair
        // (`issue_relation_unique_issue_related_issue_when_deleted_at_null`),
        // so multi-row results only span orientations (symmetric query).
        let other = seed_issue(&db, &world, 102).await;
        let other2 = seed_issue(&db, &world, 103).await;
        let stored = seed_relation(&db, &world, world.issue_id, other, "blocking", 60).await;
        let mirrored = seed_relation(&db, &world, other, world.issue_id, "blocking", 30).await;
        let dup = seed_relation(&db, &world, world.issue_id, other2, "duplicate", 0).await;
        // Asymmetric: exact orientation only, type-filtered.
        let ids = fetch_live_relation_ids(&db, world.issue_id, other, "blocking", false)
            .await
            .expect("asymmetric");
        assert_eq!(ids, vec![stored]);
        let ids = fetch_live_relation_ids(&db, world.issue_id, other2, "duplicate", false)
            .await
            .expect("other pair");
        assert_eq!(ids, vec![dup]);
        // The stored row's type filters: same pair, other type → nothing.
        let ids = fetch_live_relation_ids(&db, world.issue_id, other, "duplicate", false)
            .await
            .expect("type mismatch");
        assert!(ids.is_empty());
        // Symmetric: both orientations, newest first across them.
        let ids = fetch_live_relation_ids(&db, world.issue_id, other, "blocking", true)
            .await
            .expect("symmetric");
        assert_eq!(ids, vec![mirrored, stored]);
        // Unknown type: nothing.
        let ids = fetch_live_relation_ids(&db, world.issue_id, other, "relates", true)
            .await
            .expect("unknown type");
        assert!(ids.is_empty());
        // Deleted rows stay out.
        sqlx::query("UPDATE issue_relations SET deleted_at = now() WHERE id = $1")
            .bind(stored)
            .execute(&db)
            .await
            .expect("soft-delete");
        let ids = fetch_live_relation_ids(&db, world.issue_id, other, "blocking", false)
            .await
            .expect("after delete");
        assert!(ids.is_empty());
    }
}
