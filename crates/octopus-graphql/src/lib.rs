//! GraphQL-aware gateway layer for Octopus.
//!
//! Provides a middleware that parses incoming GraphQL queries, enforces
//! depth/complexity/introspection policy, serves a GraphiQL IDE, and delegates
//! valid operations to the normal proxy pipeline. Modules are added by
//! subsequent implementation tasks.

pub mod query;

pub use query::{analyze_query, QueryAnalysis};

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_compiles() {
        assert_eq!(2 + 2, 4);
    }
}
