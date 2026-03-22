pub(super) struct FixtureTemplate {
    pub extension: &'static str,
    pub body: &'static str,
    pub repeat_divisor: usize,
}

const RUST_TEMPLATE: &str = r"// Benchmark fixture
/* multi-line
 * block comment
 */
pub fn compute(value: usize) -> usize {
    let mut total = value;
    for offset in 0..32 {
        total += offset;
    }
    total
}
";

const PYTHON_TEMPLATE: &str = r"# Benchmark fixture
def compute(value: int) -> int:
    total = value
    for offset in range(32):
        total += offset
    return total
";

const TYPESCRIPT_TEMPLATE: &str = r"// Benchmark fixture
export function compute(value: number): number {
    let total = value;
    for (let offset = 0; offset < 32; offset += 1) {
        total += offset;
    }
    return total;
}
";

const HTML_TEMPLATE: &str = r#"<!-- Benchmark fixture -->
<!DOCTYPE html>
<html lang="en">
  <body>
    <main>
      <h1>Benchmark Fixture</h1>
    </main>
  </body>
</html>
"#;

const JSON_TEMPLATE: &str = r#"{
  "fixture": true,
  "name": "benchmark",
  "items": [1, 2, 3, 4]
}
"#;

const TOML_TEMPLATE: &str = r#"
title = "benchmark"

[metadata]
fixture = true
count = 4
"#;

const SHELL_TEMPLATE: &str = r#"# Benchmark fixture
echo "benchmark"
"#;

pub(super) const FIXTURE_TEMPLATES: &[FixtureTemplate] = &[
    FixtureTemplate {
        extension: "rs",
        body: RUST_TEMPLATE,
        repeat_divisor: 1,
    },
    FixtureTemplate {
        extension: "py",
        body: PYTHON_TEMPLATE,
        repeat_divisor: 1,
    },
    FixtureTemplate {
        extension: "ts",
        body: TYPESCRIPT_TEMPLATE,
        repeat_divisor: 1,
    },
    FixtureTemplate {
        extension: "html",
        body: HTML_TEMPLATE,
        repeat_divisor: 4,
    },
    FixtureTemplate {
        extension: "json",
        body: JSON_TEMPLATE,
        repeat_divisor: 4,
    },
    FixtureTemplate {
        extension: "toml",
        body: TOML_TEMPLATE,
        repeat_divisor: 4,
    },
    FixtureTemplate {
        extension: "sh",
        body: SHELL_TEMPLATE,
        repeat_divisor: 8,
    },
];
