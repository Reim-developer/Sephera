import argparse
import logging
import sys
from sephera.CodeLoc import CodeLoc
from datalyzer.sql import SqlManager
from utils.utils import Utils

class SepheraTest:
    def __init__(self) -> None:
        self.log = logging
        self.log.basicConfig(level = logging.DEBUG, format = "%(asctime)s - %(levelname)s - %(message)s")

        self.args: argparse.ArgumentParser = argparse.ArgumentParser()
        self.subparsers = self.args.add_subparsers(dest = "command")

        self.test_command = self.subparsers.add_parser(
            "test", help = "Run test cases",
        )
        self.test_command.add_argument(
            "command",
            nargs = "*",
            help = "Test cases"
        )
        self.parser_args = self.args.parse_args()

        if self.parser_args.command:
            self.match_args(args = str(self.parser_args.command[0]))
        else:
            self.log.error("No test case provided.")
            sys.exit(1)

    def match_args(self, args: str) -> None:
        match args.lower().strip():
            case "loc":
                codeLoc = CodeLoc(".")
                codeLoc.stdout_result()
            
            case "fetch-version":
                utils = Utils()
                version = utils.fetch_latest_version()
                self.log.debug(version)
            
            case "is-latest":
                utils = Utils()
                isLatestVersion = utils.is_latest_version()
                
                if isLatestVersion:
                    self.log.debug("This binary of Sephera is latest version")

                else:
                    self.log.debug("This binary of Sephera it not a latest version")

            case "cfg-path":
                sql = SqlManager()
                utils = Utils()
                sql.connect_to_sql(f"{utils.get_local_data()}/.config/Sephera/settings.db")

                self.log.debug(sql.get_cfg_path())
            case _:
                self.log.error(f"Test case: {args} not found.")
                sys.exit(1)

_ = SepheraTest()