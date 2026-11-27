#![forbid(unsafe_code)]

//! Axum mirror of `ReadReplicaRoutingMiddleware`
//! (`apps/api/pi_dash/middleware/db_routing.py:1-164`, fixture F37-11).
//!
//! Rule map (Python line → Rust item):
//!
//! | Python | Rust |
//! |---|
//! | `READ_ONLY_METHODS` (:38), non-read → primary in `__call__` (:56-59) | [`pidash_db::pool::is_write_method`] (kernel, reused) + the write branch of [`ReplicaRoutingService::call`] |
//! | `process_view` resolve + set (:88-93), `None` → primary (:110-111), `bool(attr)` (:113) | [`resolve_use_replica_attr`] + [`should_use_replica`] + the read branch of `call` |
//! | attr lookup order func → `view_class` → `cls` (:115-144) | [`resolve_use_replica_attr`] |
//! | `try/finally` clear (:61-68), `process_exception` clear (:146-164) | [`pidash_db::pool::replica_scope`] exit (kernel, reused): the scope ends on `Ok` and on `Err` alike |
//! | `ReadReplicaControlMixin.use_read_replica = True` (`utils/core/mixins/view.py:24`) | [`CONTROL_MIXIN_USE_READ_REPLICA`] + [`ReplicaRoutingLayer::control_mixin`] |
//!
//! Deliberately not re-ported: `set` / `should` / `clear`
//! (`request_scope.py:28-76`), `db_for_read` / `db_for_write` /
//! `allow_migrate` (`dbrouters.py:30-75`), and the primary default are
//! already exact in `pidash_db::pool` (`replica_scope`,
//! `should_use_read_replica`, `route_for`) and
//! `pidash_db::context::RequestContext`; the tests below verify that
//! mirror row by row instead of forking it. Wiring this layer into the
//! live middleware stack is cutover work (D-37 gate), not this issue.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{Request, Response};
use pidash_db::pool::{is_write_method, replica_scope};
use tower::{Layer, Service};

/// Logger name for routing lines, matching the Python middleware logger
/// (`logging.getLogger("pi_dash.api")`, `db_routing.py:23`).
pub const ROUTING_LOGGER_TARGET: &str = "pi_dash.api";

/// Mirror of `ReadReplicaControlMixin.use_read_replica = True`
/// (`utils/core/mixins/view.py:24`): a view mixing the control mixin
/// without override routes its reads to the replica.
pub const CONTROL_MIXIN_USE_READ_REPLICA: bool = true;

/// A `use_read_replica` attribute value, mirroring Python's `bool(attr)`
/// coercion (`db_routing.py:113`).
///
/// Every real view sets a bool (the mixin types it `bool`); the `Int` /
/// `Text` variants pin the coercion the fixture names (falsy `0` / `''`,
/// truthy anything else) so the decision table replays literally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplicaAttr {
    Bool(bool),
    Int(i64),
    Text(String),
}

impl ReplicaAttr {
    /// Python `bool()` over the attr types the fixture names: `False` /
    /// `0` / `''` are falsy, everything else truthy.
    pub fn truthy(&self) -> bool {
        match self {
            Self::Bool(value) => *value,
            Self::Int(value) => *value != 0,
            Self::Text(value) => !value.is_empty(),
        }
    }
}

/// Port of `_get_use_replica_attribute` (`db_routing.py:115-144`): the
/// first non-`None` of the function attr (:128-130), the Django CBV
/// `view_class` attr (:133-136), and the DRF `cls` attr (:139-142).
/// A `None` view func (:124-125) and an all-miss (:144) both yield `None`.
pub fn resolve_use_replica_attr(
    func: Option<ReplicaAttr>,
    view_class: Option<ReplicaAttr>,
    cls: Option<ReplicaAttr>,
) -> Option<ReplicaAttr> {
    func.or(view_class).or(cls)
}

