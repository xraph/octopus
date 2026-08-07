//! GraphQL query analysis built on `apollo-compiler`.
//!
//! Parses an operation document syntactically (no schema required) and computes
//! selection depth, an approximate complexity (field count), and whether the
//! operation contains an introspection field.
//!
//! Named fragments are expanded during the walk, so nesting hidden inside a
//! fragment is counted against the depth and complexity limits rather than
//! bypassing them.

use apollo_compiler::ast::{Definition, Document, FragmentDefinition, Selection};
use octopus_core::{Error, Result};
use std::collections::HashMap;

/// Result of analyzing a GraphQL operation document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryAnalysis {
    /// Maximum selection-set nesting depth across all operations (root = 1).
    pub depth: usize,
    /// Approximate cost: total number of selected fields.
    pub complexity: usize,
    /// True if any operation selects `__schema` or `__type` at any level.
    pub has_introspection: bool,
}

/// Parse and analyze a GraphQL operation document.
///
/// # Errors
/// Returns [`Error::InvalidRequest`] if the document fails to parse.
pub fn analyze_query(source: &str) -> Result<QueryAnalysis> {
    let doc = Document::parse(source, "operation.graphql")
        .map_err(|e| Error::InvalidRequest(format!("GraphQL parse error: {e}")))?;

    // Collected up front: GraphQL permits an operation to spread a fragment
    // that is defined later in the document, so a single streaming walk would
    // reach the spread before it had the definition.
    let fragments: HashMap<&str, &FragmentDefinition> = doc
        .definitions
        .iter()
        .filter_map(|def| match def {
            Definition::FragmentDefinition(frag) => Some((frag.name.as_str(), &**frag)),
            _ => None,
        })
        .collect();

    let mut analysis = QueryAnalysis {
        depth: 0,
        complexity: 0,
        has_introspection: false,
    };

    for def in &doc.definitions {
        if let Definition::OperationDefinition(op) = def {
            let mut acc = WalkAcc::default();
            let mut active = Vec::new();
            walk(&op.selection_set, 1, &fragments, &mut active, &mut acc)?;
            analysis.depth = analysis.depth.max(acc.max_depth);
            analysis.complexity += acc.field_count;
            analysis.has_introspection |= acc.has_introspection;
        }
    }

    Ok(analysis)
}

#[derive(Default)]
struct WalkAcc {
    max_depth: usize,
    field_count: usize,
    has_introspection: bool,
}

/// Recursively walk a selection set. `depth` is the current nesting level
/// (root selection set = 1). Neither inline fragments nor named fragment
/// spreads increase depth; their selections are evaluated at the enclosing
/// level, matching how the spec inlines them.
///
/// `active` is the stack of fragment names currently being expanded, used to
/// detect cycles.
///
/// # Errors
/// Returns [`Error::InvalidRequest`] if a fragment spread re-enters a fragment
/// already being expanded.
fn walk<'a>(
    selections: &'a [Selection],
    depth: usize,
    fragments: &HashMap<&'a str, &'a FragmentDefinition>,
    active: &mut Vec<&'a str>,
    acc: &mut WalkAcc,
) -> Result<()> {
    acc.max_depth = acc.max_depth.max(depth);
    for sel in selections {
        match sel {
            Selection::Field(field) => {
                acc.field_count += 1;
                let name = field.name.as_str();
                if name == "__schema" || name == "__type" {
                    acc.has_introspection = true;
                }
                if !field.selection_set.is_empty() {
                    walk(&field.selection_set, depth + 1, fragments, active, acc)?;
                }
            }
            Selection::InlineFragment(frag) => {
                walk(&frag.selection_set, depth, fragments, active, acc)?;
            }
            Selection::FragmentSpread(spread) => {
                let name = spread.fragment_name.as_str();

                // A stack, not a cumulative visited-set: the same fragment
                // spread in sibling branches must expand each time, and only
                // re-entry while still in flight is a cycle.
                if active.contains(&name) {
                    return Err(Error::InvalidRequest(format!(
                        "GraphQL fragment cycle detected: '{name}'"
                    )));
                }

                // Unknown spreads are skipped here; rejected in a follow-up.
                if let Some(frag) = fragments.get(name).copied() {
                    active.push(name);
                    walk(&frag.selection_set, depth, fragments, active, acc)?;
                    active.pop();
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_query_has_depth_one() {
        let a = analyze_query("{ a b c }").unwrap();
        assert_eq!(a.depth, 1);
        assert_eq!(a.complexity, 3);
        assert!(!a.has_introspection);
    }

    #[test]
    fn nested_query_counts_depth_and_fields() {
        let a = analyze_query("{ user { name posts { title } } }").unwrap();
        assert_eq!(a.depth, 3); // user(1) -> name/posts(2) -> title(3)
        assert_eq!(a.complexity, 4); // user, name, posts, title
    }

    #[test]
    fn detects_introspection() {
        let a = analyze_query("{ __schema { types { name } } }").unwrap();
        assert!(a.has_introspection);
    }

    #[test]
    fn inline_fragments_do_not_inflate_depth() {
        let a = analyze_query("{ node { ... on User { name } } }").unwrap();
        assert_eq!(a.depth, 2); // node(1) -> (inline fragment, same level) name(2)
    }

    #[test]
    fn syntax_error_is_invalid_request() {
        let err = analyze_query("{ unclosed ").unwrap_err();
        assert!(matches!(err, Error::InvalidRequest(_)));
    }

    #[test]
    fn fragment_spread_expands_into_depth_and_complexity() {
        // The bypass this change exists to close: without expansion this
        // measures depth 2 / complexity 1.
        let a = analyze_query(
            "query { user { ...deep } } fragment deep on User { posts { comments { title } } }",
        )
        .unwrap();
        // user(1) -> posts(2, fragment inlined at user's level) -> comments(3) -> title(4)
        assert_eq!(a.depth, 4);
        assert_eq!(a.complexity, 4); // user, posts, comments, title
    }

    #[test]
    fn fragment_defined_after_operation_resolves() {
        // Forward reference: the spread is parsed before the definition exists,
        // which is why collection is a separate pass.
        let a = analyze_query("query { user { ...f } } fragment f on User { name }").unwrap();
        assert_eq!(a.depth, 2);
        assert_eq!(a.complexity, 2); // user, name
    }

    #[test]
    fn same_fragment_spread_twice_counts_twice() {
        // Guards against using a cumulative visited-set for cycle detection,
        // which would expand `f` once and undercount the second branch.
        let a = analyze_query("query { a { ...f } b { ...f } } fragment f on T { x y }").unwrap();
        assert_eq!(a.complexity, 6); // a, x, y, b, x, y
        assert_eq!(a.depth, 2);
    }

    #[test]
    fn fragment_spread_inside_inline_fragment_keeps_enclosing_depth() {
        let a =
            analyze_query("query { node { ... on User { ...f } } } fragment f on User { name }")
                .unwrap();
        assert_eq!(a.depth, 2); // node(1) -> name(2); neither fragment form adds a level
        assert_eq!(a.complexity, 2); // node, name
    }

    #[test]
    fn self_referential_fragment_is_rejected() {
        let err = analyze_query("query { a { ...f } } fragment f on T { ...f }").unwrap_err();
        assert!(matches!(err, Error::InvalidRequest(_)));
    }

    #[test]
    fn mutually_recursive_fragments_are_rejected() {
        let err = analyze_query(
            "query { a { ...f } } fragment f on T { ...g } fragment g on T { ...f }",
        )
        .unwrap_err();
        assert!(matches!(err, Error::InvalidRequest(_)));
    }
}
