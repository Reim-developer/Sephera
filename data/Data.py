from dataclasses import dataclass
from typing import List, Optional, Dict

try:
    from etc.generate.config_data import CONFIG_DATA
except KeyboardInterrupt:
    print("\nAborted by user.")

@dataclass
class CommentStyle:
    single_line: Optional[str] = None
    multi_line_start: Optional[str] = None
    multi_line_end: Optional[str] = None

@dataclass
class LanguageConfig:
    name: str
    extensions: List[str]
    comment_style: str

class LanguageData:
    def __init__(self) -> None:
        config_data = CONFIG_DATA

        self._comment_styles: Dict[str, CommentStyle] = {
            key: CommentStyle(**value) for key, value in config_data["comment_styles"].items()
        }
        self._languages: List[LanguageConfig] = [
            LanguageConfig(
                name = language["name"],
                extensions = language["extension"],
                comment_style = language["comment_styles"]
            ) for language in config_data["languages"]
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
    
    def get_comment_style(self, language: LanguageConfig) -> Optional[CommentStyle]:
        return self._comment_styles.get(language.comment_style)
