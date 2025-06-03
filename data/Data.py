import os
import sys
import logging
from dataclasses import dataclass
from typing import List, Optional, Dict

try:
    import yaml
    from etc.generate.config_data import CONFIG_DATA
    from datalyzer.sql import SqlManager
    from utils.utils import Utils
    from utils.stdout import SepheraStdout
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

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
        logging.basicConfig(level = logging.CRITICAL, format = "%(levelname)s - %(message)s")
        utils = Utils()
        sql = SqlManager()

        user_home = utils.get_local_data()

        if os.path.exists(f"{user_home}/.config/Sephera"):

            sql.connect_to_sql(db_path = f"{user_home}/.config/Sephera/settings.db")
            cfg_path = sql.get_cfg_path()

            if cfg_path:
                if os.path.exists(cfg_path):
                    with open(file = cfg_path, mode = "r", encoding = "utf-8") as cfg_file:
                        try:
                            config_data = yaml.safe_load(cfg_file)
                        
                        except Exception as error:
                            stdout = SepheraStdout()
                            stdout.die(error = error)
                else:
                    config_data = CONFIG_DATA

        else:
            config_data = CONFIG_DATA

       
        self._comment_styles: Dict[str, CommentStyle] = {
                key: CommentStyle(**value) for key, value in config_data["comment_styles"].items() # type: ignore
            }
        
        try:
            self._languages: List[LanguageConfig] = [
                    LanguageConfig(
                        name = language["name"], # type: ignore
                        extensions = language["extension"], # type: ignore
                        comment_style = language["comment_styles"] # type: ignore
                    ) for language in config_data["languages"] # type: ignore
                ]
        except Exception as error:
            logging.critical(f"Required value: {error} not found in your YAML configuration.")
            sys.exit(1)
      
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