/// Port of `_should_use_read_replica` (`db_routing.py:99-113`): a
/// missing/`None` attr routes to the primary (safe default, :110-111),
/// otherwise `bool(attr)` decides (:113).
pub fn should_use_replica(attr: Option<ReplicaAttr>) -> bool {
    attr.as_ref().is_some_and(|value| value.truthy())
}

/// Tower layer mirroring `ReadReplicaRoutingMiddleware`.
///
/// The layer carries one route's `use_read_replica` declaration (`None`
/// = the view sets no attribute, so reads stay on the primary). Per
/// request it reproduces `__call__` + `process_view`: non-read methods
/// run the inner service inside `replica_scope(false, …)` immediately
/// (:56-59); read methods resolve the attr and run inside
/// `replica_scope(decision, …)` (:88-93). Scope exit always clears —
/// the `try/finally` (:61-68) and `process_exception` (:146-164) halves
/// — on `Ok` and on `Err` alike.
#[derive(Debug, Clone)]
pub struct ReplicaRoutingLayer {
    use_read_replica: Option<ReplicaAttr>,
}

impl ReplicaRoutingLayer {
    /// A route whose view sets no `use_read_replica` attribute.
    pub fn new() -> Self {
        Self {
            use_read_replica: None,
        }
    }

    /// A route declaring `use_read_replica`.
    pub fn with_attr(attr: ReplicaAttr) -> Self {
        Self {
            use_read_replica: Some(attr),
        }
    }

    /// A route whose handler mixes `ReadReplicaControlMixin` without
    /// override (default `True`).
    pub fn control_mixin() -> Self {
        Self::with_attr(ReplicaAttr::Bool(CONTROL_MIXIN_USE_READ_REPLICA))
    }
}

impl Default for ReplicaRoutingLayer {
    /// No attribute: the primary default, as in `_should_use_read_replica`.
    fn default() -> Self {
        Self::new()
    }
}

