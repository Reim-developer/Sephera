import os
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
        sql_query = """--sql
            CREATE TABLE IF NOT EXISTS config_path (
                global_cfg_path TEXT PRIMARY KEY,
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

    def set_global_cfg_path(self, global_cfg_path: str) -> None:
        try:
            sql_query = """--sql
                INSERT OR REPLACE INTO config_path (global_cfg_path)
                VALUES (?)
            """

            self.cursor.execute(sql_query, (global_cfg_path,))
            self.connection.commit()
        
        except Exception as error:
            stdout = SepheraStdout()
            stdout.die(error = error)
        
        finally:
            self.connection.close()

    def set_user_cfg_path(self, user_cfg_path: str) -> None:
        try:
            sql_check_query = """--sql
                SELECT COUNT(*) FROM config_path WHERE user_cfg_path = ?
            """

            self.cursor.execute(sql_check_query, (user_cfg_path, ))
            result = self.cursor.fetchone()

            if result[0] == 0:
                sql_update_query = """--sql
                    INSERT INTO config_path (user_cfg_path)
                    VALUES (?)
                """

                self.cursor.execute(sql_update_query, (user_cfg_path,))

            self.connection.commit()
        
        except Exception as error:
            stdout = SepheraStdout()
            stdout.die(error = error)

        finally:
            self.connection.close()
        
    def cfg_type(self) -> int:
        try:
            sql_query = """--sql
                SELECT global_cfg_path, user_cfg_path FROM config_path
            """
            self.cursor.execute(sql_query)
            result = self.cursor.fetchone()
        
            global_cfg_path, user_cfg_path = (result[0], result[1]) if result else (None, None)
            current_dir = os.getcwd()

            if user_cfg_path == current_dir:
                return 1
                
            if global_cfg_path and os.path.exists(global_cfg_path):
                return 2

        except Exception:
            return 3

    def get_cfg_path(self) -> str | None:
        cfg_type = self.cfg_type()

        try:
            match cfg_type:
                case 1:
                    sql_query = """--sql
                        SELECT user_cfg_path FROM config_path
                        WHERE user_cfg_path IS NOT NULL
                        LIMIT 1
                    """
                
                case 2:
                    sql_query = """--sql
                        SELECT global_cfg_path FROM config_path
                        WHERE global_cfg_path IS NOT NULL
                        LIMIT 1
                    """

                case 3:
                    return None
                
            self.cursor.execute(sql_query)
            result = self.cursor.fetchone()
            
            return result[0] if result else None
            
        except Exception as error:
            stdout = SepheraStdout()
            stdout.die(error = error)

        finally:
            self.connection.close()
