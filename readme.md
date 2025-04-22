# Sephera

**Sephera is a lightweight command-line tool for analyzing and visualizing your project's structure and codebase.**

![CodeLoc Preview](./preview/CodeLoc.gif)

## Features
- 🚀 **Blazingly fast**: 700k lines counted in just 1 second.
- ⚙️ **Portable**: Zero setup, just download and run.
- 🔍 `loc`: Count total lines of code with regex/glob support.
- 📊 `stats`: Show detailed file/folder stats (count, size, etc.).
- 🌳 `tree`: Directory tree visualization with optional chart.
- ❌ Ignore patterns: Regex-based exclusion (`__pycache__`, `.git`, etc.).
- 📈 Optional chart generation in CLI or image format.

## Installation
1. Visit the [release page](https://github.com/Reim-developer/Sephera/releases/).
2. Download the binary for your OS.
3. Add it to PATH (optional).
4. Run it from anywhere.

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
