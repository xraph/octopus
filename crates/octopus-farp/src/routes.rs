//! Per-service FARP routes in the gateway router.
//!
//! Every route FARP adds for a service carries the id `farp:<service>` and
//! origin [`RouteOrigin::Farp`]. A registration replaces all of them at once,
//! and deregistration removes them, without touching routes from the config
//! file or the admin API. Those declared routes also shadow a FARP route on the
//! same host and path, so they keep winning across re-registrations.

use crate::binding::{expand_gateway_binding, GatewayBinding};
use http::Method;
use octopus_router::{RouteBuilder, RouteOrigin, Router};
use tracing::{debug, warn};

/// The route id FARP gives every route of `service`: `farp:<service>`.
pub fn farp_route_id(service: &str) -> String {
    format!("farp:{service}")
}

/// The HTTP method a FARP route table or OpenAPI document names, if FARP
/// routes it. Anything else (`parameters`, `TRACE`, a typo) is skipped.
pub(crate) fn http_method(name: &str) -> Option<Method> {
    match name.to_uppercase().as_str() {
        "GET" => Some(Method::GET),
        "POST" => Some(Method::POST),
        "PUT" => Some(Method::PUT),
        "DELETE" => Some(Method::DELETE),
        "PATCH" => Some(Method::PATCH),
        "HEAD" => Some(Method::HEAD),
        "OPTIONS" => Some(Method::OPTIONS),
        _ => None,
    }
}

/// A FARP route builder for `service`: path `/{lowercase service}{path}`,
/// that prefix stripped before proxying, priority 100, and the upstream named
/// by the service.
pub(crate) fn service_route_builder(service: &str, path: &str, method: Method) -> RouteBuilder {
    let prefix = format!("/{}", service.to_lowercase());
    RouteBuilder::new()
        .method(method)
        .path(format!("{prefix}{path}"))
        .upstream_name(service)
        .strip_prefix(prefix)
        .priority(100)
}

/// Replace every FARP route of `service` with the routes `builders` describe.
///
/// Each builder is tagged with [`farp_route_id`] and origin
/// [`RouteOrigin::Farp`], then expanded over the `binding`'s hosts (see
/// [`expand_gateway_binding`]). All of them are built before the router
/// changes. The service's old FARP routes (that id and origin) are then
/// removed and the new ones inserted, so routes of other services, and
/// declared routes on the same paths or even with the same id, stay where they
/// are. Returns the number of routes inserted.
pub fn replace_service_routes(
    router: &Router,
    service: &str,
    builders: Vec<RouteBuilder>,
    binding: Option<&GatewayBinding>,
    route_has_auth: bool,
) -> usize {
    let id = farp_route_id(service);
    let mut routes = Vec::with_capacity(builders.len());
    for builder in builders {
        let builder = builder.id(Some(&id)).origin(RouteOrigin::Farp);
        for builder in expand_gateway_binding(builder, binding, route_has_auth) {
            match builder.build() {
                Ok(route) => routes.push(route),
                Err(e) => warn!(service = %service, error = %e, "Skipping invalid FARP route"),
            }
        }
    }

    let removed = router.remove_routes_by_id_and_origin(&id, RouteOrigin::Farp);
    let mut inserted = 0;
    for route in routes {
        let (method, path) = (route.method.clone(), route.path.clone());
        match router.add_route(route) {
            Ok(()) => inserted += 1,
            Err(e) => warn!(
                service = %service,
                method = %method,
                path = %path,
                error = %e,
                "Failed to register FARP route"
            ),
        }
    }
    debug!(service = %service, removed, inserted, "FARP routes replaced");
    inserted
}

