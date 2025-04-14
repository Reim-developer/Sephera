import base64

with open("config/languages.yml", "r") as config_file:
    yaml_source  = config_file.read()

base64_encode = base64.b64encode(yaml_source.encode()).decode()
print("Programming Language Configuration Base64:")
print(base64_encode)
