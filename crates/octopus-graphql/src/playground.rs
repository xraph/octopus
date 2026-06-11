//! Self-contained GraphiQL IDE page served on the GraphQL endpoint.

/// Build a standalone GraphiQL IDE page whose fetcher posts to `endpoint`.
///
/// Loads GraphiQL from a CDN (no build step). The endpoint is injected as a
/// JSON-encoded string so it is safe inside the inline script.
#[must_use]
pub fn graphiql_html(endpoint: &str) -> String {
    let endpoint_json = serde_json::to_string(endpoint)
        .unwrap_or_else(|_| "\"/graphql\"".to_string());
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>GraphiQL</title>
  <style>body {{ margin: 0; height: 100vh; }} #graphiql {{ height: 100vh; }}</style>
  <link rel="stylesheet" href="https://unpkg.com/graphiql@3/graphiql.min.css" />
</head>
<body>
  <div id="graphiql">Loading GraphiQL…</div>
  <script crossorigin src="https://unpkg.com/react@18/umd/react.production.min.js"></script>
  <script crossorigin src="https://unpkg.com/react-dom@18/umd/react-dom.production.min.js"></script>
  <script crossorigin src="https://unpkg.com/graphiql@3/graphiql.min.js"></script>
  <script>
    const endpoint = {endpoint_json};
    const fetcher = GraphiQL.createFetcher({{ url: endpoint }});
    const root = ReactDOM.createRoot(document.getElementById('graphiql'));
    root.render(React.createElement(GraphiQL, {{ fetcher: fetcher }}));
  </script>
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_targets_the_configured_endpoint() {
        let html = graphiql_html("/api/graphql");
        assert!(html.contains("/api/graphql"));
        assert!(html.to_lowercase().contains("graphiql"));
        assert!(html.contains("<!DOCTYPE html>"));
    }
}
