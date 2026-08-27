//! Application state and key dispatch.

/// Top-level tabs, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Jobs,
    Nodes,
    Partitions,
}
