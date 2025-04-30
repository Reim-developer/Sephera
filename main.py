import argparse
import sys

try:
    from command import Command
    from etc.subclass import SepheraArgparse
except KeyboardInterrupt:
    print("\nAborted by user.")
    sys.exit(1)

class SepheraCli:
    def __init__(self):
        self.sephera_parser = SepheraArgparse(
            add_help = False,
            usage = argparse.SUPPRESS,
            formatter_class = argparse.RawDescriptionHelpFormatter
        )
        command = Command(sephera_parser = self.sephera_parser)
        command.setup()

def main() -> None:
    try:
        cli = SepheraCli()
        args = cli.sephera_parser.parse_args()
        
        if hasattr(args, 'function'):
            args.function(args)
    
    except KeyboardInterrupt:
        print("\nAborted by user.")
        sys.exit(1)
    
if __name__ == "__main__":
   main()