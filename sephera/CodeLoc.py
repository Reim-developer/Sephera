import os
import re
import sys
from typing import Dict, Optional, Tuple, List

try:
    import mmap
    from data.Data import LanguageData, LanguageConfig, CommentStyle
    from utils.utils import Utils
    from utils.stdout import SepheraStdout
    from rich.console import Console
    from rich.table import Table
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class CodeLoc:
    def __init__(self, base_path: str = ".", ignore_pattern: Optional[List[str]] = None) -> None:
        self.language_data = LanguageData()
        self.utils = Utils()
        self.base_path = base_path
        self.languages = self.language_data.get_languages

        self.ignore_regex: List[re.Pattern[str]] = []
        self.ignore_str: List[str] = []
        self.ignore_glob: List[str] = []

        self.stdout = SepheraStdout()
        self.console = Console()

        self.loc_count = None # type: ignore

        if ignore_pattern:
            for pattern in ignore_pattern:
                try:
                    self.ignore_regex.append(re.compile(pattern = pattern))
                
                except re.error:
                    if any(char in pattern for char in "*?[]"):
                        self.ignore_glob.append(pattern)
                    else:
                        self.ignore_str.append(pattern)
        
        self.count_loc()

    def _get_language_for_file(self, path: str) -> Optional[LanguageConfig]:
        for language in self.languages:
            if any(path.endswith(extension) for extension in language.extensions):
                return language
            
        return None

    def _count_lines_in_file(self, file_path: str, language: LanguageConfig) -> Tuple[int, int, int]:
        loc_line_count: int = 0
        comment_line_count: int = 0
        empty_line_count: int = 0
        comment_nesting_level: int = 0  
        
        comment_style: Optional[CommentStyle] = self.language_data.get_comment_style(language = language)

        if os.path.getsize(file_path) == 0:
            return 0, 0, 0
         
        try:
            with open(file = file_path, mode = "r", encoding = None) as file:
                with mmap.mmap(file.fileno(), 0, access = mmap.ACCESS_READ) as memory_map:

                    for line in iter(memory_map.readline, b""):
                        line = line.decode(encoding = "utf-8").strip()

                        if not line:
                            empty_line_count += 1
                            continue
                        
                        
                        if comment_nesting_level == 0 and comment_style.single_line and line.startswith(comment_style.single_line): # type: ignore
                            comment_line_count += 1
                            continue

                        if comment_nesting_level > 0:
                            comment_line_count += 1
                            current_line = line

                            while (comment_style.multi_line_start in current_line or # type: ignore
                                comment_style.multi_line_end in current_line): # type: ignore
                                start_idx = current_line.find(comment_style.multi_line_start) # type: ignore
                                end_idx = current_line.find(comment_style.multi_line_end) # type: ignore

                                if start_idx != -1 and (end_idx == -1 or start_idx < end_idx):
                                    comment_nesting_level += 1
                                    current_line = current_line[start_idx + len(comment_style.multi_line_start):] # type: ignore

                                elif end_idx != -1:
                                    comment_nesting_level -= 1
                                    current_line = current_line[end_idx + len(comment_style.multi_line_end):] # type: ignore

                                else:
                                    break
                            continue

                        
                        if comment_style.multi_line_start and comment_style.multi_line_start in line: # type: ignore
                            
                            start_pos = line.find(comment_style.multi_line_start) # type: ignore
                            code_before_comment = line[:start_pos].strip()
                            
                            
                            if code_before_comment:
                                loc_line_count += 1
                                
                                remaining_line = line[start_pos + len(comment_style.multi_line_start):] # type: ignore
                                
                                if comment_style.multi_line_end in remaining_line: # type: ignore
                                    pass

                                else:
                                    comment_nesting_level = 1

                            else:
                            
                                comment_line_count += 1
                                
                                current_line = line[start_pos + len(comment_style.multi_line_start):] # type: ignore

                                while (comment_style.multi_line_start in current_line or # type: ignore
                                    comment_style.multi_line_end in current_line): # type: ignore

                                    start_idx = current_line.find(comment_style.multi_line_start) # type: ignore
                                    end_idx = current_line.find(comment_style.multi_line_end) # type: ignore

                                    if start_idx != -1 and (end_idx == -1 or start_idx < end_idx):
                                        comment_nesting_level += 1
                                        current_line = current_line[start_idx + len(comment_style.multi_line_start):] # type: ignore

                                    elif end_idx != -1:
                                        comment_nesting_level -= 1
                                        current_line = current_line[end_idx + len(comment_style.multi_line_end):] # type: ignore

                                    else:
                                        break
                                        
                                if comment_style.multi_line_end not in line: # type: ignore
                                    comment_nesting_level = 1
                            continue

                        loc_line_count += 1

        except Exception as error:
            self.stdout.die(error = error)

        return loc_line_count, comment_line_count, empty_line_count

    def count_loc(self) -> Dict[str, Dict[str, float]]:
        if self.loc_count is None: # type: ignore

            self.loc_count: Dict[str, Dict[str, float]] = {
                language.name: {"loc": 0, "comment": 0, "empty": 0, "size": 0.0}
                for language in self.languages
            }
            self.loc_count["Unknown"] = {"loc": 0, "comment": 0, "empty": 0, "size": 0.0}

            for root, dirs, files in os.walk(self.base_path):
                dirs[:] = [dir for dir in dirs if not 
                                self.utils.is_multi_ignored(
                                    path = os.path.join(root, dir), 
                                    ignore_regex = self.ignore_regex, 
                                    ignore_str = self.ignore_str,
                                    ignore_glob = self.ignore_glob
                        )]

                for file in files:
                    file_path = os.path.join(root, file)

                    if self.utils.is_multi_ignored(
                        path = file_path, ignore_str = self.ignore_str, 
                        ignore_regex = self.ignore_regex, ignore_glob = self.ignore_glob):
                        continue

                    language = self._get_language_for_file(path = file_path)

                    if language:
                        loc_line, comment_line, empty_line = self._count_lines_in_file(file_path = file_path, language = language)

                        try:
                            file_sizeof = os.path.getsize(file_path) / (1024 * 1024)
                        except OSError:
                            file_sizeof = 0.0

                        self.loc_count[language.name]["loc"] += loc_line
                        self.loc_count[language.name]["comment"] += comment_line
                        self.loc_count[language.name]["empty"] += empty_line
                        self.loc_count[language.name]["size"] += file_sizeof

                    else:
                        self.loc_count["Unknown"]["loc"] += 0
                        self.loc_count["Unknown"]["comment"] += 0
                        self.loc_count["Unknown"]["empty"] += 0
                        self.loc_count["Unknown"]["size"] += 0.0

        return self.loc_count

    def stdout_result(self) -> None:
        table = Table(title = f"LOC count of directory: {os.path.abspath(self.base_path)}")
        table.add_column("Language", style = "cyan")
        table.add_column("Code lines", justify = "right", style = "green")
        table.add_column("Comments lines", justify = "right", style = "yellow")
        table.add_column("Empty lines", justify = "right", style = "white")
        table.add_column("Size (MB)", justify = "right", style = "magenta")

        total_loc_count: int = 0
        total_comment: int = 0
        total_empty: int = 0
        total_project_size: float = 0.0
        language_count: int = 0

        for language, count in self.loc_count.items():
            loc_line = count["loc"]
            comment_line = count["comment"]
            empty_line = count["empty"]
            total_sizeof = count["size"]

            if loc_line > 0 or comment_line > 0 or empty_line > 0 or total_sizeof > 0:

                language_count += 1
    
                language_config  = self.language_data.get_language_by_name(name = language)
                comment_result = (
                    "N/A" if language_config.comment_style == "no_comment" # type: ignore
                    else str(comment_line)
                )

                table.add_row(
                    language, str(loc_line),
                    comment_result, str(empty_line),
                    f"{total_sizeof:.2f}"
                )
                total_loc_count += loc_line # type: ignore
                total_comment += comment_line # type: ignore
                total_empty += empty_line # type: ignore
                total_project_size += total_sizeof
        
        self.console.print(table)
        self.stdout.show_msg("\n".join([
            f"[+] Code: {total_loc_count} lines",
            f"[+] Comments: {total_comment} lines",
            f"[+] Empty: {total_empty} lines",
            f"[+] Language(s) used: {language_count} language(s)",
            f"[+] Total Project Size: {total_project_size:.2f} MB"
        ]))

        