# Basic Usage:
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