# Sephera

Sephera is a lightweight command-line tool for analyzing and visualizing your project's structure and codebase.

## Features

- 🔍 `loc`: Quickly calculate the total lines of code.
- 📊 `stats`: Show project statistics, including file/folder counts and total size.
- 🌳 `tree`: Display a directory tree view with optional chart generation.

## Installation

```bash
pip install sephera
```

## Usage

```bash
sephera [command] [options...]
```
## How to use
Use `sephera help` for more information.

## Example

```bash
sephera loc --path ./my-project
sephera stats --ignore "__pycache__|\.git"
sephera tree --chart
```

## Preview
* You can visit [here](./preview) to view how Sephera works.

### LICENSE
Sephera is licensed under the GNU General Public License v3.0.
