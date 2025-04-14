from dataclasses import dataclass
from typing import List, Optional, Dict

@dataclass
class CommentStyle:
    single_line: Optional[str] = None
    multi_line_start: Optional[str] = None
    multi_line_end: Optional[str] = None

@dataclass
class LanguageConfig:
    name: str
    extensions: List[str]
    comment_style: CommentStyle


class LanguageData:
    def __init__(self) -> None:
        self._comment_styles: Dict[str, CommentStyle] = {
            "c_style": CommentStyle(
                single_line = "//", 
                multi_line_start = "/*", multi_line_end = "*/"
            ),
            "python_style": CommentStyle(
                single_line = "#", 
                multi_line_start = '"""', multi_line_end = '"""'
            ),
            "shell_style": CommentStyle(
                single_line = "#"
            )
        }

        self._languages: List[LanguageConfig] = [
            LanguageConfig(
                name = "Python",
                extensions = [".py"],
                comment_style = self._comment_styles["python_style"]
            ),
            LanguageConfig(
                name = "Java",
                extensions = [".java"],
                comment_style = self._comment_styles["c_style"]
            ),
            LanguageConfig(
                name = "C++",
                extensions = [".cc", ".cxx", ".cpp", ".c++"],
                comment_style = self._comment_styles["c_style"]
            ),
            LanguageConfig(
                name = "JavaScript",
                extensions = [".js"],
                comment_style = self._comment_styles["c_style"]
            ),
            LanguageConfig(
                name = "Shell Script",
                extensions = [".sh"],
                comment_style = self._comment_styles["shell_style"]
            )
        ]

    @property
    def get_languages(self) -> List[LanguageConfig]:
        return self._languages
    
    @property
    def get_comment_styles(self) -> Dict[str, CommentStyle]:
        return self._comment_styles
    
    def get_language_by_name(self, name: str) -> Optional[LanguageConfig]:
        for language in self._languages:
            if language.name.lower() == name.lower():
                return language
        return None

    def get_language_by_extension(self, extension: str) -> Optional[LanguageConfig]:
        for language in self._languages:
            if extension in language.extensions:
                return language
        
        return None
