use std::time::Duration;

pub fn render_elapsed_line(duration: Duration) -> String {
    format!(
        "Elapsed: {} ({:.6} s)",
        format_compact_duration(duration),
        duration.as_secs_f64()
    )
}

fn format_compact_duration(duration: Duration) -> String {
    if duration.as_secs() > 0 {
        return format!("{:.3} s", duration.as_secs_f64());
    }

    let millis = duration.as_secs_f64() * 1_000.0;
    if millis >= 1.0 {
        return format!("{millis:.3} ms");
    }

    let micros = duration.as_secs_f64() * 1_000_000.0;
    if micros >= 1.0 {
        return format!("{micros:.3} us");
    }

    format!("{} ns", duration.as_nanos())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::render_elapsed_line;

    #[test]
    fn renders_elapsed_line_in_seconds() {
        let rendered = render_elapsed_line(Duration::from_millis(1_234));

        assert_eq!(rendered, "Elapsed: 1.234 s (1.234000 s)");
    }

    #[test]
    fn renders_elapsed_line_in_milliseconds() {
        let rendered = render_elapsed_line(Duration::from_micros(12_345));

        assert_eq!(rendered, "Elapsed: 12.345 ms (0.012345 s)");
    }
}
