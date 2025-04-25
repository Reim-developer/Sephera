import sys 
import os
import logging
import time

try:
    from rich.console import Console
    from sephera.CodeLoc import CodeLoc
    from sephera.interactive.confirm import ConfirmInteractive
    from datalyzer.Exporter import Exporter
    from utils.stdout import SepheraStdout
except KeyboardInterrupt:
    print("\n Aborted by user.")

class OptionHandler:
    def __init__(self) -> None:
        self.console = Console()
        self.confirmInteractive = ConfirmInteractive()
        self.stdout = SepheraStdout()

        logging.basicConfig(level = logging.INFO, format = "[%(levelname)s] %(message)s")

    def on_choose_dir_path(self) -> str:
        try:
            self.console.print("[yellow][+] Please input your directory path:")
            self.console.print("[cyan][+] If you want cancel, just type 'exit'.")
            while True:
                option = input(">>> ")

                if not option:
                    self.console.print("[yellow][!] Directory path is empty.")

                elif option == "exit":
                    sys.exit(0)

                elif not os.path.exists(option):
                    self.console.print(f"[yellow][!] Directory path {option} not found.")
             
                else:
                    return option
            
        except KeyboardInterrupt:
            self.console.print("\n[cyan][+] Aborted by user.")
            sys.exit(1)

    def on_markdown_export_option(self, args: str) -> None:
        start_time = time.perf_counter()
        with self.console.status(status = "Processing...", spinner = "material"):
            codeLoc = CodeLoc(args.path, args.ignore)
        
        end_time = time.perf_counter()
        self.console.clear()

        if not args.md.endswith(".md"):
            args.md += ".md"
        
        exporter = Exporter(args)
        if os.path.exists(args.md):
            confirm_override: bool = self.confirmInteractive.override_write_confirm(file_name = args.md)

            if not confirm_override:
                sys.exit(0)

            if confirm_override:
                exporter.export_to_markdown(file_path = args.md, codeLoc = codeLoc)
                logging.info(f"Finished in {end_time - start_time:.2f}s")
                self.console.clear()

                verbose_confirm = self.confirmInteractive.verbose_confirm()
                self._show_override_msg(
                    verbose_confirm = verbose_confirm, codeLoc = codeLoc, file_name = args.md
                )
                logging.info(f"Finished in {end_time - start_time:.2f}s")
                sys.exit(0)

        exporter.export_to_markdown(file_path = args.md, codeLoc = codeLoc)
        self.console.clear()

        verbose_confirm_2 = self.confirmInteractive.verbose_confirm()
        self._show_success_msg(verbose_confirm = verbose_confirm_2, codeLoc = codeLoc, file_name = args.md)
        logging.info(f"Finished in {end_time - start_time:.2f}s")

    def on_json_export_option(self, args: str) -> None:
        start_time = time.perf_counter()
        with self.console.status(status = "Processing...", spinner = "material"):
            codeLoc = CodeLoc(args.path, args.ignore)

        end_time = time.perf_counter()
        self.console.clear()

        if not args.json.endswith(".json"):
                args.json += ".json"

        exporter = Exporter(args)
        if os.path.exists(args.json):
            confirm_override: bool = self.confirmInteractive.override_write_confirm(file_name = args.json)

            if not confirm_override:
                sys.exit(0)

            if confirm_override:
                exporter.export_to_json(file_path = args.json, codeLoc = codeLoc)
                logging.info(f"Finished in {end_time - start_time:.2f}s")
                self.console.clear()

                verbose_confirm = self.confirmInteractive.verbose_confirm()
                self._show_override_msg(
                    verbose_confirm = verbose_confirm, codeLoc = codeLoc, file_name = args.json
                )
                logging.info(f"Finished in {end_time - start_time:.2f}s")
                sys.exit(0)

        exporter.export_to_json(file_path = args.json, codeLoc = codeLoc)
        self.console.clear()

        verbose_confirm_2 = self.confirmInteractive.verbose_confirm()
        self._show_success_msg(verbose_confirm = verbose_confirm_2, codeLoc = codeLoc, file_name = args.json)
        logging.info(f"Finished in {end_time - start_time:.2f}s")

    def _show_override_msg(self, verbose_confirm: bool, codeLoc: CodeLoc, file_name) -> None:
        if verbose_confirm:
            codeLoc.stdout_result()
            self.console.print("\n".join([
                f"Override file {file_name} successfully.",
                f"File path directory: {os.path.abspath(file_name)}"
            ]))

        else:
            self.console.print("\n".join([
                f"Override file {file_name} successfully.",
                f"File path directory: {os.path.abspath(file_name)}"
            ]))

    def _show_success_msg(self, verbose_confirm: bool, codeLoc: CodeLoc, file_name: str) -> None:
        if verbose_confirm:
            codeLoc.stdout_result()
            self.console.print("\n".join([
                f"Sucessfully save {file_name}.",
                f"Save directory path: {os.path.abspath(file_name)}"
            ]))

        else:
            self.console.print("\n".join([
                f"Sucessfully save {file_name}.",
                f"Save directory path: {os.path.abspath(file_name)}"
            ]))
