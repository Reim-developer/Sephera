import sys 
import os

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

    
    def on_json_export_option(self, args: str) -> None:
        codeLoc = CodeLoc(args.path, args.ignore)

        if not args.json.endswith(".json"):
                args.json += ".json"

        if os.path.exists(args.json):
            confirm_override: bool = self.confirmInteractive.override_write_confirm(file_name = args.json)

            if confirm_override:
                codeLoc.export_to_json(file_path = args.json)
                self.console.clear()

                verbose_confirm = self.confirmInteractive.verbose_confirm()
                self._show_override_msg(
                    verbose_confirm = verbose_confirm, codeLoc = codeLoc, args = args
                )

        codeLoc.export_to_json(file_path = args.json)
        self.console.clear()

        verbose_confirm_2 = self.confirmInteractive.verbose_confirm()
        self._show_success_msg(verbose_confirm = verbose_confirm_2, codeLoc = codeLoc, args = args)

    def _show_override_msg(self, verbose_confirm: bool, codeLoc: CodeLoc, args: str) -> None:
        if verbose_confirm:
            codeLoc.stdout_result()
            self.console.print("\n".join([
                f"Override file {args.json} successfully.",
                f"File path directory: {os.path.abspath(args.json)}"
            ]))
            sys.exit(0)

        else:
            self.console.print("\n".join([
                f"Override file {args.json} successfully.",
                f"File path directory: {os.path.abspath(args.json)}"
            ]))
            sys.exit(0)

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
            sys.exit(0)