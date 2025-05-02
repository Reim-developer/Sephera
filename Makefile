.PHONY: chart config data preview sephera test utils .venv build dist

# Make sure .venv exists. 
venv = .venv/bin/python
pip_venv = .venv/bin/pip
data_config = generate_data_config.py
help_config = generate_help.py
test_entry = test.py
gui_main_entry = gui.main

# Make sure requirements.txt exists
requirements_pip = requirements.txt

# Test case
test-loc:
	@$(venv) $(test_entry) test loc

test-version:
	@$(venv) $(test_entry) test fetch-version

test-is-latest:
	@$(venv) $(test_entry) test is-latest

test-languages-supports:
	@$(venv) calc.py

test-cfg:
	@$(venv) $(test_entry) test cfg-path

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

UNAME_S := $(shell uname -s)

build-py:
	@chmod +x ./scripts/pybuild.sh
	./scripts/pybuild.sh

check:
	@ruff check .

