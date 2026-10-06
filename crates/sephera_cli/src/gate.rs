//! Thresholds a caller can turn into a build failure.
//!
//! Every command in this tool ends the same way: it prints a report and exits
//! zero. That makes the reports good for reading and useless for enforcing. A
//! pipeline cannot say "fail when a cycle appears" or "fail when this file's
//! blast radius grows past twenty files" without scraping prose, and scraping
//! prose is exactly the kind of integration that passes in review and rots in
//! six months.
//!
//! The exit code is deliberately distinct from the failure code, so CI can tell
//! "the rule was broken" from "the tool broke":
//!
//! | code | meaning |
//! |------|---------|
//! | 0 | analysed, nothing crossed a threshold |
//! | 1 | the analysis could not run |
//! | 2 | analysed, something crossed a threshold |
//!
//! Merging 1 and 2 into a single non-zero would mean a broken install and a
//! violated rule look identical in a log, which is the situation where someone
//! adds `--ignore-failures` to the workflow and then never notices either.

use std::process::ExitCode;

/// Exit code for "the analysis ran and found something you asked to fail on".
///
/// Distinct from [`ExitCode::FAILURE`], which means the analysis could not run
/// at all. See the module docs for why.
pub const GATE_VIOLATED: u8 = 2;

/// One threshold, and the measurement checked against it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate {
    /// What is being counted, phrased for a human.
    label: &'static str,
    /// The value found.
    actual: u64,
    /// The value at or above which the run fails.
    limit: u64,
}

impl Gate {
    /// Build a gate.
    ///
    /// `limit` is the first value that fails, not the last value that passes, so
    /// `--fail-on-unresolved 1` means "fail as soon as one path fails to
    /// resolve". Zero is rejected by the argument parser rather than here: a
    /// limit of zero would fail every run, including the clean ones, which looks
    /// like a broken tool rather than a broken repository.
    #[must_use]
    pub const fn new(label: &'static str, actual: u64, limit: u64) -> Self {
        Self {
            label,
            actual,
            limit,
        }
    }

    /// Whether this gate's threshold was reached.
    #[must_use]
    pub const fn crossed(&self) -> bool {
        self.actual >= self.limit
    }

    /// The line printed to stderr when this gate is crossed.
    #[must_use]
    pub fn failure_line(&self) -> String {
        format!(
            "threshold exceeded: {} is {}, limit is {}",
            self.label, self.actual, self.limit
        )
    }
}

/// Turn a set of gates into an exit code.
///
/// Returns [`ExitCode::SUCCESS`] when nothing crossed. Otherwise every crossed
/// gate is named, all of them rather than just the first: a run that violates
/// three rules should not take three CI runs to find out which.
pub fn evaluate(gates: &[Gate]) -> ExitCode {
    let crossed: Vec<&Gate> =
        gates.iter().filter(|gate| gate.crossed()).collect();

    if crossed.is_empty() {
        return ExitCode::SUCCESS;
    }

    for gate in &crossed {
        eprintln!("error: {}", gate.failure_line());
    }
    ExitCode::from(GATE_VIOLATED)
}

#[cfg(test)]
mod tests {
    use super::{GATE_VIOLATED, Gate, evaluate};

    #[test]
    fn a_limit_is_the_first_failing_value_not_the_last_passing_one() {
        // The distinction that matters: someone writing
        // `--fail-on-unresolved 1` wants to fail on the first gap, not to
        // tolerate one gap and fail on the second.
        assert!(Gate::new("x", 1, 1).crossed(), "1 is at the limit");
        assert!(Gate::new("x", 2, 1).crossed(), "above the limit");
        assert!(!Gate::new("x", 0, 1).crossed(), "below the limit");
    }

    #[test]
    fn an_unmet_gate_exits_zero() {
        let gates = [Gate::new("cycles", 0, 1), Gate::new("gaps", 3, 5)];
        assert_eq!(evaluate(&gates), std::process::ExitCode::SUCCESS);
    }

    #[test]
    fn a_met_gate_exits_with_the_gate_code_not_the_failure_code() {
        let gates = [Gate::new("cycles", 2, 1)];
        let code = evaluate(&gates);

        assert_ne!(code, std::process::ExitCode::SUCCESS);
        assert_ne!(
            code,
            std::process::ExitCode::FAILURE,
            "a violated rule must not look like a broken tool"
        );
        assert_eq!(code, std::process::ExitCode::from(GATE_VIOLATED));
    }

    #[test]
    fn no_gates_is_success() {
        assert_eq!(evaluate(&[]), std::process::ExitCode::SUCCESS);
    }

    #[test]
    fn the_failure_line_names_the_label_and_both_numbers() {
        let gate = Gate::new("Circular dependencies", 3, 1);
        let line = gate.failure_line();

        assert!(line.contains("Circular dependencies"), "{line}");
        assert!(
            line.contains(" is 3,"),
            "the value found must be stated: {line}"
        );
        assert!(
            line.contains("limit is 1"),
            "the limit must be stated: {line}"
        );
    }
}