impl<S> Layer<S> for ReplicaRoutingLayer {
    type Service = ReplicaRoutingService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ReplicaRoutingService {
            inner,
            use_read_replica: self.use_read_replica.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReplicaRoutingService<S> {
    inner: S,
    use_read_replica: Option<ReplicaAttr>,
}

impl<S> Service<Request<Body>> for ReplicaRoutingService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response<Body>, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let method = req.method().to_string();
        let path = req.uri().path().to_string();
        let attr = self.use_read_replica.clone();
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        Box::pin(async move {
            if is_write_method(&method) {
                tracing::debug!(
                    target: ROUTING_LOGGER_TARGET,
                    "Routing {method} {path} to primary database"
                );
                replica_scope(false, inner.call(req)).await
            } else {
                let use_replica = should_use_replica(attr);
                let db_type = if use_replica {
                    "read replica"
                } else {
                    "primary database"
                };
                tracing::debug!(
                    target: ROUTING_LOGGER_TARGET,
                    "Routing {method} {path} to {db_type}"
                );
                replica_scope(use_replica, inner.call(req)).await
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    use pidash_db::context::RequestContext;
    use pidash_db::pool::{route_for, should_use_read_replica, Route};
    use pidash_types::{UserId, WorkspaceId};
    use tower::ServiceExt;

    /// Fixture F37-11, pinned at the source: the pins below fail if the
    /// golden drifts, and the behavioral tests replay its rows.
    const GOLDEN: &str =
        include_str!("../../../../fixtures/ops/routing/decision_table.golden.json");

    fn ctx(replica: bool) -> RequestContext {
        RequestContext::new(WorkspaceId::from("ws-1"), Some(UserId::from("u-1")))
            .with_replica(replica)
    }

    fn true_attr() -> Option<ReplicaAttr> {
        Some(ReplicaAttr::Bool(true))
    }

    fn false_attr() -> Option<ReplicaAttr> {
        Some(ReplicaAttr::Bool(false))
    }

    #[test]
    fn fixture_f37_11_pins() {
        let golden: serde_json::Value = serde_json::from_str(GOLDEN).expect("valid golden");
        assert_eq!(golden["_fixture"], "F37-11");
        assert_eq!(
            golden["middleware"]["read_only_methods"],
            serde_json::json!(["GET", "HEAD", "OPTIONS"])
        );
        let rows = golden["decision_table"].as_array().expect("rows");
        assert_eq!(rows.len(), 4);
        let replica: Vec<bool> = rows
            .iter()
            .map(|row| row["replica"].as_bool().expect("bool"))
            .collect();
        // Non-read → primary; read + missing/None → primary; read +
        // truthy → replica; read + falsy → primary.
        assert_eq!(replica, vec![false, false, true, false]);
        assert!(golden["mixin"]["default"]
            .as_str()
            .expect("str")
            .contains("True"));
    }

    #[test]
    fn attr_lookup_order_is_function_then_view_class_then_cls() {
        let t = || true_attr();
        let f = || false_attr();
        // First non-None wins, whatever its value.
        assert_eq!(resolve_use_replica_attr(t(), f(), f()), t());
        assert_eq!(resolve_use_replica_attr(f(), t(), t()), f());
        assert_eq!(resolve_use_replica_attr(None, t(), f()), t());
        assert_eq!(resolve_use_replica_attr(None, f(), t()), f());
        assert_eq!(resolve_use_replica_attr(None, None, t()), t());
        assert_eq!(resolve_use_replica_attr(None, None, f()), f());
        // None view func / all-miss → None.
        assert_eq!(resolve_use_replica_attr(None, None, None), None);
    }

    #[test]
    fn missing_attr_routes_primary_and_bool_attr_decides() {
        assert!(!should_use_replica(None));
        assert!(should_use_replica(true_attr()));
        assert!(!should_use_replica(false_attr()));
    }

    #[test]
    fn non_bool_attrs_follow_python_truthiness() {
        assert!(!should_use_replica(Some(ReplicaAttr::Int(0))));
        assert!(should_use_replica(Some(ReplicaAttr::Int(1))));
        assert!(should_use_replica(Some(ReplicaAttr::Int(-1))));
        assert!(!should_use_replica(Some(ReplicaAttr::Text(String::new()))));
        // Non-empty strings are truthy in Python, even "0"/"False".
        assert!(should_use_replica(Some(ReplicaAttr::Text("0".to_owned()))));
        assert!(should_use_replica(Some(ReplicaAttr::Text(
            "False".to_owned()
        ))));
    }

    #[test]
    fn control_mixin_default_is_replica() {
        // Compile-time pin: flipping the const breaks the build, and
        // read_methods_resolve_the_attr pins it behaviorally too.
        const { assert!(CONTROL_MIXIN_USE_READ_REPLICA) };
    }

    async fn ambient_in_inner(method: &str, layer: ReplicaRoutingLayer) -> bool {
        let inner = tower::service_fn(|_req: Request<Body>| async move {
            Ok::<_, Infallible>(Response::new(Body::from(
                should_use_read_replica().to_string(),
            )))
        });
        let mut service = layer.layer(inner);
        let response = service
            .ready()
            .await
            .expect("ready")
            .call(
                Request::builder()
                    .method(method)
                    .uri("http://x/api/v1/issues/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let body = axum::body::to_bytes(response.into_body(), 16)
            .await
            .expect("body");
        assert!(!should_use_read_replica(), "scope cleared after call");
        body == "true"
    }

    #[tokio::test]
    async fn non_read_methods_use_primary_whatever_the_attr() {
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            for layer in [
                ReplicaRoutingLayer::new(),
                ReplicaRoutingLayer::with_attr(ReplicaAttr::Bool(true)),
                ReplicaRoutingLayer::control_mixin(),
            ] {
                assert!(
                    !ambient_in_inner(method, layer).await,
                    "{method} must stay primary"
                );
            }
        }
    }

    #[tokio::test]
    async fn read_methods_resolve_the_attr() {
        for method in ["GET", "HEAD", "OPTIONS"] {
            assert!(
                ambient_in_inner(method, ReplicaRoutingLayer::control_mixin()).await,
                "{method} + mixin default routes replica"
            );
            assert!(
                ambient_in_inner(
                    method,
                    ReplicaRoutingLayer::with_attr(ReplicaAttr::Bool(true))
                )
                .await,
                "{method} + True routes replica"
            );
            assert!(
                !ambient_in_inner(
                    method,
                    ReplicaRoutingLayer::with_attr(ReplicaAttr::Bool(false))
                )
                .await,
                "{method} + False stays primary"
            );
            assert!(
                !ambient_in_inner(method, ReplicaRoutingLayer::new()).await,
                "{method} + missing attr stays primary"
            );
        }
    }

    #[tokio::test]
    async fn sequential_requests_do_not_leak() {
        let inner = tower::service_fn(|_req: Request<Body>| async move {
            Ok::<_, Infallible>(Response::new(Body::from(
                should_use_read_replica().to_string(),
            )))
        });
        let mut service = ReplicaRoutingLayer::control_mixin().layer(inner);
        for (method, want) in [("GET", "true"), ("POST", "false"), ("GET", "true")] {
            let response = service
                .ready()
                .await
                .expect("ready")
                .call(
                    Request::builder()
                        .method(method)
                        .uri("http://x/api/v1/issues/")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            let body = axum::body::to_bytes(response.into_body(), 16)
                .await
                .expect("body");
            assert_eq!(body, want, "{method}");
        }
        assert!(!should_use_read_replica());
    }

    #[derive(Debug)]
    struct Boom;

    impl std::fmt::Display for Boom {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("boom")
        }
    }

    impl std::error::Error for Boom {}

    #[tokio::test]
    async fn inner_error_clears_and_propagates() {
        // The process_exception half: cleanup runs and the error keeps
        // propagating (the middleware returns None).
        let inner = tower::service_fn(|_req: Request<Body>| async move {
            assert!(should_use_read_replica(), "set before the view runs");
            Err::<Response<Body>, Boom>(Boom)
        });
        let mut service = ReplicaRoutingLayer::control_mixin().layer(inner);
        let err = service
            .ready()
            .await
            .expect("ready")
            .call(
                Request::builder()
                    .method("GET")
                    .uri("http://x/api/v1/issues/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect_err("propagates");
        assert_eq!(err.to_string(), "boom");
        assert!(!should_use_read_replica(), "cleared on the error path");
    }

    #[tokio::test]
    async fn layer_and_router_kernel_agree() {
        // The layer sets the ambient flag; route_for turns it into the
        // pool choice (db_for_read mirror). Replica configured.
        let inner = tower::service_fn(|_req: Request<Body>| async move {
            let route = route_for(false, &ctx(false), true);
            Ok::<_, Infallible>(Response::new(Body::from(match route {
                Route::Replica => "replica",
                Route::Primary => "primary",
            })))
        });
        for (layer, want) in [
            (ReplicaRoutingLayer::control_mixin(), "replica"),
            (ReplicaRoutingLayer::new(), "primary"),
        ] {
            let mut service = layer.layer(inner);
            let response = service
                .ready()
                .await
                .expect("ready")
                .call(
                    Request::builder()
                        .method("GET")
                        .uri("http://x/api/v1/issues/")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            let body = axum::body::to_bytes(response.into_body(), 16)
                .await
                .expect("body");
            assert_eq!(body, want);
        }
        // Writes stay primary even opted in; no replica means primary.
        assert_eq!(route_for(true, &ctx(true), true), Route::Primary);
        assert_eq!(route_for(false, &ctx(true), false), Route::Primary);
    }
}
