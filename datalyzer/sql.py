import sqlite3

try:
    from rich.console import Console
    from utils.stdout import SepheraStdout
except KeyboardInterrupt:
    print("\nAborted by user.")

class SqlManager:
    def __init__(self) -> None:
        self.console = Console()
        self.connection = None
        self.cursor = None

    def connect_to_sql(self, db_path: str) -> None:
        try:
            self.connection = sqlite3.connect(database = db_path)
            self.cursor = self.connection.cursor()

        except Exception as error:
            stdout = SepheraStdout()
            stdout.die(error = error)

    
    def create_sql_table(self) -> None:
        sql_query = """\
        CREATE TABLE IF NOT EXISTS config_path (
            global_cfg_path TEXT NOT NULL,
            user_cfg_path TEXT,
            UNIQUE(global_cfg_path)
        )
        """

        try:
            self.cursor.execute(sql_query)
            self.connection.commit()
        except  Exception as error:
            stdout = SepheraStdout()
            stdout.die(error = error)