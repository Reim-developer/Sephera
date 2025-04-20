.PHONY: chart config data preview sephera test utils .venv

# Make sure .venv exists. 
venv = .venv/bin/python
pip_venv = .venv/bin/pip
config_python = generate_config.py

# Make sure requirements.txt exists
requirements_pip = requirements.txt

test:
	@$(venv) test.py

config:
	@$(venv) $(config_python)

# Install dependencies from requirements.txt
deps:
	@$(pip_venv) install -r $(requirements_pip)

# Check venv is exists.
venv_check:
	@if [ ! -d ".venv" ]; then \
		python3 -m venv .venv; \
	fi
	@echo "Virtual enviroment is ready. Use source .venv/bin/activate to activate this."
