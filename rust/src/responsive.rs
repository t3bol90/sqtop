//! Responsive tier infrastructure for sqtop.
//!
//! Defines terminal-width breakpoints and helpers used across all views
//! to make layout decisions without magic numbers scattered everywhere.

/// Terminal width tier for responsive layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tier {
    Xs,
    Sm,
    Md,
    Lg,
}

impl Tier {
    /// Minimum width (inclusive) to enter this tier.
    pub const fn min_width(self) -> u16 {
        match self {
            Self::Xs => 40,
            Self::Sm => 80,
            Self::Md => 110,
            Self::Lg => 160,
        }
    }

    /// Return all tiers in rank order (xs lowest, lg highest).
    const fn all() -> [Self; 4] {
        [Self::Xs, Self::Sm, Self::Md, Self::Lg]
    }

    /// Return the rank (0..4) of this tier.
    const fn rank(self) -> usize {
        match self {
            Self::Xs => 0,
            Self::Sm => 1,
            Self::Md => 2,
            Self::Lg => 3,
        }
    }
}

/// Terminal dimensions below which sqtop refuses to render.
pub const TOO_SMALL_WIDTH: u16 = 40;
pub const TOO_SMALL_HEIGHT: u16 = 10;

/// Chrome overhead: DataTable left/right padding (2 cells) + scrollbar reserve (1 cell).
///
/// Measured empirically: Textual DataTable uses 1-cell left pad, 1-cell right pad,
/// and reserves 1 cell for the scrollbar when content overflows.
pub const CHROME_OVERHEAD: u16 = 3;

/// Return the responsive tier for the given terminal width.
pub fn tier_for(width: u16) -> Tier {
    if width < Tier::Sm.min_width() {
        Tier::Xs
    } else if width < Tier::Md.min_width() {
        Tier::Sm
    } else if width < Tier::Lg.min_width() {
        Tier::Md
    } else {
        Tier::Lg
    }
}

/// Return `true` if `width` qualifies for at least `target` tier.
///
/// # Examples
///
/// ```
/// # use sqtop::responsive::{at_least, Tier};
/// assert!(at_least(Tier::Sm, 80));
/// assert!(!at_least(Tier::Sm, 79));
/// assert!(at_least(Tier::Md, 110));
/// ```
pub fn at_least(target: Tier, width: u16) -> bool {
    tier_for(width).rank() >= target.rank()
}

/// Specification for a single table column used by [`allocate_columns`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSpec {
    /// Column name.
    pub name: String,
    /// Smallest readable width including 1-char padding.
    pub min_width: u16,
    /// Cap for auto-sizing (from per-view config or sensible default).
    pub content_max: u16,
    /// Higher = kept longer when budget shrinks.
    pub priority: u16,
    /// Eligibility filter: column only shown at this tier or wider.
    pub min_tier: Tier,
}

impl ColumnSpec {
    pub fn new(
        name: impl Into<String>,
        min_width: u16,
        content_max: u16,
        priority: u16,
        min_tier: Tier,
    ) -> Self {
        Self {
            name: name.into(),
            min_width,
            content_max,
            priority,
            min_tier,
        }
    }
}

/// Return list of `(name, width)` such that `sum(width) <= budget`.
///
/// Algorithm (spec §5.1.1):
/// 1. Filter to columns where `at_least(min_tier, budget+CHROME_OVERHEAD)` is true
///    (using the full terminal width implied by budget + CHROME_OVERHEAD).
/// 2. Sort by priority desc.
/// 3. Pass 1: assign min_width to each.
/// 4. Pass 2: distribute remaining budget by priority, capped at content_max.
/// 5. Pass 3: while sum > budget and len > 1, drop lowest-priority column.
/// 6. Return preserving the input ordering of survivors.
pub fn allocate_columns(
    budget: u16,
    columns: &[ColumnSpec],
    _current_tier: Tier,
) -> Vec<(String, u16)> {
    if budget == 0 {
        return Vec::new();
    }

    // Reconstruct the full terminal width so at_least() works on tier breakpoints.
    let terminal_width = budget.saturating_add(CHROME_OVERHEAD);

    // Step 1: filter by tier eligibility.
    let mut eligible: Vec<&ColumnSpec> = columns
        .iter()
        .filter(|col| at_least(col.min_tier, terminal_width))
        .collect();

    if eligible.is_empty() {
        return Vec::new();
    }

    // Step 2: work in priority-descending order.
    eligible.sort_by(|a, b| b.priority.cmp(&a.priority));

    // Step 3 (Pass 1): assign minimum widths.
    let mut assigned: std::collections::HashMap<String, u16> = eligible
        .iter()
        .map(|col| (col.name.clone(), col.min_width))
        .collect();

    // Check if even the minimums exceed budget — if so go straight to Pass 3.
    let total: u16 = assigned.values().sum();
    let mut remaining = budget.saturating_sub(total);

    // Step 4 (Pass 2): distribute surplus by priority order, capped at content_max.
    if remaining > 0 {
        for col in &eligible {
            if remaining == 0 {
                break;
            }
            let current = assigned[&col.name];
            let extra = remaining.min(col.content_max.saturating_sub(current));
            if extra > 0 {
                *assigned.get_mut(&col.name).unwrap() += extra;
                remaining = remaining.saturating_sub(extra);
            }
        }
    }

    // Step 5 (Pass 3): drop lowest-priority columns until we fit within budget.
    while assigned.values().sum::<u16>() > budget && assigned.len() > 1 {
        // Find the lowest-priority *still-assigned* column.
        let drop = eligible
            .iter()
            .filter(|col| assigned.contains_key(&col.name))
            .min_by_key(|col| col.priority)
            .unwrap();
        assigned.remove(&drop.name);
    }

    // Step 6: return in input order (not priority order).
    columns
        .iter()
        .filter_map(|col| {
            assigned
                .get(&col.name)
                .map(|&width| (col.name.clone(), width))
        })
        .collect()
}

