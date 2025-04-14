import yaml
import base64
from dataclasses import dataclass
from typing import List, Optional, Dict

BASE64_CONFIG_YAML = """IyBTZXBoZXJhIFByb2plY3Q6IFByb2dyYW1taW5nIExhbmd1YWdlIENvbmZpZy4KIyBUaGlzIHByb2plY3QgaXMgbGljZW5zZWQgdW5kZXIgdGhlIEdOVSBHZW5lcmFsIFB1YmxpYyBMaWNlbnNlIHYzLjAuCgpjb21tZW50X3N0eWxlczoKICBjX3N0eWxlOgogICAgc2luZ2xlX2xpbmU6ICIvLyIKICAgIG11bHRpX2xpbmVfc3RhcnQ6ICIvKiIKICAgIG11bHRpX2xpbmVfZW5kOiAiKi8iCgogIHB5dGhvbl9zdHlsZToKICAgIHNpbmdsZV9saW5lOiAiIyIKICAgIG11bHRpX2xpbmVfc3RhcnQ6ICciIiInCiAgICBtdWx0aV9saW5lX2VuZDogJyIiIicKICAKICBzaGVsbF9zdHlsZToKICAgIHNpbmdsZV9saW5lOiAiIyIKICAKbGFuZ3VhZ2VzOgogIC0gbmFtZTogUHl0aG9uCiAgICBleHRlbnNpb246CiAgICAgIC0gLnB5CiAgICBjb21tZW50X3N0eWxlczogcHl0aG9uX3N0eWxlCgogIC0gbmFtZTogSmF2YQogICAgZXh0ZW5zaW9uOgogICAgICAtIC5qYXZhCiAgICBjb21tZW50X3N0eWxlczogY19zdHlsZQogIAogIC0gbmFtZTogSmF2YVNjcmlwdAogICAgZXh0ZW5zaW9uOgogICAgICAtIC5qcwogICAgICAtIC5qc3gKICAgICAgLSAubWpzCiAgICBjb21tZW50X3N0eWxlczogY19zdHlsZQoKICAtIG5hbWU6IFNoZWxsIFNjcmlwdAogICAgZXh0ZW5zaW9uOgogICAgICAtIC5zaAogICAgY29tbWVudF9zdHlsZXM6IHNoZWxsX3N0eWxlCgogIC0gbmFtZTogQysrCiAgICBleHRlbnNpb246CiAgICAgIC0gLmNjCiAgICAgIC0gLmNwcAogICAgICAtIC5jeHgKICAgICAgLSAuYysrCiAgICBjb21tZW50X3N0eWxlczogY19zdHlsZQoKICAtIG5hbWU6IEdvbGFuZwogICAgZXh0ZW5zaW9uOgogICAgICAtIC5nbwogICAgY29tbWVudF9zdHlsZXM6IGNfc3R5bGUKCiAgLSBuYW1lOiBQZXJsCiAgICBleHRlbnNpb246CiAgICAgIC0gLnBsCiAgICBjb21tZW50X3N0eWxlczogc2hlbGxfc3R5bGUKCiAgLSBuYW1lOiBSdWJ5CiAgICBleHRlbnNpb246CiAgICAtIC5yYgogICAgY29tbWVudF9zdHlsZXM6IHNoZWxsX3N0eWxlCgogIC0gbmFtZTogQyBIZWFkZXIgRmlsZQogICAgZXh0ZW5zaW9uOgogICAgLSAuaAogICAgY29tbWVudF9zdHlsZXM6IGNfc3R5bGUKICAKICAtIG5hbWU6IEMrKyBIZWFkZXIgRmlsZQogICAgZXh0ZW5zaW9uOgogICAgLSAuaHBwCiAgICAtIC5oaAogICAgLSAuaCsrCiAgICBjb21tZW50X3N0eWxlczogY19zdHlsZQoKICAtIG5hbWU6IEMjCiAgICBleHRlbnNpb246CiAgICAtIC5jcwogICAgY29tbWVudF9zdHlsZXM6IGNfc3R5bGU="""

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
        yaml_source = base64.b64decode(BASE64_CONFIG_YAML).decode()
        config_data = yaml.safe_load(yaml_source)

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
