import yaml
import base64
from dataclasses import dataclass
from typing import List, Optional, Dict

BASE64_CONFIG_YAML = """IyBTZXBoZXJhIFByb2plY3Q6IFByb2dyYW1taW5nIExhbmd1YWdlIENvbmZpZy4KIyBUaGlzIHByb2plY3QgaXMgbGljZW5zZWQgdW5kZXIgdGhlIEdOVSBHZW5lcmFsIFB1YmxpYyBMaWNlbnNlIHYzLjAuCgojIENvbW1lbnQgU3R5bGVzIGZvciBwcm9ncmFtbWluZyBsYW5ndWFnZXMuCmNvbW1lbnRfc3R5bGVzOgogIGNfc3R5bGU6CiAgICBzaW5nbGVfbGluZTogIi8vIgogICAgbXVsdGlfbGluZV9zdGFydDogIi8qIgogICAgbXVsdGlfbGluZV9lbmQ6ICIqLyIKCiAgcHl0aG9uX3N0eWxlOgogICAgc2luZ2xlX2xpbmU6ICIjIgogICAgbXVsdGlfbGluZV9zdGFydDogJyIiIicKICAgIG11bHRpX2xpbmVfZW5kOiAnIiIiJwogIAogIHNoZWxsX3N0eWxlOgogICAgc2luZ2xlX2xpbmU6ICIjIgoKICBwZXJsX3N0eWxlOgogICAgc2luZ2xlX2xpbmU6ICIjIgogICAgbXVsdGlfbGluZV9zdGFydDogIj0iCiAgICBtdWx0aV9saW5lX2VuZDogIj1jdXQiCgogIHJ1Ynlfc3R5bGU6CiAgICBzaW5nbGVfbGluZTogIiMiCiAgICBtdWx0aV9saW5lX3N0YXJ0OiAiPWJlZ2luIgogICAgbXVsdGlfbGluZV9lbmQ6ICI9ZW5kIgogIAogIG5vX2NvbW1lbnQ6CiAgICBzaW5nbGVfbGluZTogbnVsbAogICAgbXVsdGlfbGluZV9zdGFydDogbnVsbAogICAgbXVsdGlfbGluZV9lbmQ6IG51bGwKCiAgaHRtbF9zdHlsZToKICAgIHNpbmdsZV9saW5lOiBudWxsCiAgICBtdWx0aV9saW5lX3N0YXJ0OiAiPCEtLSIKICAgIG11bHRpX2xpbmVfZW5kOiAiLS0+IgoKIyBMYW5ndWFnZXMgZXh0ZW5zaW9uLCBuYW1lLCBhbmQgY29tbWVudCBzdHlsZSBjb25maWd1cmF0aW9uLgpsYW5ndWFnZXM6CiAgLSBuYW1lOiBQeXRob24KICAgIGV4dGVuc2lvbjoKICAgICAgLSAucHkKICAgIGNvbW1lbnRfc3R5bGVzOiBweXRob25fc3R5bGUKCiAgLSBuYW1lOiBKYXZhCiAgICBleHRlbnNpb246CiAgICAgIC0gLmphdmEKICAgIGNvbW1lbnRfc3R5bGVzOiBjX3N0eWxlCiAgCiAgLSBuYW1lOiBKYXZhU2NyaXB0CiAgICBleHRlbnNpb246CiAgICAgIC0gLmpzCiAgICAgIC0gLmpzeAogICAgICAtIC5tanMKICAgIGNvbW1lbnRfc3R5bGVzOiBjX3N0eWxlCgogIC0gbmFtZTogU2hlbGwgU2NyaXB0CiAgICBleHRlbnNpb246CiAgICAgIC0gLnNoCiAgICBjb21tZW50X3N0eWxlczogc2hlbGxfc3R5bGUKCiAgLSBuYW1lOiBDKysKICAgIGV4dGVuc2lvbjoKICAgICAgLSAuY2MKICAgICAgLSAuY3BwCiAgICAgIC0gLmN4eAogICAgICAtIC5jKysKICAgIGNvbW1lbnRfc3R5bGVzOiBjX3N0eWxlCgogIC0gbmFtZTogR29sYW5nCiAgICBleHRlbnNpb246CiAgICAgIC0gLmdvCiAgICBjb21tZW50X3N0eWxlczogY19zdHlsZQoKICAtIG5hbWU6IFBlcmwKICAgIGV4dGVuc2lvbjoKICAgICAgLSAucGwKICAgIGNvbW1lbnRfc3R5bGVzOiBwZXJsX3N0eWxlCgogIC0gbmFtZTogUnVieQogICAgZXh0ZW5zaW9uOgogICAgLSAucmIKICAgIGNvbW1lbnRfc3R5bGVzOiBydWJ5X3N0eWxlCgogIC0gbmFtZTogQyBIZWFkZXIgRmlsZQogICAgZXh0ZW5zaW9uOgogICAgLSAuaAogICAgY29tbWVudF9zdHlsZXM6IGNfc3R5bGUKICAKICAtIG5hbWU6IEMrKyBIZWFkZXIgRmlsZQogICAgZXh0ZW5zaW9uOgogICAgLSAuaHBwCiAgICAtIC5oaAogICAgLSAuaCsrCiAgICBjb21tZW50X3N0eWxlczogY19zdHlsZQoKICAtIG5hbWU6IEMjCiAgICBleHRlbnNpb246CiAgICAtIC5jcwogICAgY29tbWVudF9zdHlsZXM6IGNfc3R5bGUKCiAgLSBuYW1lOiBUeXBlU2NyaXB0CiAgICBleHRlbnNpb246CiAgICAtIC50cwogICAgLSAudHN4CiAgICBjb21tZW50X3N0eWxlczogY19zdHlsZQogIAogIC0gbmFtZTogUnVzdAogICAgZXh0ZW5zaW9uOgogICAgLSAucnMKICAgIGNvbW1lbnRfc3R5bGVzOiBjX3N0eWxlCgogIC0gbmFtZTogUEhQCiAgICBleHRlbnNpb246IAogICAgLSAucGhwCiAgICBjb21tZW50X3N0eWxlczogY19zdHlsZQoKICAtIG5hbWU6IFlBTUwKICAgIGV4dGVuc2lvbjoKICAgIC0gLnltbAogICAgLSAueWFtbAogICAgY29tbWVudF9zdHlsZXM6IHNoZWxsX3N0eWxlCgogIC0gbmFtZTogSlNPTgogICAgZXh0ZW5zaW9uOgogICAgLSAuanNvbiAKICAgIGNvbW1lbnRfc3R5bGVzOiBub19jb21tZW50CgogIC0gbmFtZTogQ3l0aG9uCiAgICBleHRlbnNpb246CiAgICAtIC5weXgKICAgIC0gLnB4ZAogICAgLSAucHhpCiAgICBjb21tZW50X3N0eWxlczogcHl0aG9uX3N0eWxlCgogIC0gbmFtZTogQ1NTCiAgICBleHRlbnNpb246CiAgICAtIC5jc3MKICAgIGNvbW1lbnRfc3R5bGVzOiBjX3N0eWxlCiAgCiAgLSBuYW1lOiBIVE1MCiAgICBleHRlbnNpb246CiAgICAtIC5odG1sCiAgICAtIC5odG0gCiAgICBjb21tZW50X3N0eWxlczogaHRtbF9zdHlsZQoKICAtIG5hbWU6IFhNTAogICAgZXh0ZW5zaW9uOgogICAgLSAueG1sCiAgICBjb21tZW50X3N0eWxlczogaHRtbF9zdHlsZQoKICAtIG5hbWU6IERhcnQKICAgIGV4dGVuc2lvbjoKICAgIC0gLmRhcnQKICAgIGNvbW1lbnRfc3R5bGVzOiBjX3N0eWxlCiAgICA="""

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