/// Remove every FARP route of `service`. Returns the number removed.
///
/// Only routes with id [`farp_route_id`] and origin [`RouteOrigin::Farp`]
/// go. Routes of other services and declared routes on the same paths or
/// with the same id stay.
pub fn remove_service_routes(router: &Router, service: &str) -> usize {
    let removed = router.remove_routes_by_id_and_origin(&farp_route_id(service), RouteOrigin::Farp);
    debug!(service = %service, removed, "FARP routes removed");
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::GatewayBinding;
    use http::Method;
    use octopus_router::{HostMatch, RouteBuilder, RouteOrigin, Router};

    fn svc_route(path: &str) -> RouteBuilder {
        RouteBuilder::new()
            .method(Method::GET)
            .path(path)
            .upstream_name("svc")
            .priority(100)
    }

    fn a_dev() -> GatewayBinding {
        GatewayBinding::for_hostnames(&["a.dev".into()])
    }

    #[test]
    fn farp_route_id_prefixes_the_service() {
        assert_eq!(farp_route_id("billing"), "farp:billing");
    }

    #[test]
    fn replace_service_routes_swaps_old_routes() {
        let router = Router::new();
        let binding = a_dev();
        let added = replace_service_routes(
            &router,
            "svc",
            vec![svc_route("/svc/a")],
            Some(&binding),
            false,
        );
        assert_eq!(added, 1);
        let added = replace_service_routes(
            &router,
            "svc",
            vec![svc_route("/svc/b")],
            Some(&binding),
            false,
        );
        assert_eq!(added, 1);

        assert!(router.match_route("a.dev", &Method::GET, "/svc/a").is_err());
        let m = router.match_route("a.dev", &Method::GET, "/svc/b").unwrap();
        assert_eq!(m.route.origin, RouteOrigin::Farp);
        assert_eq!(m.route.id.as_deref(), Some("farp:svc"));
        assert_eq!(router.total_route_count(), 1);
    }

    #[test]
    fn replace_service_routes_leaves_other_services_alone() {
        let router = Router::new();
        replace_service_routes(&router, "one", vec![svc_route("/one/a")], None, false);
        replace_service_routes(&router, "two", vec![svc_route("/two/a")], None, false);
        replace_service_routes(&router, "one", vec![], None, false);

        assert!(router.match_route("", &Method::GET, "/one/a").is_err());
        assert!(router.match_route("", &Method::GET, "/two/a").is_ok());
    }

    #[test]
    fn a_declared_route_with_a_farp_id_survives_replace_and_remove() {
        let router = Router::new();
        router
            .add_route(
                RouteBuilder::new()
                    .id(Some("farp:svc"))
                    .origin(RouteOrigin::Config)
                    .method(Method::GET)
                    .path("/manual")
                    .upstream_name("lp-manual")
                    .build()
                    .unwrap(),
            )
            .unwrap();

        replace_service_routes(&router, "svc", vec![svc_route("/svc/a")], None, false);
        replace_service_routes(&router, "svc", vec![svc_route("/svc/b")], None, false);
        assert!(router.match_route("", &Method::GET, "/manual").is_ok());
        assert!(router.match_route("", &Method::GET, "/svc/a").is_err());

        assert_eq!(remove_service_routes(&router, "svc"), 1);
        let left = router.routes_by_id("farp:svc");
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].origin, RouteOrigin::Config);
    }

    #[test]
    fn http_method_knows_the_routed_methods_only() {
        assert_eq!(http_method("get"), Some(Method::GET));
        assert_eq!(http_method("OPTIONS"), Some(Method::OPTIONS));
        assert_eq!(http_method("parameters"), None);
        assert_eq!(http_method("TRACE"), None);
    }

    #[test]
    fn remove_service_routes_keeps_manual_routes() {
        let router = Router::new();
        let binding = a_dev();
        replace_service_routes(
            &router,
            "svc",
            vec![svc_route("/svc/a")],
            Some(&binding),
            false,
        );
        router
            .add_route(
                RouteBuilder::new()
                    .id(Some("lpr-1"))
                    .origin(RouteOrigin::Admin)
                    .method(Method::GET)
                    .path("/svc/a")
                    .host(HostMatch::Exact("a.dev".into()))
                    .upstream_name("lp-manual")
                    .build()
                    .unwrap(),
            )
            .unwrap();

        assert_eq!(remove_service_routes(&router, "svc"), 1);

        let m = router.match_route("a.dev", &Method::GET, "/svc/a").unwrap();
        assert_eq!(m.route.upstream_name, "lp-manual");
        assert_eq!(router.routes_by_id("lpr-1").len(), 1);
        assert_eq!(router.total_route_count(), 1);
    }
}
