import toml
import logging

def generate_help() -> None:
    toml_source = toml.load("./config/msg.toml")
    usage_help: dict[str, str] = toml_source.get("usage", {})
    logging.basicConfig(level = logging.DEBUG, format = "%(asctime)s - %(levelname)s - %(message)s")

    first_line: list[str] = [
        "# ================================================================\n\n"
        "# Auto-generated file config from YAML configuration",
        "# You can customize this config via config/languages.yml file",
        "# If this file is not exists, you can find this in:",
        "# https://github.com/Reim-developer/Sephera/tree/master/config",
        "# This project is licensed under the GNU General Public License v3.0",
        "# https://github.com/Reim-developer/Sephera?tab=GPL-3.0-1-ov-file\n",
        "# ==============================================================\n"
    ]

    key: str = ""
    value: str = ""
    for key, value in usage_help.items():
        first_line.append(f'{key.upper()}_HELP = """{value.strip()}"""\n')

    with open("./etc/generate/config_help.py", "w") as config_help:
        config_help.write("\n".join(first_line))

    logging.debug("Generate configuration help successufully.")

generate_help()
