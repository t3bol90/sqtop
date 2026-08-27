//! Pure helpers for user-driven column reordering and column definitions.

use crate::responsive::{ColumnSpec, Tier};

/// Return a column-name list that reconciles saved order with default columns.
///
/// Rules:
/// 1. Result contains every name in `default` exactly once.
/// 2. Relative order of entries in `saved` that exist in `default` is preserved.
/// 3. Names in `saved` not present in `default` are dropped.
/// 4. Names in `default` not present in `saved` are appended in their default order.
///
/// Malformed-input coercions:
/// - Non-string entries inside `saved` are skipped.
/// - Duplicate entries in `saved` are de-duplicated (first occurrence wins).
pub fn reconcile_order(saved: &[String], default: &[String]) -> Vec<String> {
    let default_set: std::collections::HashSet<&String> = default.iter().collect();
    let mut seen = std::collections::HashSet::new();
    let mut ordered = Vec::new();

    for entry in saved {
        if seen.contains(entry) {
            continue;
        }
        seen.insert(entry.clone());
        if default_set.contains(entry) {
            ordered.push(entry.clone());
        }
    }

    // Append default names not present in saved, in their original default order.
    for name in default {
        if !seen.contains(name) {
            ordered.push(name.clone());
        }
    }

    ordered
}

/// Return a new list with `name` repositioned.
///
/// - If `before` is `None`, `name` is moved to the end.
/// - If `name` is not in `order`, return `order` unchanged.
/// - If `before` is not in `order`, `name` is moved to the end.
/// - Pure: does not mutate the input.
pub fn move_in_order(order: &[String], name: &str, before: Option<&str>) -> Vec<String> {
    if !order.contains(&name.to_string()) {
        return order.to_vec();
    }

    let mut result: Vec<String> = order
        .iter()
        .filter(|x| x.as_str() != name)
        .cloned()
        .collect();

    match before {
        None => result.push(name.to_string()),
        Some(before_name) => {
            if let Some(idx) = result.iter().position(|x| x == before_name) {
                result.insert(idx, name.to_string());
            } else {
                result.push(name.to_string());
            }
        }
    }

    result
}

/// Jobs view column definitions.
///
/// ColumnSpec(name, min_width, content_max, priority, min_tier)
/// content_max will be overridden at runtime from config.
pub fn jobs_columns() -> Vec<ColumnSpec> {
    vec![
        ColumnSpec::new("JOBID", 8, 12, 100, Tier::Xs),
        ColumnSpec::new("STATE", 10, 14, 95, Tier::Xs),
        ColumnSpec::new("NAME", 8, 24, 90, Tier::Xs),
        ColumnSpec::new("USER", 8, 12, 80, Tier::Sm),
        ColumnSpec::new("TIME", 10, 12, 75, Tier::Sm),
        ColumnSpec::new("TIME_LEFT", 10, 12, 70, Tier::Sm),
        ColumnSpec::new("PARTITION", 9, 14, 60, Tier::Md),
        ColumnSpec::new("NODES", 6, 8, 55, Tier::Md),
        ColumnSpec::new("CPUS", 6, 8, 50, Tier::Md),
        ColumnSpec::new("QOS", 8, 12, 45, Tier::Md),
        ColumnSpec::new("TIME_LIMIT", 10, 12, 40, Tier::Md),
        ColumnSpec::new("NODELIST(REASON)", 14, 40, 30, Tier::Lg),
    ]
}

