import sys
import os 

try:
    from rich.console import Console
    from utils.stdout import SepheraStdout
    from utils.utils import Utils
    from sephera.interactive.confirm import ConfirmInteractive
    from datalyzer.sql import SqlManager
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class SetConfiguration:
    def __init__(self) -> None:
        self.console = Console()
        self.sql = SqlManager()

    def _get_default_cfg(self) -> str:
        YAML_SOURCE = """\
# Auto generate by Sephera
# GitHub: https://github.com/reim-developer/Sephera

# Comment style for your programming language.
comment_styles:
    python_style:
        # Comment of language. If the language 
        # does not support it comment type,
        # you can set this field to null.

        single_line: '#' 
        multi_line_start: '"\"\"\'
        multi_line_end: '"\"\"\'

# Languages extension, and style
languages:
    # Language name.
    - name: Python

      # Language extension.
      extension:
        - .py
      
      # Language comment style.
      comment_styles: python_style
"""
        return YAML_SOURCE
    
    def setup_settings(self, utils: Utils, stdout: SepheraStdout) -> None:
        user_local = utils.get_local_data()
        global_cfg_path = f"{user_local}/.config/Sephera"

        try: 
            self.sql.connect_to_sql(db_path = f"{global_cfg_path}/settings.db")
            self.sql.create_sql_table()

        except Exception as error:
            stdout.die(error = error)

    def create_settings(self, global_cfg_path: str) -> None:
        if not os.path.exists(f"{global_cfg_path}/settings.db"):
            
            if not os.path.exists(global_cfg_path):
                os.makedirs(name = global_cfg_path, exist_ok = True)

            open(f"{global_cfg_path}/settings.db", "w")

    def set_language_cfg(self, stdout: SepheraStdout, cfg_name: str = "SepheraCfg.yml", global_cfg: bool = False) -> None:
        YAML_SOURCE = self._get_default_cfg()
        utils = Utils()

        user_local = utils.get_local_data()
        global_cfg_path = f"{user_local}/.config/Sephera"
        
        self.create_settings(global_cfg_path = global_cfg_path)
        self.setup_settings(utils = utils, stdout = stdout)

        confirm_result = False # Default is FALSE.
        if not global_cfg:
            if os.path.exists(cfg_name):
                confirm_override = ConfirmInteractive()
                confirm_result = confirm_override.override_write_confirm(file_name = cfg_name)

            if not confirm_result:
                sys.exit(0)

            if not cfg_name.endswith(".yml"):
                cfg_name += ".yml"

            try:
                with open(file = cfg_name, mode = "w", encoding = "utf-8") as yaml_cfg:
                    yaml_cfg.write(YAML_SOURCE)

            except Exception as error:
                stdout.die(error = error)

            self.sql.set_user_cfg_path(user_cfg_path = f"{os.getcwd()}")
            self.console.print("\n".join([
                "[cyan][+] Language detection configuration saved successfully.",
                "[cyan][+] Configuration type: User Config",
                f"[cyan][+] Configuration path: {os.path.abspath(cfg_name)}",
                "[cyan][+] Done! This action will also override and update your user configuration."
            ]))
            sys.exit(0)
        
        try:
            os.makedirs(global_cfg_path, exist_ok = True)

            if os.path.exists(f"{global_cfg_path}/{cfg_name}"):
                confirm_override = ConfirmInteractive()
                confirm_result = confirm_override.override_write_confirm(file_name = cfg_name)

                if not confirm_result:
                    sys.exit(0)

            with open(file = f"{global_cfg_path}/{cfg_name}", mode = "w", encoding = "utf-8") as cfg_file:
                cfg_file.write(YAML_SOURCE)
            
            self.sql.set_global_cfg_path(global_cfg_path = f"{global_cfg_path}/{cfg_name}")
            self.console.print("\n".join([
                "[cyan][+] Language detection configuration saved successfully.",
                "[cyan][+] Configuration type: Global Configuration.",
                f"[cyan][+] Configuration path: {os.path.abspath(os.path.join(global_cfg_path, cfg_name))}"
            ]))
            sys.exit(0)
        
        except Exception as error:
            stdout.die(error = error)
