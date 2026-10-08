from __future__ import annotations

from benchmark_model import BenchmarkReport, CaseResult, CommandStats, OutputSummary


def render_markdown_report(report: BenchmarkReport) -> str:
	lines = [
		"# Sephera CLI Benchmark Report",
		"",
		f"Generated at (UTC): {report.generated_at_utc}",
		"",
		"## Environment",
		"",
		f"- Platform: {report.environment.platform}",
		f"- Machine: {report.environment.machine}",
		f"- Processor: {report.environment.processor}",
		f"- CPU count: {format_optional_int(report.environment.cpu_count)}",
		f"- Python version: {report.environment.python_version}",
		f"- Python executable: {report.environment.python_executable}",
		f"- PROCESSOR_ARCHITECTURE: {format_optional_str(report.environment.os_arch_summary.processor_architecture_env)}",
		f"- PROCESSOR_IDENTIFIER: {format_optional_str(report.environment.os_arch_summary.processor_identifier_env)}",
		"",
		"## Settings",
		"",
		f"- Datasets: {', '.join(report.settings.datasets)}",
		f"- Warmup runs: {report.settings.warmup_runs}",
		f"- Measured runs: {report.settings.measured_runs}",
		"",
		"## Summary",
		"",
		f"- Cache policy: {report.settings.cache_policy} ({report.settings.warmup_runs} warmup, "
		f"{report.settings.measured_runs} measured)",
		"",
		"| Dataset | Rust min | Rust mean | Rust median | Rust max | spread |",
		"| --- | ---: | ---: | ---: | ---: | ---: |",
	]

	for result in report.results:
		lines.append(
			"| "
			f"{result.dataset} | {result.rust.min_seconds:.6f} | {result.rust.mean_seconds:.6f} | "
			f"{result.rust.median_seconds:.6f} | {result.rust.max_seconds:.6f} | "
			f"{result.rust.spread_ratio:.2f}x |"
		)

	lines.extend(["", "## Dataset Details", ""])
	for result in report.results:
		lines.extend(render_case_markdown(result))

	return "\n".join(lines) + "\n"


def render_case_markdown(result: CaseResult) -> list[str]:
	lines = [
		f"### {result.dataset}",
		"",
		f"- Path: `{result.path}`",
		f"- Ignore patterns: {render_ignore_patterns(result.ignore_patterns)}",
		f"- Rust median: `{result.rust.median_seconds:.6f}s`",
		"",
		"#### Rust",
		"",
		*render_command_details(result.rust),
		"",
	]
	return lines


def render_command_details(stats: CommandStats) -> list[str]:
	lines = [
		f"- Command: `{stats.command}`",
		f"- Runs (s): `{format_run_samples(stats.runs)}`",
		f"- Min/Mean/Median/Max: `{stats.min_seconds:.6f} / {stats.mean_seconds:.6f} / {stats.median_seconds:.6f} / {stats.max_seconds:.6f}`",
		f"- Spread (max/min): `{stats.spread_ratio:.2f}x`",
	]
	if stats.summary is not None:
		lines.extend(render_summary_lines(stats.summary))
	lines.extend([
		"",
		"Output:",
		"```text",
		*stats.stdout_lines,
		"```",
	])
	if stats.stderr_lines:
		lines.extend([
			"",
			"stderr:",
			"```text",
			*stats.stderr_lines,
			"```",
		])
	return lines


def render_summary_lines(summary: OutputSummary) -> list[str]:
	return [
		f"- Summary code/comment/empty: `{summary.code_lines} / {summary.comment_lines} / {summary.empty_lines}`",
		f"- Summary size bytes: `{summary.size_bytes}`",
		f"- Files scanned: `{summary.files_scanned}`",
		f"- Languages detected: `{summary.languages_detected}`",
	]


def render_terminal_summary(results: list[CaseResult]) -> str:
	lines = ["Dataset | Rust mean (s) | Rust median (s)"]
	for result in results:
		lines.append(
			f"{result.dataset} | {result.rust.mean_seconds:.6f} | {result.rust.median_seconds:.6f}"
		)
	return "\n".join(lines)


def render_ignore_patterns(patterns: tuple[str, ...]) -> str:
	if not patterns:
		return "`none`"
	return ", ".join(f"`{pattern}`" for pattern in patterns)


def format_run_samples(samples: list[float]) -> str:
	return ", ".join(f"{sample:.6f}" for sample in samples)


def format_optional_int(value: int | None) -> str:
	return "n/a" if value is None else str(value)


def format_optional_str(value: str | None) -> str:
	return "n/a" if value is None else value
