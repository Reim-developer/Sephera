.PHONY: chart config data preview sephera test utils .venv

# Make sure .venv exists. 
venv = .venv/bin/python
pip_venv = .venv/bin/pip
data_config = generate_data_config.py
help_config = generate_help.py

# Make sure requirements.txt exists
requirements_pip = requirements.txt

test:
	@$(venv) test.py

gen-data-cfg:
	@$(venv) $(data_config)

gen-help-cfg:
	@$(venv) $(help_config)

# Install dependencies from requirements.txt
deps:
	@$(pip_venv) install -r $(requirements_pip)

# Check venv is exists.
venv_check:
	@if [ ! -d ".venv" ]; then \
		python3 -m venv .venv; \
	fi
	@echo "Virtual enviroment is ready. Use source .venv/bin/activate to activate this."
