from __future__ 	import annotations
from re 			import Pattern, compile, MULTILINE
from argparse		import ArgumentParser, ArgumentTypeError
from json 			import dumps
from dataclasses 	import asdict, dataclass
from datetime 		import datetime, timezone
from pathlib 		import Path
from typing 		import Final, Literal, TypeAlias, cast
from platform		import platform, machine, processor, python_version
from sys 			import executable
from shutil			import rmtree
from subprocess		import CompletedProcess, run, list2cmdline
from time 			import perf_counter
from statistics		import fmean, median
from os 			import cpu_count, environ, name as os_name

DatasetName: TypeAlias = Literal["repo", "small", "medium", "large", "extra-large"]
DATASET_CHOICES: Final[tuple[DatasetName, ...]] = ("repo", "small", "medium", "large", "extra-large")
DEFAULT_DATASETS: Final[tuple[DatasetName, ...]] = ("small", "medium", "large")
ROOT: Final[Path] = Path(__file__).resolve().parent.parent
BENCHMARKS_DIR: Final[Path] = ROOT / "benchmarks"
CORPUS_DIR: Final[Path] = BENCHMARKS_DIR / "generated_corpus"
REPORTS_DIR: Final[Path] = BENCHMARKS_DIR / "reports"
REPO_IGNORE_PATTERNS: Final[tuple[str, ...]] = (
	"target",
	r"\.venv",
	r"\.git",
	"benchmarks/generated_corpus",
	"benchmarks/reports",
)
RUST_TOTALS_RE: Final[Pattern[str]] = compile(
	r"^Totals: code=(?P<code>\d+) comment=(?P<comment>\d+) empty=(?P<empty>\d+) "
	r"size_bytes=(?P<size_bytes>\d+) files_scanned=(?P<files_scanned>\d+) "
	r"languages_detected=(?P<languages_detected>\d+)$",
	MULTILINE,
)
PYTHON_CODE_RE: Final[Pattern[str]] = compile(r"\[\+\] Code: (?P<value>\d+) lines")
PYTHON_COMMENT_RE: Final[Pattern[str]] = compile(r"\[\+\] Comments: (?P<value>\d+) lines")
PYTHON_EMPTY_RE: Final[Pattern[str]] = compile(r"\[\+\] Empty: (?P<value>\d+) lines")
PYTHON_LANGUAGE_RE: Final[Pattern[str]] = compile(
	r"\[\+\] Language\(s\) used: (?P<value>\d+) language\(s\)"
)
PYTHON_SIZE_RE: Final[Pattern[str]] = compile(
	r"\[\+\] Total Project Size: (?P<value>[0-9]+(?:\.[0-9]+)?) MB"
)
PYTHON_FINISHED_RE: Final[Pattern[str]] = compile(
	r"\[INFO\] Finished in (?P<value>[0-9]+(?:\.[0-9]+)?)s"
)


@dataclass(frozen = True)
class BenchmarkArgs:
	datasets: 		tuple[DatasetName, ...]
	warmup_runs: 	int
	measured_runs: 	int
	skip_python: 	bool


@dataclass(frozen = True)
class BenchmarkCase:
	name: 				DatasetName
	path: 				Path
	ignore_patterns: 	tuple[str, ...]


@dataclass(frozen = True)
class ArchitectureSummary:
	machine: 						str
	processor_architecture_env: 	str | None
	processor_identifier_env: 		str | None


@dataclass(frozen = True)
class EnvironmentSnapshot:
	platform: 			str
	machine: 			str
	processor: 			str
	cpu_count: 			int | None
	python_version: 	str
	python_executable: 	str
	os_arch_summary: 	ArchitectureSummary

@dataclass(frozen = True)
class BenchmarkSettings:
	datasets: 				tuple[DatasetName, ...]
	warmup_runs: 			int
	measured_runs: 			int
	python_bench_enabled: 	bool


@dataclass(frozen = True)
class OutputSummary:
	code_lines: 					int
	comment_lines: 					int | None
	empty_lines: 					int
	size_bytes: 					int | None
	size_megabytes: 				float | None
	files_scanned: 					int | None
	languages_detected: 			int | None
	finished_seconds_reported: 		float | None


