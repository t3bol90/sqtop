//! Responsive chrome: tab labels, subtitle, and footer binding visibility.
//!
//! This module ports the Python app.py chrome logic to provide:
//! - Tab labels that collapse from full to short at xs tier
//! - Subtitle truncation rules
//! - Footer binding visibility based on terminal tier
//!
//! All functions are pure - they take tier and dimensions as parameters
//! and return formatted strings or booleans.

use crate::app::Tab;
use crate::responsive::Tier;

/// Tab labels: (short, full) for each tab.
/// - short: used at xs tier, no bracket
/// - full: used at sm+ tier, includes [N] suffix
const TAB_LABELS: [(Tab, &str, &str); 5] = [
    (Tab::Jobs, "Jobs", "Jobs [1]"),
    (Tab::Nodes, "Nodes", "Nodes [2]"),
    (Tab::Partitions, "Partitions", "Partitions [3]"),
    (Tab::History, "History", "History [4]"),
    (Tab::Health, "Health", "Health [5]"),
];

/// Return the appropriate tab label for the given tab and tier.
///
/// At tier xs, returns the short label (no bracket).
/// At sm and above, returns the full label (with [N] suffix).
pub fn tab_label(tab: Tab, tier: Tier) -> &'static str {
    let (_, short, full) = TAB_LABELS
        .iter()
        .find(|(t, _, _)| std::mem::discriminant(t) == std::mem::discriminant(&tab))
        .expect("tab must be in TAB_LABELS");

    match tier {
        Tier::Xs => short,
        _ => full,
    }
}

/// Format the subtitle according to tier and terminal width.
///
/// - At xs: returns empty string
/// - At sm+: returns base truncated to ≤ width // 2 - 10 with ellipsis if needed
///
/// The ellipsis character is "…" (U+2026).
pub fn sub_title(base: &str, tier: Tier, width: u16) -> String {
    if matches!(tier, Tier::Xs) {
        return String::new();
    }

    // Truncate to ≤ width // 2 - 10 at sm+
    let max_width = (width / 2).saturating_sub(10) as usize;

    let base_len = base.chars().count();

    if base_len <= max_width || max_width == 0 {
        base.to_string()
    } else if max_width < 2 {
        String::new()
    } else {
        // Take (max_width - 1) characters and add ellipsis
        let truncated: String = base.chars().take(max_width - 1).collect();
        format!("{}…", truncated)
    }
}

/// Minimum tier for each action's binding to be shown in the footer.
///
/// Actions not listed here inherit their original show state.
/// The visibility DOES NOT disable the binding - the key still works,
/// it is only hidden from the footer at narrower tiers.
const BINDING_SHOW_AT: &[(&str, Tier)] = &[
    // Always visible (xs+)
    ("quit", Tier::Xs),
    ("show_keybindings", Tier::Xs),
    // sm+ bindings
    ("refresh", Tier::Sm),
    ("switch_tab('jobs')", Tier::Sm),
    ("switch_tab('nodes')", Tier::Sm),
    ("switch_tab('partitions')", Tier::Sm),
    ("switch_tab('history')", Tier::Sm),
    ("switch_tab('health')", Tier::Sm),
];

/// Return whether the given action's binding should be visible at the given tier.
///
/// Rules:
/// - If `originally_shown` is false, always returns false (hidden stays hidden)
/// - If the action is in BINDING_SHOW_AT, returns true only if tier >= minimum tier
/// - Otherwise returns true (not listed but originally shown = always visible)
///
/// Note: This only controls footer visibility, not whether the binding is active.
/// The key binding still works regardless of visibility.
pub fn binding_visible(action: &str, tier: Tier, originally_shown: bool) -> bool {
    if !originally_shown {
        return false; // hidden stays hidden, always
    }
    if let Some((_, min_tier)) = BINDING_SHOW_AT.iter().find(|(a, _)| *a == action) {
        tier_ge(tier, *min_tier)
    } else {
        true // listed nowhere but shown: always show
    }
}

/// Compare tiers: returns true if a >= b.
fn tier_ge(a: Tier, b: Tier) -> bool {
    tier_ord(a) >= tier_ord(b)
}

