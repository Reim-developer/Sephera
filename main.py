import argparse
import sys

try:
    from command import Command
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class SepheraCli:
    def __init__(self):
        self.sephera_parser = argparse.ArgumentParser(description = "Sephera Commmand Line Interface")
        command = Command(sephera_parser = self.sephera_parser)
        command.setup()
       
if __name__ == "__main__":
    try:
        cli = SepheraCli()
        args = cli.sephera_parser.parse_args()
        args.function(args)
    except KeyboardInterrupt:
        print("\n Aborted by user.")