@dataclass(frozen = True)
class CommandStats:
	command: 		str
	runs: 			list[float]
	min_seconds: 	float
	mean_seconds: 	float
	median_seconds: float
	max_seconds: 	float
	stdout_lines: 	list[str]
	stderr_lines: 	list[str]
	summary: 		OutputSummary | None


@dataclass(frozen = True)
class CaseResult:
	dataset: 					DatasetName
	path: 						str
	ignore_patterns: 			tuple[str, ...]
	rust: 						CommandStats
	python: 					CommandStats | None
	relative_speedup_vs_python: float | None


@dataclass(frozen = True)
class BenchmarkReport:
	generated_at_utc: 	str
	environment: 		EnvironmentSnapshot
	settings: 			BenchmarkSettings
	results: 			list[CaseResult]


def parse_args() -> BenchmarkArgs:
	parser = ArgumentParser(
		description="Benchmark Sephera Rust CLI against Sephera Python CLI"
	)
	parser.add_argument(
		"--datasets",
		nargs	=	"+",
		choices	=	DATASET_CHOICES,
		default	=	list(DEFAULT_DATASETS),
		help	=	"Datasets to benchmark",
	)
	parser.add_argument(
		"--warmup",
		type	=	non_negative_int,
		default	=	1,
		help	=	"Warmup runs per command",
	)
	parser.add_argument(
		"--runs",
		type	=	positive_int,
		default	=	5,
		help	=	"Measured runs per command",
	)
	parser.add_argument(
		"--skip-python",
		action	=	"store_true",
		help	=	"Skip Python CLI setup and only benchmark the Rust CLI",
	)
	namespace = parser.parse_args()

	raw_datasets_object: object = namespace.datasets
	if not isinstance(raw_datasets_object, list):
		raise RuntimeError("argparse returned an unexpected dataset payload")

	raw_datasets = cast(list[object], raw_datasets_object)
	dataset_items: list[DatasetName] = []
	for raw_item in raw_datasets:
		if not isinstance(raw_item, str):
			raise RuntimeError("argparse returned a non-string dataset name")
		dataset_items.append(parse_dataset_name(raw_item))

	warmup_runs 	= namespace.warmup
	measured_runs 	= namespace.runs
	skip_python 	= namespace.skip_python
	if not isinstance(warmup_runs, int) or not isinstance(measured_runs, int):
		raise RuntimeError("argparse returned a non-integer benchmark count")

	if not isinstance(skip_python, bool):
		raise RuntimeError("argparse returned a non-boolean skip-python flag")

	datasets: tuple[DatasetName, ...] = tuple(dataset_items)
	return BenchmarkArgs(
		datasets		=	datasets,
		warmup_runs		=	warmup_runs,
		measured_runs	=	measured_runs,
		skip_python		=	skip_python,
	)


def main() -> int:
	args = parse_args()
	REPORTS_DIR.mkdir(parents = True, exist_ok = True)

	build_rust_binaries()
	generate_benchmark_corpus(args.datasets)

	python_cli = None if args.skip_python else prepare_python_cli()
	rust_cli = release_binary("sephera_cli")

	cases = resolve_cases(args.datasets)
	results = [
		benchmark_case(case, rust_cli, python_cli, args.warmup_runs, args.measured_runs)
		for case in cases
	]

	report = BenchmarkReport(
		generated_at_utc		 = datetime.now(timezone.utc).isoformat(),
		environment				 = environment_snapshot(),
		settings				 = BenchmarkSettings(
			datasets			 = args.datasets,
			warmup_runs			 = args.warmup_runs,
			measured_runs		 = args.measured_runs,
			python_bench_enabled = not args.skip_python,
		),
		results = results,
	)

	timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
	json_path = REPORTS_DIR / f"benchmark-{timestamp}.json"
	markdown_path = REPORTS_DIR / f"benchmark-{timestamp}.md"
	json_path.write_text(dumps(asdict(report), indent = 4), encoding="utf-8")
	markdown_path.write_text(render_markdown_report(report), encoding="utf-8")

	print(f"Benchmark report written to {json_path}")
	print(f"Benchmark summary written to {markdown_path}")
	print(render_terminal_summary(results))
	return 0


def build_rust_binaries() -> None:
	run_command(["cargo", "build", "--release", "-p", "sephera_cli", "-p", "sephera_tools"])


