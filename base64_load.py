import base64

class Base64Load:
    @staticmethod
    def load_base64_config(file_path: str) -> str:
        with open(file = file_path, mode = "r") as config_file:
            yaml_source  = config_file.read()
            base64_encode = base64.b64encode(yaml_source.encode()).decode()

            return base64_encode

print("Programming Language Configuration Base64:")
base64_value: str = Base64Load.load_base64_config("config/languages.yml")
print()
print(base64_value)