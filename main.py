import argparse
from walk.WalkFile import WalkFile

class Sephera:
    def __init__(self):
        self.sephera_parser = argparse.ArgumentParser(description = "Sephera Commmand Line Interface")
        sub_command = self.sephera_parser.add_subparsers(dest = "command", required = True)

        tree_command = sub_command.add_parser("tree", help = "List tree view all files")
        tree_command.add_argument(
            "--ignore",
            type = str,
            help = "Regex pattern to ignore files or folders (e.g. '__pycache__|\\.git')",
            default = None
        )
        tree_command.set_defaults(function = self.walk_files)

    def walk_files(self, args) -> None:
        walker = WalkFile(args.ignore)
        walker.show_list_tree()

if __name__ == "__main__":
    cli = Sephera()
    args = cli.sephera_parser.parse_args()
    args.function(args)