def generate_benchmark_corpus(dataset_names: tuple[DatasetName, ...]) -> None:
	synthetic_datasets = [
		dataset_name for dataset_name in dataset_names if dataset_name != "repo"
	]
	if not synthetic_datasets:
		return

	command = [
		str(release_binary("sephera_tools")),
		"generate-benchmark-corpus",
		"--output",
		str(CORPUS_DIR),
		"--clean",
		"--datasets",
		*synthetic_datasets,
	]
	run_command(command)


def prepare_python_cli() -> Path:
	venv_root = ROOT / ".venv"
	venv_python = venv_executable("python")

	if not venv_python.exists():
		try:
			run_command([executable, "-m", "venv", str(venv_root)])

		except RuntimeError:
			rmtree(venv_root, ignore_errors=True)
			run_command([executable, "-m", "venv", "--without-pip", str(venv_root)])

	run_command(
		[
			executable,
			"-m",
			"pip",
			"--python",
			str(venv_python),
			"install",
			"sephera",
		]
	)

	python_cli = venv_executable("sephera")
	if not python_cli.exists():
		raise RuntimeError("The Python Sephera CLI executable was not created in `.venv`.")
	return python_cli


def resolve_cases(dataset_names: tuple[DatasetName, ...]) -> list[BenchmarkCase]:
	available_cases: dict[DatasetName, BenchmarkCase] = {
		"repo": BenchmarkCase("repo", ROOT, REPO_IGNORE_PATTERNS),
		"small": BenchmarkCase("small", CORPUS_DIR / "small", ()),
		"medium": BenchmarkCase("medium", CORPUS_DIR / "medium", ()),
		"large": BenchmarkCase("large", CORPUS_DIR / "large", ()),
		"extra-large": BenchmarkCase("extra-large", CORPUS_DIR / "extra-large", ()),
	}
	return [available_cases[name] for name in dataset_names]

def benchmark_case(
	case: BenchmarkCase,
	rust_cli: Path,
	python_cli: Path | None,
	warmup: int,
	runs: int,
) -> CaseResult:
	if not case.path.exists():
		raise RuntimeError(f"Benchmark dataset does not exist: {case.path}")

	rust_command = loc_command(rust_cli, case)
	rust_stats = measure_command(rust_command, warmup, runs)

	python_stats: CommandStats | None = None
	speedup: float | None = None
	if python_cli is not None:
		python_command = python_loc_command(python_cli, case)
		python_stats = measure_command(python_command, warmup, runs)
		speedup = python_stats.median_seconds / rust_stats.median_seconds

	return CaseResult(
		dataset						= case.name,
		path						= str(case.path),
		ignore_patterns			    = case.ignore_patterns,
		rust						= rust_stats,
		python						= python_stats,
		relative_speedup_vs_python	= speedup,
	)


def loc_command(executable: Path, case: BenchmarkCase) -> list[str]:
	command = [str(executable), "loc", "--path", str(case.path)]
	for pattern in case.ignore_patterns:
		command.extend(["--ignore", pattern])
	return command


def python_loc_command(executable: Path, case: BenchmarkCase) -> list[str]:
	command = [str(executable), "loc", "--path", str(case.path)]
	for pattern in case.ignore_patterns:
		command.extend(["--ignore", pattern])
	return command


def measure_command(command: list[str], warmup: int, runs: int) -> CommandStats:
	last_completed: CompletedProcess[str] | None = None

	for _ in range(warmup):
		last_completed = run_command(command)

	samples: list[float] = []
	for _ in range(runs):
		start = perf_counter()
		last_completed = run_command(command)
		samples.append(perf_counter() - start)

	if last_completed is None:
		raise RuntimeError("No benchmark execution was performed.")

	return CommandStats(
		command			= render_command(command),
		runs			= samples,
		min_seconds		= min(samples),
		mean_seconds	= fmean(samples),
		median_seconds	= median(samples),
		max_seconds		= max(samples),
		stdout_lines	= normalize_output_lines(last_completed.stdout),
		stderr_lines	= normalize_output_lines(last_completed.stderr),
		summary			= parse_output_summary(last_completed.stdout, last_completed.stderr),
	)


def run_command(command: list[str]) -> CompletedProcess[str]:
	completed = run(
		command,
		cwd				= ROOT,
		text			= True,
		capture_output	= True,
		check			= False,
		encoding		= "utf-8",
	)

	if completed.returncode != 0:
		raise RuntimeError(
			f"Command failed: {render_command(command)}\n"
			f"stdout:\n{completed.stdout}\n"
			f"stderr:\n{completed.stderr}"
		)
	return completed