/// Convert tier to ordinal for comparison.
fn tier_ord(tier: Tier) -> u8 {
    match tier {
        Tier::Xs => 0,
        Tier::Sm => 1,
        Tier::Md => 2,
        Tier::Lg => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Tab label tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_tab_labels_constants_defined() {
        // All five tabs must be present in TAB_LABELS
        assert_eq!(TAB_LABELS.len(), 5);
        let tabs = TAB_LABELS.iter().map(|(t, _, _)| t).collect::<Vec<_>>();
        assert!(tabs.iter().any(|t| matches!(t, Tab::Jobs)));
        assert!(tabs.iter().any(|t| matches!(t, Tab::Nodes)));
        assert!(tabs.iter().any(|t| matches!(t, Tab::Partitions)));
        assert!(tabs.iter().any(|t| matches!(t, Tab::History)));
        assert!(tabs.iter().any(|t| matches!(t, Tab::Health)));
    }

    #[test]
    fn test_tab_labels_xs_short() {
        // At xs, the short label has no bracket suffix
        for (tab, short, _) in &TAB_LABELS {
            assert!(
                !short.contains('['),
                "Short label for {:?} must not contain '['",
                tab
            );
        }
    }

    #[test]
    fn test_tab_labels_full_has_bracket() {
        // At sm+, the full label includes [N]
        for (tab, _, full) in &TAB_LABELS {
            assert!(
                full.contains('['),
                "Full label for {:?} should contain '[N]'",
                tab
            );
        }
    }

    #[test]
    fn test_short_is_prefix_of_full() {
        // Short label text is a prefix of the full label
        for (tab, short, full) in &TAB_LABELS {
            assert!(
                full.starts_with(short),
                "Full label {:?} should start with short label {:?} for tab {:?}",
                full,
                short,
                tab
            );
        }
    }

    #[test]
    fn test_tab_label_xs_returns_short() {
        assert_eq!(tab_label(Tab::Jobs, Tier::Xs), "Jobs");
        assert_eq!(tab_label(Tab::Nodes, Tier::Xs), "Nodes");
        assert_eq!(tab_label(Tab::Partitions, Tier::Xs), "Partitions");
        assert_eq!(tab_label(Tab::History, Tier::Xs), "History");
        assert_eq!(tab_label(Tab::Health, Tier::Xs), "Health");
    }

    #[test]
    fn test_tab_label_sm_returns_full() {
        assert_eq!(tab_label(Tab::Jobs, Tier::Sm), "Jobs [1]");
        assert_eq!(tab_label(Tab::Nodes, Tier::Sm), "Nodes [2]");
        assert_eq!(tab_label(Tab::Partitions, Tier::Sm), "Partitions [3]");
        assert_eq!(tab_label(Tab::History, Tier::Sm), "History [4]");
        assert_eq!(tab_label(Tab::Health, Tier::Sm), "Health [5]");
    }

    #[test]
    fn test_tab_label_md_returns_full() {
        assert_eq!(tab_label(Tab::Jobs, Tier::Md), "Jobs [1]");
    }

    #[test]
    fn test_tab_label_lg_returns_full() {
        assert_eq!(tab_label(Tab::Jobs, Tier::Lg), "Jobs [1]");
    }

    // -----------------------------------------------------------------------
    // Subtitle tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_sub_title_empty_at_xs() {
        let result = sub_title("Slurm Dashboard", Tier::Xs, 80);
        assert_eq!(result, "");
    }

    #[test]
    fn test_sub_title_present_at_sm() {
        let result = sub_title("Slurm Dashboard", Tier::Sm, 200);
        assert_eq!(result, "Slurm Dashboard");
    }

    #[test]
    fn test_sub_title_present_at_lg() {
        let result = sub_title("Slurm Dashboard", Tier::Lg, 200);
        assert_eq!(result, "Slurm Dashboard");
    }

    #[test]
    fn test_sub_title_truncated_at_narrow_sm() {
        // At sm with width=80, max_width = 80//2 - 10 = 30
        let long_text = format!("Slurm Dashboard — {}", "a".repeat(100));
        let result = sub_title(&long_text, Tier::Sm, 80);
        assert!(result.chars().count() <= 30);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_sub_title_no_ellipsis_when_fits() {
        // Short sub_title is NOT truncated at lg
        let result = sub_title("Slurm Dashboard", Tier::Lg, 200);
        assert!(!result.contains('…'));
        assert_eq!(result, "Slurm Dashboard");
    }

    #[test]
    fn test_sub_title_truncation_exact_boundary() {
        // Test exact boundary: if base.len() == max_width, no truncation
        let width = 80u16;
        let max_width = (width / 2 - 10) as usize; // 30
        let exact_text = "a".repeat(max_width);
        let result = sub_title(&exact_text, Tier::Sm, width);
        assert_eq!(result, exact_text);
        assert!(!result.contains('…'));
    }

    #[test]
    fn test_sub_title_truncation_one_over() {
        // If base.len() = max_width + 1, should truncate with ellipsis
        let width = 80u16;
        let max_width = (width / 2 - 10) as usize; // 30
        let over_text = "a".repeat(max_width + 1);
        let result = sub_title(&over_text, Tier::Sm, width);
        assert_eq!(result.chars().count(), max_width);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_sub_title_very_narrow_terminal() {
        // Very narrow terminal: max_width < 2 should return empty
        let result = sub_title("Slurm Dashboard", Tier::Sm, 20);
        // width=20, max_width = 20//2 - 10 = 0
        assert_eq!(result, "Slurm Dashboard"); // max_width=0 case

        let result2 = sub_title("Slurm Dashboard", Tier::Sm, 22);
        // width=22, max_width = 22//2 - 10 = 1
        assert_eq!(result2, ""); // max_width < 2 case
    }

    // -----------------------------------------------------------------------
    // Binding visibility tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_xs_only_quit_and_keys() {
        // At xs, only 'quit' and 'show_keybindings' are visible (assuming originally shown)
        assert!(binding_visible("quit", Tier::Xs, true));
        assert!(binding_visible("show_keybindings", Tier::Xs, true));

        // Tab-switching actions are NOT visible at xs (even if originally shown)
        assert!(!binding_visible("switch_tab('jobs')", Tier::Xs, true));
        assert!(!binding_visible("switch_tab('nodes')", Tier::Xs, true));
        assert!(!binding_visible("switch_tab('partitions')", Tier::Xs, true));
        assert!(!binding_visible("switch_tab('history')", Tier::Xs, true));
        assert!(!binding_visible("switch_tab('health')", Tier::Xs, true));
        assert!(!binding_visible("refresh", Tier::Xs, true));
    }

    #[test]
    fn test_sm_includes_tabs_and_refresh() {
        // At sm, tab-switching and refresh are visible (assuming originally shown)
        assert!(binding_visible("quit", Tier::Sm, true));
        assert!(binding_visible("show_keybindings", Tier::Sm, true));
        assert!(binding_visible("refresh", Tier::Sm, true));
        assert!(binding_visible("switch_tab('jobs')", Tier::Sm, true));
        assert!(binding_visible("switch_tab('nodes')", Tier::Sm, true));
        assert!(binding_visible("switch_tab('partitions')", Tier::Sm, true));
        assert!(binding_visible("switch_tab('history')", Tier::Sm, true));
        assert!(binding_visible("switch_tab('health')", Tier::Sm, true));
    }

    #[test]
    fn test_sm_more_visible_than_xs() {
        // sm shows strictly more than xs
        let xs_count = BINDING_SHOW_AT
            .iter()
            .filter(|(_, tier)| tier_ge(Tier::Xs, *tier))
            .count();
        let sm_count = BINDING_SHOW_AT
            .iter()
            .filter(|(_, tier)| tier_ge(Tier::Sm, *tier))
            .count();
        assert!(sm_count > xs_count);
    }

    #[test]
    fn test_bindings_not_in_table_default_visible() {
        // Actions not in BINDING_SHOW_AT default to visible if originally shown
        assert!(binding_visible("some_unknown_action", Tier::Xs, true));
        assert!(binding_visible("some_unknown_action", Tier::Sm, true));
        assert!(binding_visible("some_unknown_action", Tier::Md, true));
    }

    #[test]
    fn test_md_shows_all_sm_bindings() {
        // md tier shows all sm+ bindings (assuming originally shown)
        for (action, _) in BINDING_SHOW_AT {
            if binding_visible(action, Tier::Sm, true) {
                assert!(
                    binding_visible(action, Tier::Md, true),
                    "{} visible at sm should also be visible at md",
                    action
                );
            }
        }
    }

    #[test]
    fn test_lg_shows_all_bindings() {
        // lg tier shows all bindings in the table (assuming originally shown)
        for (action, _) in BINDING_SHOW_AT {
            assert!(
                binding_visible(action, Tier::Lg, true),
                "{} should be visible at lg",
                action
            );
        }
    }

    #[test]
    fn test_tier_comparison() {
        // Test tier_ge helper
        assert!(tier_ge(Tier::Xs, Tier::Xs));
        assert!(tier_ge(Tier::Sm, Tier::Xs));
        assert!(tier_ge(Tier::Md, Tier::Sm));
        assert!(tier_ge(Tier::Lg, Tier::Md));

        assert!(!tier_ge(Tier::Xs, Tier::Sm));
        assert!(!tier_ge(Tier::Sm, Tier::Md));
        assert!(!tier_ge(Tier::Md, Tier::Lg));
    }

    #[test]
    fn test_health_tab_label_full_includes_bracket_5() {
        // Specific test for health tab
        let label = tab_label(Tab::Health, Tier::Sm);
        assert!(label.contains("[5]"));
    }

    #[test]
    fn test_health_binding_show_at_sm() {
        // Health tab switch binding is visible at sm (assuming originally shown)
        assert!(binding_visible("switch_tab('health')", Tier::Sm, true));
        assert!(!binding_visible("switch_tab('health')", Tier::Xs, true));
    }

    #[test]
    fn test_originally_hidden_stays_hidden() {
        // ctrl+c / quit with originally_shown = false is hidden at all tiers
        assert!(!binding_visible("quit", Tier::Xs, false));
        assert!(!binding_visible("quit", Tier::Sm, false));
        assert!(!binding_visible("quit", Tier::Md, false));
        assert!(!binding_visible("quit", Tier::Lg, false));
    }

    #[test]
    fn test_originally_hidden_in_table_stays_hidden() {
        // Action in tier table with originally_shown = false is still hidden at lg
        // (the hidden flag wins over the tier rule)
        assert!(!binding_visible("refresh", Tier::Lg, false));
        assert!(!binding_visible("switch_tab('jobs')", Tier::Lg, false));
    }

    #[test]
    fn test_not_in_table_originally_shown_always_visible() {
        // Action not in table with originally_shown = true is visible at every tier
        assert!(binding_visible("some_custom_action", Tier::Xs, true));
        assert!(binding_visible("some_custom_action", Tier::Sm, true));
        assert!(binding_visible("some_custom_action", Tier::Md, true));
        assert!(binding_visible("some_custom_action", Tier::Lg, true));
    }
}
