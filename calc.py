import yaml
import logging

def calc_languages_supports() -> None:
    logging.basicConfig(level = logging.DEBUG, format = "%(asctime)s - %(levelname)s - %(message)s")

    with open("config/languages.yml", "r") as config_file:
        yaml_source = yaml.safe_load(config_file)
        
    languages_support: int = sum(
        1 for language in yaml_source.get("languages", []) 
        if "name" in language)
    
    logging.info(f"Programming Languages Supported: {languages_support}")

calc_languages_supports()