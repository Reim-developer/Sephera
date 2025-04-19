.PHONY: chart config data preview sephera test utils .venv

# Make sure .venv exists. 
# If you use Windows, change activate to
# activate.ps1
venv = .venv/bin/activate
config_python = generate_config.py

# Make sure requirements.txt exists
requirements_pip = requirements.txt
install_command = pip install -r

source:
	source $(venv)

test:
	@python test.py

config:
	@python $(config_python)

# Install dependencies from requirements.txt
deps:
	$(install_command) $(requirements_pip)