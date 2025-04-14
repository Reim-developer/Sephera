# sephera/CodeLoc.py
import os
from typing import Dict, Optional, Tuple
from data.Data import LanguageData, LanguageConfig, CommentStyle

class CodeLoc:
    def __init__(self) -> None:
        self.language_data = LanguageData()
        self.languages = self.language_data.get_languages

    def _get_language_for_file(self, file_path: str) -> Optional[LanguageConfig]:
        for language in self.languages:
            if any(file_path.endswith(extension) for extension in language.extensions):
                return language
        return None

    def _count_lines_in_file(self, file_path: str, language: LanguageConfig) -> Tuple[int, int, int]:
        loc_line_count: int = 0
        comment_line_count: int = 0
        empty_line_count: int = 0
        in_multi_line_comment: bool = False

        comment_style: Optional[CommentStyle] = self.language_data.get_comment_style(language = language)

        try:
            with open(file = file_path, mode = "r", encoding = "utf-8") as file:
                for line in file:
                    line = line.strip()
                    if not line:
                        empty_line_count += 1
                        continue

                    if comment_style.single_line and line.startswith(comment_style.single_line):
                        comment_line_count += 1
                        continue

                    if comment_style.multi_line_start and comment_style.multi_line_end:
                        if in_multi_line_comment:
                            comment_line_count += 1

                            if comment_style.multi_line_end in line:
                                in_multi_line_comment = False
                            continue

                        if line.startswith(comment_style.multi_line_start):
                            comment_line_count += 1

                            if comment_style.multi_line_end in line[line.find(comment_style.multi_line_start) + len(comment_style.multi_line_start):]:
                                continue

                            in_multi_line_comment = True
                            continue

                    loc_line_count += 1
        except UnicodeDecodeError:
            print(f"Error when read: {file_path}. That's not text file, skipping...")
        except Exception as e:
            print(f"Exception: '{e}' when read: {file_path}, skipping...")

        return loc_line_count, comment_line_count, empty_line_count

    def count_loc(self, directory: str) -> Dict[str, Dict[str, int]]:
        result: Dict[str, Dict[str, int]] = {
            language.name: {"loc": 0, "comment": 0, "empty": 0}
            for language in self.languages
        }
        result["Unknown"] = {"loc": 0, "comment": 0, "empty": 0}

        for root, _, files in os.walk(directory):
            for file in files:
                file_path = os.path.join(root, file)
                language = self._get_language_for_file(file_path = file_path)

                if language:
                    loc_line, comment_line, empty_line = self._count_lines_in_file(file_path = file_path, language=language)
                    result[language.name]["loc"] += loc_line
                    result[language.name]["comment"] += comment_line
                    result[language.name]["empty"] += empty_line
                else:
                    result["Unknown"]["loc"] += 0
                    result["Unknown"]["comment"] += 0
                    result["Unknown"]["empty"] += 0

        return result

    def stdout_result(self, directory: str) -> None:
        loc_count = self.count_loc(directory = directory)
        print(f"LOC count of directory: {directory}")
        print("-" * 50)
        total_loc_count: int = 0
        total_comment: int = 0
        total_empty: int = 0

        for language, count in loc_count.items():
            loc_line = count["loc"]
            comment_line = count["comment"]
            empty_line = count["empty"]

            if loc_line > 0 or comment_line > 0 or empty_line > 0:
                print(f"Language: {language}")
                print(f"  Code: {loc_line} lines")
                print(f"  Comments: {comment_line} lines")
                print(f"  Empty: {empty_line} lines")
                print("-" * 50)
                
                total_loc_count += loc_line
                total_comment += comment_line
                total_empty += empty_line
                
        print(f"Total:")
        print(f"  Code: {total_loc_count} lines")
        print(f"  Comments: {total_comment} lines")
        print(f"  Empty: {total_empty} lines")
