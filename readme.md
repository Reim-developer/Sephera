# Sephera

**Sephera is a lightweight command-line tool for analyzing and visualizing your project's structure and codebase.**

![CodeLoc Preview](./preview/CodeLoc.gif)

![Benchmark test with cloc](./benchmark/benchmark.png)

## Features
- ⚙️ **Portable**: Zero setup, just download and run.
- ⚙️ **Customize:** Enjoy infinite customization through YAML configuration.
- ⚙️ **Update online:** Always can update Sephera in your console.
- 🔍 `loc`: Count total lines of code with regex/glob support.
- 📊 `stats`: Show detailed file/folder stats (count, size, etc.).
- 🌳 `tree`: Directory tree visualization with optional chart.
- ❌ Ignore patterns: Regex-based exclusion (`__pycache__`, `.git`, etc.).
- 📈 Optional chart generation in CLI or image format.
- 🧠 **Verbose Mode**: Interactive prompt to toggle detailed info.
- 📁 **JSON Export**: Export scan results in structured JSON format.
- 🔥 **Massive Scan Support**: Handles entire `~/` directory in ~20s.
- 🎯 **Hardcore File/Folder Ignore**: Directly exclude entire paths.
- 🏷️ **Language Detection**: Auto detects 70+ languages.
- 🧪 **Benchmark**: 1.38M lines scanned in ~1.13s.

## For more information, please visit Sephera documentation:
* [Website Documentation](https://reim-developer.github.io/Sephera/)
* [Markdown Documentation](./docs/index.md)

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
Use `sephera help` for more information

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
