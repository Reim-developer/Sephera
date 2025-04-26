# Sephera Commands:
---
## Loc Command
---
- **Description:** *Count lines of code in current directory*.
```bash
sephera loc
```
---
**Arguments:**
```bash
--path
```
- **Description:** Target directory you want Sephera to analyze. If the `--path` flag is not provided, it will count lines of code in current directory. This flag is optional.
```bash
# Example usage:
sephera loc --path ~/myProject # Linux/macOS
sephera loc --path C:\Document\myProject # Windows
```
---
```bash
--ignore
```
- **Description:** Directory or file patterns to exclude from counting. You can use the `--ignore` flag multiple times. Supports both Glob and Regex. This flag is optional.

---
```bash
sephera loc --ignore "*.py*" # Ignore ALL Python files.
sephera loc --ignore "^.*\.py$" # Ignore ALL Python files with Regex.
sephera loc --ignore "node_module" # Ignore ALL files, and folders in `node_modules`
sephera loc --ignore "*.py" --ignore "*.js" # Use mutiple --ignore flags.
```
---
```bash
--json
```
- **Description:** Export result to a .json file. This flag is optional. If no filename is provided, it will default to SepheraExport.json.
```bash
sephera loc --json # Will export: SepheraExport.json
sephera loc --json hello_sephera # Will export: hello_sephera.json
```
---
```bash
--md
```
- **Description:** **Description:** Export result to a .md file. This flag is optional. If no filename is provided, it will default to SepheraExport.md.

```bash
sephera loc --md # Will export: SepheraExport.md
sephera loc --md hello_sephera # Will export: hello_sephera.md
```

## Update Command
---
- **Description:** Update Sephera to the latest version or install it to a different directory path.
```bash
sephera update
```
---
- **Note:** When you use this command, if you're on the latest version of Sephera, it will redirect you to interactive mode, like this:
```bash
[!] You're using latest version of Sephera, do you want:
[1] Re-install Sephera.
[2] Install to another directory path.
[3] Cancel and exit now.
Your option [1-3]: 
```
- Otherwise, it will automatically update for you.

## Stats Command
---
* **Description:** Stats your project metadata, files and folders count, project size

**Arguments:**
```bash
--path
```
- **Description:** The target directory that you want Sephera to analyze. If the `--path` flag is not provided, it will default to the current directory. This flag is optional.
```bash
# Example
sephera stats --path ~/myProject # For Linux/macOS
sephera stats --path C:\users\Document\myProject # Windows
```
---
```bash
--ignore
```
- **Description:** Ignores directories or folders. If the --ignore flag is not provided, it will default to the current directory. It supports both regular expressions and exact file/folder names. This flag is optional.
```bash
# Example
sephera stats --ignore node_modules
```
---
```bash
--chart
```
- **Description:** Create chart for your stat overview. Default chart name is 'SepheraChart'.
```bash
# Example:
sephera stats --chart # Will export to default name, as SepheraChart
sephera stats --chart myCustomizeChart # Will export to myCustomizeChart
```

## Tree Command
* **Description:** Show your project structure.

**Arguments:**
```bash
--path
```
- **Description:** The target directory that you want Sephera to analyze. If the `--path` flag is not provided, it will default to the current directory. This flag is optional.
```bash
# Example
sephera tree --path ~/myProject # For Linux/macOS
sephera tree --path C:\users\Document\myProject # Windows
```
---
```bash
--ignore
```
- **Description:** Ignores directories or folders. If the --ignore flag is not provided, it will default to the current directory. It supports both regular expressions and exact file/folder names. This flag is optional.
```bash
# Example
sephera tree --ignore node_modules
```
---
```bash
--chart
```
- **Description:** Create chart for your stat overview. Default chart name is 'SepheraChart'.
```bash
# Example:
sephera tree --chart # Will export to default name, as SepheraChart
sephera tree --chart myCustomizeChart # Will export to myCustomizeChart
```

## Language Support Command
---
* **Description:** Show the count of languages currently supported by Sephera.
```bash
sephera language-support
```
---
```bash
--list
```
* **Description:** Show the list of languages currently supported by Sephera.
```bash
# Example and output:
sephera language-support --list
...
[66] Eiffel
[67] Pascal
[68] TCL
[69] Elixir
[70] Markdown
[71] M4
[72] Kotlin Build Script
[73] V Lang
[+] Total language(s) supported by Sephera: 73
[+] Sephera current version: 1.0.0
```

## Version Command
---
* **Description:** Show current or latest Sephera version from GitHub.
```bash
sephera version
```
---
```bash
--git
```
* **Description:** Show the latest version of Sephera from GitHub.

```bash
# Example
sephera version --git
[+] The Sephera latest version on GitHub: 1.0.0
[+] Your Sephera version: 1.0.0
[+] You are use the latest version of Sephera
```

## Cfg-language
---
* **Description:** Set configuration language to your current directory.
```bash
sephera cfg-language
```
---
```bash
--global
```
* **Description:** Set Sephera configuration in your user home directory, e.g. ~/ or AppData/Local

```bash
# Example for create global configuration.
sephera cfg-language --global
[+] Language detection configuration saved successfully.
[+] Configuration path: /home/reim/.config/Sephera/SepheraCfg.yml
```
