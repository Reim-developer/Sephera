from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Literal, TypeAlias

DatasetName: TypeAlias = Literal["repo", "small", "medium", "large", "extra-large"]


@dataclass(frozen = True)
class BenchmarkArgs:
	datasets: tuple[DatasetName, ...]
	warmup_runs: int
	measured_runs: int


@dataclass(frozen = True)
class BenchmarkCase:
	name: DatasetName
	path: Path
	ignore_patterns: tuple[str, ...]


@dataclass(frozen = True)
class ArchitectureSummary:
	machine: str
	processor_architecture_env: str | None
	processor_identifier_env: str | None


@dataclass(frozen = True)
class EnvironmentSnapshot:
	platform: str
	machine: str
	processor: str
	cpu_count: int | None
	python_version: str
	python_executable: str
	os_arch_summary: ArchitectureSummary


@dataclass(frozen = True)
class BenchmarkSettings:
	datasets: tuple[DatasetName, ...]
	warmup_runs: int
	measured_runs: int


@dataclass(frozen = True)
class OutputSummary:
	code_lines: int
	comment_lines: int
	empty_lines: int
	size_bytes: int
	files_scanned: int
	languages_detected: int


@dataclass(frozen = True)
class CommandStats:
	command: str
	runs: list[float]
	min_seconds: float
	mean_seconds: float
	median_seconds: float
	max_seconds: float
	stdout_lines: list[str]
	stderr_lines: list[str]
	summary: OutputSummary | None


@dataclass(frozen = True)
class CaseResult:
	dataset: DatasetName
	path: str
	ignore_patterns: tuple[str, ...]
	rust: CommandStats


@dataclass(frozen = True)
class BenchmarkReport:
	generated_at_utc: str
	environment: EnvironmentSnapshot
	settings: BenchmarkSettings
	results: list[CaseResult]
