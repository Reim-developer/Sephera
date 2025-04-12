import os
import re
from typing import Optional

try:
    from rich.console import Console
    from chart.Exporter import Exporter
    from rich.table import Table
    from rich.progress import Progress, SpinnerColumn, TextColumn, TimeElapsedColumn
except KeyboardInterrupt:
    print("\n Aborted by user.")

class Stats:
    def __init__(self, base_path: str = ".", ignore_pattern: Optional[str] = None) -> None:
        self.base_path = base_path
        self.ignore_regex: Optional[re.Pattern] = None
        self.ignore_str: Optional[str] = None

        if ignore_pattern:
            try:
                self.ignore_regex = re.compile(ignore_pattern)
            except re.error:
                self.ignore_str = ignore_pattern

    def _is_ignored(self, path: str) -> None:
        if self.ignore_regex:
            return bool(self.ignore_regex.search(path))
        
        if self.ignore_str:
            return self.ignore_str in path
        
        return False

    def _is_hidden_path(self, path: str, base_path: str) -> None:
        rel_path = os.path.relpath(path, base_path)
        parts = rel_path.split(os.sep)

        return any(part.startswith(".") for part in parts)

    def stats_all_files(self, output_chart: str = None) -> None:
        file_count: int = 0
        folder_count: int = 0
        total_size: int = 0

        hidden_file_count: int = 0
        hidden_folder_count: int = 0
        total_hidden_size: int = 0
        
        console = Console()

        with Progress(
            SpinnerColumn(), TextColumn("[progress.description]{task.description}"),
            TimeElapsedColumn(), console = console,
            transient = True
            ) as progressBar:
            progressBar.add_task("Processing...", total = None)

            for root, dirs, files in os.walk(self.base_path):
                dirs[:] = [dir for dir in dirs if not self._is_ignored(os.path.join(root, dir))]

                for dir in dirs:
                    full_dir_path = os.path.join(root, dir)

                    if self._is_hidden_path(full_dir_path, self.base_path):
                        hidden_folder_count += 1
                    folder_count += 1
                
                for file in files:
                    file_count += 1
                    full_path = os.path.join(root, file)

                    try:
                        size = os.path.getsize(full_path)
                        total_size += size

                        if self._is_hidden_path(full_path, self.base_path):
                            hidden_file_count += 1
                            total_hidden_size += size

                    except Exception:
                        pass
        
        data: dict = {
            "Folder": folder_count,
            "File": file_count,
            "Hidden Folder": hidden_folder_count,
            "Hidden File": hidden_file_count
        }
        self._stdout_stats(data = data)
        exporter = Exporter(output_path = output_chart)

        print(f"[+] Total Size: {total_size / (1024 ** 2):.2f} MB")
        print(f"[+] Total Hidden Size: {total_hidden_size / (1024 ** 2):.2f} MB")

        if(output_chart):
            exporter.export_stats_chart(data = data, total_size = total_size, total_hidden_size = total_hidden_size)
            print(f"[+] Saved chart as name: {output_chart}")
        
    
    def _stdout_stats(self, data: dict) -> None:
        console = Console()
        total = sum(data.values())
        table = Table(title = "Sephera Stats Overview", show_header = True, header_style = "bold magenta")

        table.add_column("Category")
        table.add_column("Count", justify = "right")
        table.add_column("Percent", justify = "right")

        for key, value in data.items():
            percent = (value / total) * 100 if total else 0
            table.add_row(str(key), str(value), f"{percent:.1f}%")

        console.print(table)
