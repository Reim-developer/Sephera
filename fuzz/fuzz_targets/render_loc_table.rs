#![no_main]

use std::{path::PathBuf, time::Duration};

use libfuzzer_sys::fuzz_target;
use sephera::render_report_table;
use sephera_core::core::code_loc::{CodeLocReport, LanguageLoc, LocMetrics};

const LANGUAGE_NAMES: [&str; 6] =
    ["Rust", "Python", "TypeScript", "JSON", "HTML", "Shell Script"];

fuzz_target!(|data: &[u8]| {
    let language_count = usize::from(data.first().copied().unwrap_or(0))
        % (LANGUAGE_NAMES.len() + 1);
    let metric_bytes = data.get(1..).unwrap_or(&[]);
    let mut by_language = Vec::new();
    let mut totals = LocMetrics::zero();
    let mut files_scanned = 0_u64;

    for (index, chunk) in metric_bytes.chunks(8).take(language_count).enumerate() {
        let metrics = LocMetrics {
            code_lines: scaled_value(chunk.first().copied().unwrap_or(0)),
            comment_lines: scaled_value(chunk.get(1).copied().unwrap_or(0)),
            empty_lines: scaled_value(chunk.get(2).copied().unwrap_or(0)),
            size_bytes: scaled_value(chunk.get(3).copied().unwrap_or(0)) * 16,
        };
        totals.add_assign(metrics);
        files_scanned += scaled_value(chunk.get(4).copied().unwrap_or(0)) + 1;
        by_language.push(LanguageLoc {
            language: LANGUAGE_NAMES[index],
            metrics,
        });
    }

    let report = CodeLocReport {
        base_path: PathBuf::from("fuzz"),
        by_language,
        totals,
        files_scanned,
        languages_detected: language_count,
        elapsed: Duration::ZERO,
    };

    let _ = render_report_table(&report);
});

fn scaled_value(value: u8) -> u64 {
    u64::from(value) * 257
}
