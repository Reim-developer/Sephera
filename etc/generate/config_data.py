# ================================================================
# Auto-generated file config from YAML configuration
# You can customize this config via config/languages.yml file
# If this file is not exists, you can find this in:
# https://github.com/Reim-developer/Sephera/tree/master/config
# This project is licensed under the GNU General Public License v3.0
# https://github.com/Reim-developer/Sephera?tab=GPL-3.0-1-ov-file
# ==============================================================

CONFIG_DATA: dict[str, object] = {
  "comment_styles": {
    "c_style": {
      "single_line": "//",
      "multi_line_start": "/*",
      "multi_line_end": "*/"
    },
    "python_style": {
      "single_line": "#",
      "multi_line_start": "\"\"\"",
      "multi_line_end": "\"\"\""
    },
    "shell_style": {
      "single_line": "#",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "perl_style": {
      "single_line": "#",
      "multi_line_start": "=",
      "multi_line_end": "=cut"
    },
    "ruby_style": {
      "single_line": "#",
      "multi_line_start": "=begin",
      "multi_line_end": "=end"
    },
    "no_comment": {
      "single_line": None,
      "multi_line_start": None,
      "multi_line_end": None
    },
    "html_style": {
      "single_line": None,
      "multi_line_start": "<!--",
      "multi_line_end": "-->"
    },
    "sql_style": {
      "single_line": "--",
      "multi_line_start": "/*",
      "multi_line_end": "*/"
    },
    "vim_style": {
      "single_line": "\"",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "godot_style": {
      "single_line": "#",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "lua_style": {
      "single_line": "--",
      "multi_line_start": "--[[",
      "multi_line_end": "]]"
    },
    "lisp_style": {
      "single_line": ";",
      "multi_line_start": "#|",
      "multi_line_end": "|#"
    },
    "asm_style": {
      "single_line": ";",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "meson_style": {
      "single_line": "#",
      "multi_line_start": "/*",
      "multi_line_end": "*/"
    },
    "smalltalk_style": {
      "single_line": "\"",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "haskell_style": {
      "single_line": "--",
      "multi_line_start": "{-",
      "multi_line_end": "-}"
    },
    "nim_style": {
      "single_line": "#",
      "multi_line_start": "#[",
      "multi_line_end": "]#"
    },
    "julia_style": {
      "single_line": "#",
      "multi_line_start": "#=",
      "multi_line_end": "=#"
    },
    "coffee_script_style": {
      "single_line": "###",
      "multi_line_start": "/*",
      "multi_line_end": "*/"
    },
    "basic_style": {
      "single_line": "'",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "matlab_style": {
      "single_line": "%",
      "multi_line_start": "%{",
      "multi_line_end": "}%"
    },
    "ada_style": {
      "single_line": "--",
      "multi_line_start": "/*",
      "multi_line_end": "*/"
    },
    "autohotkey_style": {
      "single_line": ";",
      "multi_line_start": "/*",
      "multi_line_end": "*/"
    },
    "erlang_style": {
      "single_line": "%%",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "clojure_style": {
      "single_line": ";",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "cobol_style": {
      "single_line": "*>",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "fortran_style": {
      "single_line": "!",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "ocaml_style": {
      "single_line": None,
      "multi_line_start": "(*",
      "multi_line_end": "*)"
    },
    "eiffel_style": {
      "single_line": "--",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "pascal_style": {
      "single_line": "//",
      "multi_line_start": "{",
      "multi_line_end": "}"
    },
    "elixir_style": {
      "single_line": "#",
      "multi_line_start": "\"\"\"",
      "multi_line_end": "\"\"\""
    },
    "batchfile_style": {
      "single_line": "REM",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "fsharp_style": {
      "single_line": "//",
      "multi_line_start": "(*",
      "multi_line_end": "*)"
    },
    "vhdl_style": {
      "single_line": "--",
      "multi_line_start": None,
      "multi_line_end": None
    },
    "tex_style": {
      "single_line": "%",
      "multi_line_start": None,
      "multi_line_end": None
    }
  },
  "languages": [
    {
      "name": "Python",
      "extension": [
        ".py"
      ],
      "comment_styles": "python_style"
    },
    {
      "name": "Java",
      "extension": [
        ".java"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "JavaScript",
      "extension": [
        ".js",
        ".mjs"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Shell Script",
      "extension": [
        ".sh"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "C++",
      "extension": [
        ".cc",
        ".cpp",
        ".cxx",
        ".c++"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "C",
      "extension": [
        ".c"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Golang",
      "extension": [
        ".go"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Perl",
      "extension": [
        ".pl"
      ],
      "comment_styles": "perl_style"
    },
    {
      "name": "Ruby",
      "extension": [
        ".rb"
      ],
      "comment_styles": "ruby_style"
    },
    {
      "name": "C Header File",
      "extension": [
        ".h"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "C++ Header File",
      "extension": [
        ".hpp",
        ".hh",
        ".h++"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "C#",
      "extension": [
        ".cs"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "TypeScript",
      "extension": [
        ".ts"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "React JavaScript",
      "extension": [
        ".jsx"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "React TypeScript",
      "extension": [
        ".tsx"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Rust",
      "extension": [
        ".rs"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "PHP",
      "extension": [
        ".php"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "YAML",
      "extension": [
        ".yml",
        ".yaml"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "JSON",
      "extension": [
        ".json"
      ],
      "comment_styles": "no_comment"
    },
    {
      "name": "Cython",
      "extension": [
        ".pyx",
        ".pxd",
        ".pxi"
      ],
      "comment_styles": "python_style"
    },
    {
      "name": "CSS",
      "extension": [
        ".css"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "HTML",
      "extension": [
        ".html",
        ".htm"
      ],
      "comment_styles": "html_style"
    },
    {
      "name": "XML",
      "extension": [
        ".xml"
      ],
      "comment_styles": "html_style"
    },
    {
      "name": "Dart",
      "extension": [
        ".dart"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Kotlin",
      "extension": [
        ".kt"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "SQL",
      "extension": [
        ".sql"
      ],
      "comment_styles": "sql_style"
    },
    {
      "name": "Vim Script",
      "extension": [
        ".vim",
        ".vimrc"
      ],
      "comment_styles": "vim_style"
    },
    {
      "name": "Godot Script",
      "extension": [
        ".gd"
      ],
      "comment_styles": "godot_style"
    },
    {
      "name": "Lua",
      "extension": [
        ".lua"
      ],
      "comment_styles": "lua_style"
    },
    {
      "name": "Lisp",
      "extension": [
        ".lisp",
        ".lsp"
      ],
      "comment_styles": "lisp_style"
    },
    {
      "name": "Scala",
      "extension": [
        ".scala",
        ".sc"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "CMake",
      "extension": [
        ".cmake"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Nix",
      "extension": [
        ".nix"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Assembly",
      "extension": [
        ".asm"
      ],
      "comment_styles": "asm_style"
    },
    {
      "name": "Objective-C",
      "extension": [
        ".m"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Objective-C++",
      "extension": [
        ".mm"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "PowerShell",
      "extension": [
        ".ps1"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Meson",
      "extension": [
        ".meson"
      ],
      "comment_styles": "meson_style"
    },
    {
      "name": "Makefile",
      "extension": [
        "Makefile"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "R",
      "extension": [
        ".r"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Zig",
      "extension": [
        ".zig"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Scheme",
      "extension": [
        ".scm"
      ],
      "comment_styles": "lisp_style"
    },
    {
      "name": "Groovy",
      "extension": [
        ".groovy"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Smalltalk",
      "extension": [
        ".st"
      ],
      "comment_styles": "smalltalk_style"
    },
    {
      "name": "Haskell",
      "extension": [
        ".hs"
      ],
      "comment_styles": "haskell_style"
    },
    {
      "name": "Nim",
      "extension": [
        ".nim"
      ],
      "comment_styles": "nim_style"
    },
    {
      "name": "Julia",
      "extension": [
        ".jl"
      ],
      "comment_styles": "julia_style"
    },
    {
      "name": "Coffee Script",
      "extension": [
        ".coffee"
      ],
      "comment_styles": "coffee_script_style"
    },
    {
      "name": "BASIC",
      "extension": [
        ".bas"
      ],
      "comment_styles": "basic_style"
    },
    {
      "name": "MATLAB",
      "extension": [
        ".mlx"
      ],
      "comment_styles": "matlab_style"
    },
    {
      "name": "Action Script",
      "extension": [
        ".as"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Ada",
      "extension": [
        ".ada",
        ".adb"
      ],
      "comment_styles": "ada_style"
    },
    {
      "name": "AutoHotkey Script",
      "extension": [
        ".ahk"
      ],
      "comment_styles": "autohotkey_style"
    },
    {
      "name": "Carbon",
      "extension": [
        ".carbon"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Swift",
      "extension": [
        ".swift"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Emacs Lisp",
      "extension": [
        ".el",
        ".elc",
        ".eln"
      ],
      "comment_styles": "lisp_style"
    },
    {
      "name": "Fantom",
      "extension": [
        ".fan"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Erlang",
      "extension": [
        ".erl"
      ],
      "comment_styles": "erlang_style"
    },
    {
      "name": "Crytal",
      "extension": [
        ".cr"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Clojure",
      "extension": [
        ".clj"
      ],
      "comment_styles": "clojure_style"
    },
    {
      "name": "COBOL",
      "extension": [
        ".cbl"
      ],
      "comment_styles": "cobol_style"
    },
    {
      "name": "D",
      "extension": [
        ".d"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Dylan",
      "extension": [
        ".dylan"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Fortran",
      "extension": [
        ".f",
        ".f95"
      ],
      "comment_styles": "fortran_style"
    },
    {
      "name": "OCaml",
      "extension": [
        ".ml"
      ],
      "comment_styles": "ocaml_style"
    },
    {
      "name": "Eiffel",
      "extension": [
        ".e"
      ],
      "comment_styles": "eiffel_style"
    },
    {
      "name": "Pascal",
      "extension": [
        ".pas"
      ],
      "comment_styles": "pascal_style"
    },
    {
      "name": "TCL",
      "extension": [
        ".tcl"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Elixir",
      "extension": [
        ".ex",
        ".exs"
      ],
      "comment_styles": "elixir_style"
    },
    {
      "name": "Markdown",
      "extension": [
        ".md"
      ],
      "comment_styles": "no_comment"
    },
    {
      "name": "M4",
      "extension": [
        ".m4"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Kotlin Build Script",
      "extension": [
        ".kts"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "V Lang",
      "extension": [
        ".v",
        ".vsh"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Visual Basic",
      "extension": [
        ".vb"
      ],
      "comment_styles": "basic_style"
    },
    {
      "name": "Batch File",
      "extension": [
        ".bat",
        ".cmd"
      ],
      "comment_styles": "batchfile_style"
    },
    {
      "name": "SCSS",
      "extension": [
        ".scss"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "QML",
      "extension": [
        ".qml"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Verilog",
      "extension": [
        ".vlg",
        ".verilog"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "F#",
      "extension": [
        ".fs"
      ],
      "comment_styles": "fsharp_style"
    },
    {
      "name": "Jupyter Notebook",
      "extension": [
        ".ipynb"
      ],
      "comment_styles": "no_comment"
    },
    {
      "name": "VHDL",
      "extension": [
        ".vhd"
      ],
      "comment_styles": "vhdl_style"
    },
    {
      "name": "Fish Script",
      "extension": [
        ".fish"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Tex",
      "extension": [
        ".tex"
      ],
      "comment_styles": "tex_style"
    },
    {
      "name": "KornShell",
      "extension": [
        ".ksh"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Zsh",
      "extension": [
        ".zsh"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Dockerfile",
      "extension": [
        "Dockerfile"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Apple Script",
      "extension": [
        ".applescript"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Unreal Script",
      "extension": [
        ".uc"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "ASP.NET",
      "extension": [
        ".asp"
      ],
      "comment_styles": "html_style"
    },
    {
      "name": "ASPX",
      "extension": [
        ".aspx"
      ],
      "comment_styles": "html_style"
    },
    {
      "name": "GLSL",
      "extension": [
        ".glsl"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Cg",
      "extension": [
        ".cg"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "HLSL",
      "extension": [
        ".hlsl"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "OpenCL",
      "extension": [
        ".cl"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "CUDA",
      "extension": [
        ".cu"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "GraphQL",
      "extension": [
        ".gql",
        ".graphql"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Apache Config",
      "extension": [
        ".htaccess"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "TOML",
      "extension": [
        ".toml"
      ],
      "comment_styles": "shell_style"
    },
    {
      "name": "Jinja2 Template",
      "extension": [
        ".j2"
      ],
      "comment_styles": "no_comment"
    },
    {
      "name": "GameMarker Studio",
      "extension": [
        ".gml"
      ],
      "comment_styles": "c_style"
    },
    {
      "name": "Brainfuck",
      "extension": [
        ".bf"
      ],
      "comment_styles": "no_comment"
    },
    {
      "name": "Chef",
      "extension": [
        ".chef"
      ],
      "comment_styles": "no_comment"
    },
    {
      "name": "APL",
      "extension": [
        ".apl"
      ],
      "comment_styles": "no_comment"
    }
  ]
}
