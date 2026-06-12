//! GraphQL query analysis built on `apollo-compiler`.
//!
//! Parses an operation document syntactically (no schema required at this
//! milestone) and computes selection depth, an approximate complexity (field
//! count), and whether the operation contains an introspection field.

use apollo_compiler::ast::{Definition, Document, Selection};
use octopus_core::{Error, Result};

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

    let mut analysis = QueryAnalysis {
        depth: 0,
        complexity: 0,
        has_introspection: false,
    };

    for def in &doc.definitions {
        if let Definition::OperationDefinition(op) = def {
            let mut acc = WalkAcc::default();
            walk(&op.selection_set, 1, &mut acc);
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
/// (root selection set = 1). Inline fragments do not increase depth; their
/// selections are evaluated at the enclosing level. Fragment spreads are not
/// expanded at this milestone (no fragment definitions available pre-schema).
fn walk(selections: &[Selection], depth: usize, acc: &mut WalkAcc) {
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
                    walk(&field.selection_set, depth + 1, acc);
                }
            }
            Selection::InlineFragment(frag) => {
                walk(&frag.selection_set, depth, acc);
            }
            Selection::FragmentSpread(_) => {
                // Not expanded pre-schema; revisited in a later milestone.
            }
        }
    }
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
}
