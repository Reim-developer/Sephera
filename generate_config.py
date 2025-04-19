import yaml
import json
import re

def generate_config() -> None:
    with open(file = "./config/languages.yml", mode = "r", encoding = "utf-8") as config_file:
        config_data = yaml.safe_load(config_file)

    config_json = json.dumps(config_data, indent = 2, ensure_ascii = False)
    config_json = re.sub(r'\bnull\b', 'None', config_json)

    with open(file = "config_data.py", mode = "w", encoding = "utf-8") as output_file:
        output_file.write("# Auto-generated file config from YAML configuration.\n")
        output_file.write(f"CONFIG_DATA = {config_json}\n")

    print(f"Generated config_data file successfully.")

generate_config()