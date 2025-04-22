import sys
import os

try:
    from rich.console import Console
    from sephera.CodeLoc import CodeLoc
except KeyboardInterrupt:
    print("\n Aborted by user.")

class SepheraInteractive:
    def __init__(self) -> None:
        self.console = Console()
    
    def _code_loc_exist_file_handler(self, json_file: str, base_path: str = ".") -> None:
        codeLoc = CodeLoc(base_path = base_path)
            
        while True:
            self.console.print("\n".join([
                    f"[yellow][!] This file {json_file}.json already exists. Do you want:",
                    f"[cyan][1] Replace and override all data in {json_file}.",
                    f"[cyan][2] Save {json_file} to other directory path.",
                    "[cyan][3] Change file name before save.",
                    "[cyan][4] Cancel & exit."
            ]))
            user_option: str = input("Your option: ").strip()

            match user_option:
                case "1":
                        codeLoc.export_to_json(file_path = json_file)
                        self.console.print("\n".join([
                            "[cyan][+] Export to JSON file successfully.",
                        f"[cyan][+] With name: {json_file}",
                        f"[cyan][+] Save as: {os.path.abspath(json_file)}"
                    ]))
                        sys.exit(0)
                        
                case "2":
                    pass

                case "3":
                    pass

                case "4":
                    sys.exit(0)


    def _code_loc_handler(self, base_path: str = ".") -> None:
        while True:
            self.console.print("\n".join([
                "[cyan][+] Please input JSON file name to export.",
                "[cyan][+] Leave blank if you want create with default name."
            ]))
            user_option: str = input("Input JSON file name: ").replace(" ", "_")

            if not user_option.endswith(".json"):
                user_option += ".json"

            if user_option == ".json":
                sys.exit(0)

            if os.path.exists(path = user_option):
                try:
                    self._code_loc_exist_file_handler(json_file = user_option, base_path = base_path)

                except KeyboardInterrupt:
                    self.console.print("\n[cyan][+] Aborted by user.")    

            codeLoc = CodeLoc()
            codeLoc.export_to_json(file_path = user_option)
            self.console.print("\n".join([
                "[cyan][+] Export to JSON file successfully.",
                f"[cyan][+] With name: {user_option}",
                f"[cyan][+] Save as: {os.path.abspath(user_option)}"
            ]))
            sys.exit(0)

    def codeloc_interactive(self, base_path: str = ".") -> None:
        while True:
            self.console.print("\n".join([
                "[cyan][+] Missing value of --export flag. Do you want:",
                "[yellow][1] Export LOC count to JSON file format.",
                "[yellow][2] Cancel, and exit."
            ]))

            user_option: str = input("Your option [1, 2]: ").strip()

            match user_option:
                case "1":
                    try:
                        self._code_loc_handler()
                    
                    except KeyboardInterrupt:
                        self.console.print("\n[cyan][+] Aborted by user.")
                        sys.exit()

                case "2":
                    sys.exit(0)

                case _:
                    self.console.print(f"[red] Invalid option: {user_option}")