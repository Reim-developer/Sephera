.PHONY: chart config data preview sephera test utils .venv

# Make sure .venv exists. 
venv = .venv/bin/python
pip_venv = .venv/bin/pip
config_python = generate_config.py

# Make sure requirements.txt exists
requirements_pip = requirements.txt
install_command = @pip install -r

test:
	@$(venv) test.py

config:
	@$(venv) $(config_python)

# Install dependencies from requirements.txt
deps:
	@$(pip_venv) $(install_command) $(requirements_pip)