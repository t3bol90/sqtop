//! Pure parsers for Slurm CLI output. No I/O lives here.

/// Shared `squeue` format string. The field count is fixed at 12; any change
/// here must be matched in `parse_squeue_row`.
pub const SQUEUE_FMT: &str = "%i|%j|%u|%T|%P|%D|%C|%M|%l|%R|%N|%q";

/// Shared `sinfo` format string for the Partitions view.
pub const SINFO_PARTITION_FMT: &str = "%P|%a|%l|%D|%T|%N";