/// Nodes view column definitions.
///
/// ColumnSpec(name, min_width, content_max, priority, min_tier)
pub fn nodes_columns() -> Vec<ColumnSpec> {
    vec![
        ColumnSpec::new("NODE", 12, 20, 100, Tier::Xs),
        ColumnSpec::new("STATE", 12, 16, 95, Tier::Xs),
        ColumnSpec::new("CPU%", 14, 18, 90, Tier::Xs),
        ColumnSpec::new("GPU%", 14, 18, 80, Tier::Sm),
        ColumnSpec::new("CPUS A/T", 10, 12, 75, Tier::Sm),
        ColumnSpec::new("GPU A/T", 9, 12, 70, Tier::Sm),
        ColumnSpec::new("MEM FREE", 10, 12, 60, Tier::Md),
        ColumnSpec::new("PARTITION", 12, 20, 55, Tier::Md),
        ColumnSpec::new("MEM TOTAL", 10, 12, 45, Tier::Lg),
        ColumnSpec::new("LOAD", 8, 10, 40, Tier::Lg),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // reconcile_order tests
    #[test]
    fn test_reconcile_identity() {
        let default = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let saved = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        assert_eq!(reconcile_order(&saved, &default), default);
    }

    #[test]
    fn test_reconcile_empty_saved() {
        let default = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        assert_eq!(reconcile_order(&[], &default), default);
    }

    #[test]
    fn test_reconcile_dropped_name_appended() {
        let default = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let saved = vec!["A".to_string(), "C".to_string()];
        assert_eq!(
            reconcile_order(&saved, &default),
            vec!["A".to_string(), "C".to_string(), "B".to_string()]
        );
    }

    #[test]
    fn test_reconcile_unknown_saved_name_dropped() {
        let default = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let saved = vec![
            "A".to_string(),
            "X".to_string(),
            "B".to_string(),
            "C".to_string(),
        ];
        assert_eq!(
            reconcile_order(&saved, &default),
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        );
    }

    #[test]
    fn test_reconcile_permutation_preserved() {
        let default = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
        ];
        let saved = vec![
            "D".to_string(),
            "B".to_string(),
            "A".to_string(),
            "C".to_string(),
        ];
        assert_eq!(
            reconcile_order(&saved, &default),
            vec![
                "D".to_string(),
                "B".to_string(),
                "A".to_string(),
                "C".to_string()
            ]
        );
    }

    #[test]
    fn test_reconcile_duplicates_first_wins() {
        let default = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let saved = vec![
            "A".to_string(),
            "B".to_string(),
            "A".to_string(),
            "C".to_string(),
        ];
        assert_eq!(
            reconcile_order(&saved, &default),
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        );
    }

    #[test]
    fn test_reconcile_all_unknown_saved() {
        let default = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let saved = vec!["X".to_string(), "Y".to_string(), "Z".to_string()];
        assert_eq!(reconcile_order(&saved, &default), default);
    }

    #[test]
    fn test_reconcile_empty_default() {
        let saved = vec!["A".to_string(), "B".to_string()];
        assert_eq!(reconcile_order(&saved, &[]), Vec::<String>::new());
    }

    #[test]
    fn test_reconcile_multiple_unknowns_and_permutation() {
        let default = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
        ];
        let saved = vec![
            "D".to_string(),
            "C".to_string(),
            "X".to_string(),
            "A".to_string(),
        ];
        assert_eq!(
            reconcile_order(&saved, &default),
            vec![
                "D".to_string(),
                "C".to_string(),
                "A".to_string(),
                "B".to_string()
            ]
        );
    }

    // move_in_order tests
    #[test]
    fn test_move_first_to_last() {
        let order = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
        ];
        assert_eq!(
            move_in_order(&order, "A", None),
            vec![
                "B".to_string(),
                "C".to_string(),
                "D".to_string(),
                "A".to_string()
            ]
        );
    }

    #[test]
    fn test_move_last_to_first() {
        let order = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
        ];
        assert_eq!(
            move_in_order(&order, "D", Some("A")),
            vec![
                "D".to_string(),
                "A".to_string(),
                "B".to_string(),
                "C".to_string()
            ]
        );
    }

    #[test]
    fn test_move_middle_to_middle() {
        let order = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
        ];
        assert_eq!(
            move_in_order(&order, "B", Some("D")),
            vec![
                "A".to_string(),
                "C".to_string(),
                "B".to_string(),
                "D".to_string()
            ]
        );
    }

    #[test]
    fn test_move_noop_same_position() {
        let order = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let result = move_in_order(&order, "A", Some("B"));
        assert_eq!(
            result,
            vec!["A".to_string(), "B".to_string(), "C".to_string()]
        );
    }

    #[test]
    fn test_move_before_none_appends() {
        let order = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        assert_eq!(
            move_in_order(&order, "B", None),
            vec!["A".to_string(), "C".to_string(), "B".to_string()]
        );
    }

    #[test]
    fn test_move_name_not_in_order() {
        let order = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        assert_eq!(move_in_order(&order, "X", Some("A")), order);
    }

    #[test]
    fn test_move_before_not_in_order() {
        let order = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        assert_eq!(
            move_in_order(&order, "A", Some("Z")),
            vec!["B".to_string(), "C".to_string(), "A".to_string()]
        );
    }

    #[test]
    fn test_move_pure_does_not_mutate() {
        let order = vec!["A".to_string(), "B".to_string(), "C".to_string()];
        let original = order.clone();
        move_in_order(&order, "A", None);
        assert_eq!(order, original);
    }

    #[test]
    fn test_move_single_element() {
        assert_eq!(
            move_in_order(&["A".to_string()], "A", None),
            vec!["A".to_string()]
        );
        assert_eq!(
            move_in_order(&["A".to_string()], "A", Some("A")),
            vec!["A".to_string()]
        );
    }

    // Jobs columns tests
    #[test]
    fn test_jobs_highest_priority_is_jobid() {
        let cols = jobs_columns();
        let top = cols.iter().max_by_key(|c| c.priority).unwrap();
        assert_eq!(top.name, "JOBID");
    }

    #[test]
    fn test_jobs_lowest_priority_is_nodelist() {
        let cols = jobs_columns();
        let bottom = cols.iter().min_by_key(|c| c.priority).unwrap();
        assert_eq!(bottom.name, "NODELIST(REASON)");
    }

    // Nodes columns tests
    #[test]
    fn test_nodes_highest_priority_is_node() {
        let cols = nodes_columns();
        let top = cols.iter().max_by_key(|c| c.priority).unwrap();
        assert_eq!(top.name, "NODE");
    }
}