/// Truncate `text` to fit within `width` cells, appending `…` if needed.
///
/// If `width` < 2, returns an empty string (no room for any visible character).
/// If `text` already fits, it is returned unchanged.
pub fn truncate_cell(text: &str, width: usize) -> String {
    if width < 2 {
        return String::new();
    }
    if text.len() <= width {
        return text.to_string();
    }
    format!("{}…", &text[..width - 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    // TIER_WIDTH constants
    #[test]
    fn test_tier_xs_width() {
        assert_eq!(Tier::Xs.min_width(), 40);
    }

    #[test]
    fn test_tier_sm_width() {
        assert_eq!(Tier::Sm.min_width(), 80);
    }

    #[test]
    fn test_tier_md_width() {
        assert_eq!(Tier::Md.min_width(), 110);
    }

    #[test]
    fn test_tier_lg_width() {
        assert_eq!(Tier::Lg.min_width(), 160);
    }

    #[test]
    fn test_too_small_width() {
        assert_eq!(TOO_SMALL_WIDTH, 40);
    }

    #[test]
    fn test_too_small_height() {
        assert_eq!(TOO_SMALL_HEIGHT, 10);
    }

    // tier_for boundaries
    #[test]
    fn test_tier_for_xs() {
        assert_eq!(tier_for(39), Tier::Xs);
        assert_eq!(tier_for(40), Tier::Xs);
        assert_eq!(tier_for(79), Tier::Xs);
    }

    #[test]
    fn test_tier_for_sm() {
        assert_eq!(tier_for(80), Tier::Sm);
        assert_eq!(tier_for(109), Tier::Sm);
    }

    #[test]
    fn test_tier_for_md() {
        assert_eq!(tier_for(110), Tier::Md);
        assert_eq!(tier_for(159), Tier::Md);
    }

    #[test]
    fn test_tier_for_lg() {
        assert_eq!(tier_for(160), Tier::Lg);
        assert_eq!(tier_for(9999), Tier::Lg);
    }

    // at_least
    #[test]
    fn test_at_least_xs_always_true() {
        assert!(at_least(Tier::Xs, 40));
        assert!(at_least(Tier::Xs, 79));
        assert!(at_least(Tier::Xs, 160));
    }

    #[test]
    fn test_at_least_sm() {
        assert!(!at_least(Tier::Sm, 79));
        assert!(at_least(Tier::Sm, 80));
        assert!(at_least(Tier::Sm, 160));
    }

    #[test]
    fn test_at_least_md() {
        assert!(!at_least(Tier::Md, 109));
        assert!(at_least(Tier::Md, 110));
        assert!(at_least(Tier::Md, 160));
    }

    #[test]
    fn test_at_least_lg() {
        assert!(!at_least(Tier::Lg, 159));
        assert!(at_least(Tier::Lg, 160));
        assert!(at_least(Tier::Lg, 9999));
    }

    // allocate_columns
    #[test]
    fn test_allocate_no_columns_below_min_budget() {
        let cols = vec![ColumnSpec::new("A", 8, 12, 100, Tier::Xs)];
        assert_eq!(allocate_columns(0, &cols, Tier::Xs), vec![]);
    }

    #[test]
    fn test_allocate_preserves_input_order() {
        let cols = vec![
            ColumnSpec::new("A", 8, 12, 50, Tier::Xs),
            ColumnSpec::new("B", 8, 12, 100, Tier::Xs),
            ColumnSpec::new("C", 8, 12, 75, Tier::Xs),
        ];
        let result = allocate_columns(100, &cols, Tier::Xs);
        let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["A", "B", "C"]);
    }

    #[test]
    fn test_allocate_drops_low_priority_when_budget_tight() {
        let cols = vec![
            ColumnSpec::new("HIGH", 8, 12, 100, Tier::Xs),
            ColumnSpec::new("LOW", 10, 14, 50, Tier::Xs),
        ];
        // Budget = 15, total min = 18, should drop LOW
        let result = allocate_columns(15, &cols, Tier::Xs);
        let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["HIGH"]);
    }

    #[test]
    fn test_allocate_respects_min_tier() {
        let cols = vec![
            ColumnSpec::new("XS", 8, 12, 100, Tier::Xs),
            ColumnSpec::new("LG", 10, 14, 50, Tier::Lg),
        ];
        // At width 100 (md tier), LG column should not appear
        let result = allocate_columns(97, &cols, Tier::Md);
        let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["XS"]);
    }

    // truncate_cell
    #[test]
    fn test_truncate_cell_truncates_with_ellipsis() {
        assert_eq!(truncate_cell("hello world", 5), "hell…");
    }

    #[test]
    fn test_truncate_cell_no_truncation_when_fits() {
        assert_eq!(truncate_cell("hi", 10), "hi");
    }

    #[test]
    fn test_truncate_cell_exact_fit() {
        assert_eq!(truncate_cell("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_cell_width_2_last_resort() {
        assert_eq!(truncate_cell("abc", 2), "a…");
    }

    #[test]
    fn test_truncate_cell_width_1_returns_empty() {
        assert_eq!(truncate_cell("abc", 1), "");
    }

    #[test]
    fn test_truncate_cell_width_0_returns_empty() {
        assert_eq!(truncate_cell("abc", 0), "");
    }

    #[test]
    fn test_truncate_cell_empty_string() {
        assert_eq!(truncate_cell("", 5), "");
    }
}