def parse_output_summary(stdout: str, stderr: str) -> OutputSummary | None:
	rust_summary = parse_rust_summary(stdout)
	if rust_summary is not None:
		return rust_summary
	return parse_python_summary(stdout, stderr)


def parse_rust_summary(stdout: str) -> OutputSummary | None:
	totals_match = RUST_TOTALS_RE.search(stdout)
	if totals_match is None:
		return None

	return OutputSummary(
		code_lines					= int(totals_match.group("code")),
		comment_lines				= int(totals_match.group("comment")),
		empty_lines					= int(totals_match.group("empty")),
		size_bytes					= int(totals_match.group("size_bytes")),
		size_megabytes				= None,
		files_scanned				= int(totals_match.group("files_scanned")),
		languages_detected			= int(totals_match.group("languages_detected")),
		finished_seconds_reported	= None,
	)


def parse_python_summary(stdout: str, stderr: str) -> OutputSummary | None:
	code_match = PYTHON_CODE_RE.search(stdout)
	comment_match = PYTHON_COMMENT_RE.search(stdout)
	empty_match = PYTHON_EMPTY_RE.search(stdout)
	language_match = PYTHON_LANGUAGE_RE.search(stdout)
	size_match = PYTHON_SIZE_RE.search(stdout)
	finished_match = PYTHON_FINISHED_RE.search(stdout)
	if finished_match is None:
		finished_match = PYTHON_FINISHED_RE.search(stderr)

	if code_match is None or empty_match is None:
		return None

	comment_lines = int(comment_match.group("value")) if comment_match is not None else None
	size_megabytes = float(size_match.group("value")) if size_match is not None else None
	languages_detected = (
		int(language_match.group("value")) if language_match is not None else None
	)
	finished_seconds_reported = (
		float(finished_match.group("value")) if finished_match is not None else None
	)

	return OutputSummary(
		code_lines					= int(code_match.group("value")),
		comment_lines				= comment_lines,
		empty_lines					= int(empty_match.group("value")),
		size_bytes					= None,
		size_megabytes				= size_megabytes,
		files_scanned			  	= None,
		languages_detected		  	= languages_detected,
		finished_seconds_reported 	= finished_seconds_reported,
	)


def environment_snapshot() -> EnvironmentSnapshot:
	return EnvironmentSnapshot(
		platform				= platform(),
		machine					= machine(),
		processor				= processor(),
		cpu_count				= cpu_count(),
		python_version			= python_version(),
		python_executable		= executable,
		os_arch_summary			= ArchitectureSummary(
			machine						= machine(),
			processor_architecture_env	= environ.get("PROCESSOR_ARCHITECTURE"),
			processor_identifier_env	= environ.get("PROCESSOR_IDENTIFIER"),
		),
	)


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
		f"- Python benchmark enabled: {report.settings.python_bench_enabled}",
		"",
		"## Summary",
		"",
		"| Dataset | Rust min | Rust mean | Rust median | Rust max | Python min | Python mean | Python median | Python max | Rust speedup vs Python |",
		"| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
	]

	for result in report.results:
		python_min = "n/a"
		python_mean = "n/a"
		python_median = "n/a"
		python_max = "n/a"
		speedup = "n/a"
		if result.python is not None:
			python_min = f"{result.python.min_seconds:.6f}"
			python_mean = f"{result.python.mean_seconds:.6f}"
			python_median = f"{result.python.median_seconds:.6f}"
			python_max = f"{result.python.max_seconds:.6f}"
		if result.relative_speedup_vs_python is not None:
			speedup = f"{result.relative_speedup_vs_python:.2f}x"
		lines.append(
			"| "
			f"{result.dataset} | {result.rust.min_seconds:.6f} | {result.rust.mean_seconds:.6f} | "
			f"{result.rust.median_seconds:.6f} | {result.rust.max_seconds:.6f} | {python_min} | "
			f"{python_mean} | {python_median} | {python_max} | {speedup} |"
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
	]
	if result.python is not None:
		lines.append(f"- Python median: `{result.python.median_seconds:.6f}s`")
	if result.relative_speedup_vs_python is not None:
		lines.append(
			f"- Rust speedup vs Python: `{result.relative_speedup_vs_python:.2f}x`"
		)

	lines.extend([
		"",
		"#### Rust",
		"",
		*render_command_details(result.rust),
	])

	if result.python is not None:
		lines.extend([
			"",
			"#### Python",
			"",
			*render_command_details(result.python),
		])

	lines.append("")
	return lines


