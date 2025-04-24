import sys 
import os
import logging
import time

try:
    from rich.console import Console
    from sephera.CodeLoc import CodeLoc
    from sephera.interactive.confirm import ConfirmInteractive
except KeyboardInterrupt:
    print("\n Aborted by user.")

class OptionHandler:
    def __init__(self) -> None:
        self.console = Console()
        self.confirmInteractive = ConfirmInteractive()
        logging.basicConfig(level = logging.INFO, format = "[%(levelname)s] %(message)s")

    def on_json_export_option(self, args: str) -> None:
        start_time = time.perf_counter()
        with self.console.status(status = "Processing...", spinner = "material"):
            codeLoc = CodeLoc(args.path, args.ignore)

        end_time = time.perf_counter()
        self.console.clear()

        if not args.json.endswith(".json"):
                args.json += ".json"

        if os.path.exists(args.json):
            confirm_override: bool = self.confirmInteractive.override_write_confirm(file_name = args.json)

            if not confirm_override:
                sys.exit(0)

            if confirm_override:
                codeLoc.export_to_json(file_path = args.json)
                logging.info(f"Finished in {end_time - start_time:.2f}s")
                self.console.clear()

                verbose_confirm = self.confirmInteractive.verbose_confirm()
                self._show_override_msg(
                    verbose_confirm = verbose_confirm, codeLoc = codeLoc, args = args
                )
                logging.info(f"Finished in {end_time - start_time:.2f}s")
                sys.exit(0)

        codeLoc.export_to_json(file_path = args.json)
        self.console.clear()

        verbose_confirm_2 = self.confirmInteractive.verbose_confirm()
        self._show_success_msg(verbose_confirm = verbose_confirm_2, codeLoc = codeLoc, args = args)
        logging.info(f"Finished in {end_time - start_time:.2f}s")

    def _show_override_msg(self, verbose_confirm: bool, codeLoc: CodeLoc, args: str) -> None:
        if verbose_confirm:
            codeLoc.stdout_result()
            self.console.print("\n".join([
                f"Override file {args.json} successfully.",
                f"File path directory: {os.path.abspath(args.json)}"
            ]))

        else:
            self.console.print("\n".join([
                f"Override file {args.json} successfully.",
                f"File path directory: {os.path.abspath(args.json)}"
            ]))

    def _show_success_msg(self, verbose_confirm: bool, codeLoc: CodeLoc, args: str) -> None:
        if verbose_confirm:
            codeLoc.stdout_result()
            self.console.print("\n".join([
                f"Sucessfully save {args.json}.",
                f"Save directory path: {os.path.abspath(args.json)}"
            ]))

        else:
            self.console.print("\n".join([
                f"Sucessfully save {args.json}.",
                f"Save directory path: {os.path.abspath(args.json)}"
            ]))
