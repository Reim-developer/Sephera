## Introduction
---

* Sephera offers maximum customization and supports an unlimited number of programming languages for detection and LOC counting. Below is a basic guide on how to configure it.

* First, use the following command to create the configuration for your project:
```bash
sephera cfg-language
```

* This command will generate the SepheraCfg.yml file, including its basic syntax. You will find the SepheraCfg.yml file with the following content:
```yml
# Auto generate by Sephera
# GitHub: https://github.com/reim-developer/Sephera

# Comment style for your programming language.
comment_styles:
    python_style:
        # Comment of language. If the language 
        # does not support it comment type,
        # you can set this field to null.

        single_line: '#' 
        multi_line_start: '"\"\"\'
        multi_line_end: '"\"\"\'

# Languages extension, and style
languages:
    # Language name.
    - name: Python

      # Language extension.
      extension:
        - .py
      
      # Language comment style.
      comment_styles: python_style
```

* Explanation of the values: `comment_styles:`

    > `python_style:` This is the name of the style, and you can customize it as you like. However, it’s recommended to give it a descriptive name that is easy to understand.

---

* This section defines how comments are handled for a programming language. It includes 3 fields:

    > `single_line:` This is where you define the syntax for single-line comments. For example, in the default configuration for Python, it is #. If the language doesn't support single-line comments, just assign a null value, like so: `single_line: null`
    

    > `multi_line_start and multi_line_end:` These fields define the syntax for multi-line comments. For example, for languages like C, it would be `/*` and `*/`. 
    
    > If the language doesn't have multi-line comments, simply assign null values to both fields, like so:

    > `multi_line_start: null`

    > `multi_line_end: null`

---

* Explanation of values `languages`:
    
    > `name:` This is where you define the name of the programming language, such as "Python".

    > `extension:` Here, you list the file extensions that are associated with the language. For example, Python files have the .py extension.

    > `comment_styles:` This links to the comment style defined earlier in the configuration (in this case, python_style). This tells the tool which comment syntax to use for the language.