import sys
import os
import time
import logging
import argparse

try:
    from rich.console import Console
    from utils.stdout import SepheraStdout
    from sephera.Stats import Stats
    from sephera.WalkFile import WalkFile
    from datalyzer.Exporter import Exporter
    from utils.utils import Utils
    from sephera.CodeLoc import CodeLoc
    from sephera.help import SepheraHelp
    from sephera.get_update import GetUpdate
    from sephera.interactive.option import OptionHandler
    from sephera.fetch_lang_support import FetchLanguage
    from sephera.version import VersionCommand
    from sephera.set_cfg import SetConfiguration
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class Handler:
    def __init__(self) -> None:
        self.console = Console()
        self.sephera_stdout = SepheraStdout()
        self.utils = Utils()
        
    def show_usage(self, args: argparse.Namespace) -> None:
        if args.command is None:
            sepheraHelp = SepheraHelp()
            sepheraHelp.usage()

    def stats_command_handler(self, args: argparse.Namespace) -> None:
        if not os.path.exists(args.path):
            self.sephera_stdout.show_error(f"Fatal error: {args.path} not found.")
            sys.exit(1)

        stats = Stats(base_path = args.path, ignore_pattern = args.ignore)
        stats.stats_all_files(output_chart = args.chart)

    def tree_command_handler(self, args: argparse.Namespace) -> None:
        if not os.path.exists(args.path):
            error = SepheraStdout()
            error.show_error(f"Path: {args.path} not found.")
            sys.exit(1)

        walker = WalkFile(args.ignore, args.path)
       
        stats = walker.show_list_tree()

        if args.chart:
            exporter = Exporter(args.chart)
            exporter.export_file_tree_chart(
                    files = stats["Files"],
                    dirs = stats["Directory"],
                    hidden_files = stats["Hidden_Files"],
                    hidden_dirs = stats["Hidden_Directory"],
                        
            )
            self.console.print(f"[cyan][+] Successfully created chart with name: {args.chart}.png")

    def loc_command_handler(self, args: argparse.Namespace) -> None:
        if not self.utils.is_path_exists(args.path):
            self.sephera_stdout.show_error(f"{args.path} not found.")
            sys.exit(1)
        
        logging.basicConfig(level = logging.INFO, format = "[%(levelname)s] %(message)s")

        if args.json and args.md:
            self.console.print("[yellow][+] You can export one file in this time.")
            sys.exit(0)

        if args.json:
            option = OptionHandler()
            option.on_json_export_option(args = args) # type: ignore
            sys.exit(0)
        
        if args.md:
            option = OptionHandler()
            option.on_markdown_export_option(args = args) # type: ignore
            sys.exit(0)

        start_time: float = time.perf_counter()
        with self.console.status(status = "Processing...", spinner = "material"):
            codeLoc = CodeLoc(args.path, args.ignore)

        end_time: float = time.perf_counter()
        self.console.clear()

        codeLoc.stdout_result()
        logging.info(f"Finished in {end_time - start_time:.2f}s")
    
    def help_command_handler(self, args: argparse.Namespace) -> None:
        sepheraHelp = SepheraHelp()

        if not args.command:
            sepheraHelp.usage()
        else:
            sepheraHelp.show_help(args = str(args.command[0]))

    def update_command_handler(self, args: argparse.Namespace) -> None:
        get_update = GetUpdate()
        get_update.update_sephera()

    def fetch_languages_supports_handler(self, args: argparse.Namespace) -> None:
        fetch_languages = FetchLanguage()
        if args.list:
            fetch_languages.fetch_language_count(verbose = True)

        fetch_languages.fetch_language_count()

    def version_command_handler(self, args: argparse.Namespace) -> None:
        version_command = VersionCommand()

        version_command.fetch_version(show_git_version = args.git)

    def language_cfg_handler(self, args: argparse.Namespace) -> None:
        set_cfg = SetConfiguration()
        set_cfg.set_language_cfg(stdout = self.sephera_stdout, global_cfg = args.global_)