import yaml
import json
import re
import logging

def generate_data_config() -> None:
    with open(file = "./config/languages.yml", mode = "r", encoding = "utf-8") as config_file:
        config_data = yaml.safe_load(config_file)

    config_json = json.dumps(config_data, indent = 2, ensure_ascii = False)
    config_json = re.sub(r'\bnull\b', 'None', config_json)

    logging.basicConfig(level = logging.DEBUG, format = "%(asctime)s - %(levelname)s - %(message)s")

    with open(file = "./etc/generate/config_data.py", mode = "w", encoding = "utf-8") as output_file:
        output_file.write("\n".join([
            "# ================================================================",
            "# Auto-generated file config from YAML configuration",
            "# You can customize this config via config/languages.yml file",
            "# If this file is not exists, you can find this in:",
            "# https://github.com/Reim-developer/Sephera/tree/master/config",
            "# This project is licensed under the GNU General Public License v3.0",
            "# https://github.com/Reim-developer/Sephera?tab=GPL-3.0-1-ov-file",
            "# ==============================================================\n"
        ]))
        output_file.write(f"CONFIG_DATA = {config_json}\n")

    logging.debug("Generate data configuration sucessfully.")

generate_data_config()