def render_command_details(stats: CommandStats) -> list[str]:
	lines = [
		f"- Command: `{stats.command}`",
		f"- Runs (s): `{format_run_samples(stats.runs)}`",
		f"- Min/Mean/Median/Max: `{stats.min_seconds:.6f} / {stats.mean_seconds:.6f} / {stats.median_seconds:.6f} / {stats.max_seconds:.6f}`",
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
		f"- Summary code/comment/empty: `{summary.code_lines} / {format_optional_int(summary.comment_lines)} / {summary.empty_lines}`",
		f"- Summary size bytes: `{format_optional_int(summary.size_bytes)}`",
		f"- Summary size MB: `{format_optional_float(summary.size_megabytes)}`",
		f"- Files scanned: `{format_optional_int(summary.files_scanned)}`",
		f"- Languages detected: `{format_optional_int(summary.languages_detected)}`",
		f"- CLI-reported elapsed seconds: `{format_optional_float(summary.finished_seconds_reported)}`",
	]


def render_terminal_summary(results: list[CaseResult]) -> str:
	lines = [
		"Dataset | Rust mean (s) | Rust median (s) | Python mean (s) | Python median (s) | Rust speedup vs Python"
	]
	for result in results:
		python_mean = "n/a"
		python_median = "n/a"
		speedup = "n/a"
		if result.python is not None:
			python_mean = f"{result.python.mean_seconds:.6f}"
			python_median = f"{result.python.median_seconds:.6f}"
		if result.relative_speedup_vs_python is not None:
			speedup = f"{result.relative_speedup_vs_python:.2f}x"
		lines.append(
			f"{result.dataset} | {result.rust.mean_seconds:.6f} | {result.rust.median_seconds:.6f} | "
			f"{python_mean} | {python_median} | {speedup}"
		)
	return "\n".join(lines)


def normalize_output_lines(text: str) -> list[str]:
	stripped = text.strip()

	if not stripped:
		return []
	
	return stripped.splitlines()


def render_command(command: list[str]) -> str:
	if os_name == "nt":
		return list2cmdline(command)
	
	return " ".join(command)


def release_binary(binary_name: str) -> Path:
	suffix = '.exe' if os_name == 'nt' else ''
	return ROOT / 'target' / 'release' / f'{binary_name}{suffix}'


def venv_executable(executable_name: str) -> Path:
	candidates = [
		ROOT / '.venv' / 'Scripts' / f'{executable_name}.exe',
		ROOT / '.venv' / 'Scripts' / executable_name,
		ROOT / '.venv' / 'bin' / executable_name,
	]
	if os_name != 'nt':
		candidates = [
			ROOT / '.venv' / 'bin' / executable_name,
			ROOT / '.venv' / 'Scripts' / f'{executable_name}.exe',
			ROOT / '.venv' / 'Scripts' / executable_name,
		]

	for candidate in candidates:
		if candidate.exists():
			return candidate

	return candidates[0]

def parse_dataset_name(value: str) -> DatasetName:
	if value not in DATASET_CHOICES:
		raise ValueError(f"unsupported dataset name: {value}")
	return value


def non_negative_int(value: str) -> int:
	parsed = int(value)
	if parsed < 0:
		raise ArgumentTypeError("expected a non-negative integer")
	return parsed


def positive_int(value: str) -> int:
	parsed = int(value)
	if parsed <= 0:
		raise ArgumentTypeError("expected a positive integer")
	
	return parsed


def render_ignore_patterns(patterns: tuple[str, ...]) -> str:
	if not patterns:
		return "`none`"
	return ", ".join(f"`{pattern}`" for pattern in patterns)


def format_run_samples(samples: list[float]) -> str:
	return ", ".join(f"{sample:.6f}" for sample in samples)


def format_optional_int(value: int | None) -> str:
	return "n/a" if value is None else str(value)


def format_optional_float(value: float | None) -> str:
	return "n/a" if value is None else f"{value:.6f}"


def format_optional_str(value: str | None) -> str:
	return "n/a" if value is None else value


if __name__ == "__main__":
	raise SystemExit(main())












