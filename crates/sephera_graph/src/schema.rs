//! Schema-change impact: a `.proto` change joined to the in-repo code
//! that consumes exactly that changed element, through the generated-code
//! boundary, with the evidence tier of every claim stated.
//!
//! The schema side is a hand-written proto3 parser (strict: a file it
//! cannot fully parse aborts the run, never a partial index) and a
//! diff whose identity anchor is the field number: a field renamed
//! with the same number and type is provably the same field, which is
//! what lets this command prove a rename that `breaking` can only read
//! as removal + addition. The consumer side is tiered: A1 names the
//! generated message (proven, message-level), A2 names the field in a
//! syntactic position tied to the generated type (proven, field-level),
//! A3 names the field in a file that imports the generated module
//! (flagged name-match, never gated). A bare-substring tier is refused
//! outright: there is no `--include-weak` flag, because a weak tier
//! users can enable becomes the tier everyone enables and distrusts.
//!
//! One design deviation is documented here because reading the code
//! disproved the design's reuse claim: Tier A1 is not `breaking`'s
//! `consumers()` alone. Go import edges carry empty `items` (a
//! whole-module import records no names) and Go resolution targets
//! the first file of a package directory, so `consumers()`'s symbol
//! match never fires for `pb.User`; Python plain imports
//! (`import foo.bar_pb2` + qualified use) likewise produce no item
//! edge. Only Python from-imports fire via the gap-half rule. A1 is
//! therefore `consumers()` UNION an import-gated AST message check
//! (Go selector `alias.Message`, Python attribute chain rooted at an
//! imported generated module, Python identifier equal to a from-imported
//! message name). The evidence strength is identical — a whole
//! identifier plus an import gate — and the corpus proves
//! precision/recall per tier.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::breaking::{ResolutionOracle, consumers, downstream_of};
use crate::types::GraphReport;
use sephera_ignore::IgnoreMatcher;

/// What changed about one schema element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaChange {
    /// The element is gone from the new side.
    Removed,
    /// Same number and type, new name. A proof, not a guess: the
    /// number is the identity.
    Renamed {
        /// The old name.
        from: String,
        /// The new name.
        to: String,
    },
    /// Same name and number, different type.
    TypeChanged {
        /// The old type spelling.
        from: String,
        /// The new type spelling.
        to: String,
    },
    /// Same name, different number: a wire break and a source break.
    /// Field numbers are `u32` and enum numbers are `i32`; both fit
    /// in `i64`, so one variant serves both.
    NumberChanged {
        /// The old number.
        from: i64,
        /// The new number.
        to: i64,
    },
    /// Same name and number, `optional` vs `repeated`.
    LabelChanged {
        /// The old label.
        from: String,
        /// The new label.
        to: String,
    },
    /// Same name and number, oneof membership changed.
    OneofChanged {
        /// The old oneof, if the field was in one.
        from: Option<String>,
        /// The new oneof, if the field is in one.
        to: Option<String>,
    },
    /// An rpc method's input message changed.
    InputChanged {
        /// The old input message.
        from: String,
        /// The new input message.
        to: String,
    },
    /// An rpc method's output message changed.
    OutputChanged {
        /// The old output message.
        from: String,
        /// The new output message.
        to: String,
    },
}

impl SchemaChange {
    /// The change in reviewer words: what changed, not a code.
    ///
    /// Both renderers (the CLI report and the MCP Markdown) call this
    /// one function, so the two can never drift the way the CLI/MCP
    /// header drifted on test-priority.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            SchemaChange::Removed => "removed".to_owned(),
            SchemaChange::Renamed { from, to } => {
                format!("renamed from `{from}` to `{to}`")
            }
            SchemaChange::TypeChanged { from, to } => {
                format!("type changed from `{from}` to `{to}`")
            }
            SchemaChange::NumberChanged { from, to } => {
                format!("field number changed from {from} to {to}")
            }
            SchemaChange::LabelChanged { from, to } => {
                format!("label changed from `{from}` to `{to}`")
            }
            SchemaChange::OneofChanged { from, to } => {
                let from = from.as_deref().unwrap_or("(none)");
                let to = to.as_deref().unwrap_or("(none)");
                format!("oneof membership changed from `{from}` to `{to}`")
            }
            SchemaChange::InputChanged { from, to } => {
                format!("input changed from `{from}` to `{to}`")
            }
            SchemaChange::OutputChanged { from, to } => {
                format!("output changed from `{from}` to `{to}`")
            }
        }
    }
}

/// How one file evidences consuming a schema element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumerVia {
    /// The file imports the generated module and names the generated
    /// message, enum, or service symbol (Tier A1).
    Import,
    /// A Go composite literal names the message type and the field
    /// (Tier A2).
    StructLiteral,
    /// A Python constructor keyword names the field on a call to the
    /// generated message (Tier A2).
    ConstructorKwarg,
    /// An enum constant reference names the value (Tier A2).
    EnumConst,
    /// A method call names the rpc method in a file importing the
    /// generated package (proven for method breaks only).
    MethodCall,
    /// A field access names the field in a file importing the
    /// generated module, receiver type unknown (Tier A3).
    FieldAccess,
    /// A string literal equal to the field's `json_name` in a file
    /// importing the generated module (Tier A3).
    JsonKey,
}

impl ConsumerVia {
    /// The evidence in reviewer words: what mechanism, not a code.
    ///
    /// Both renderers call this one function, for the same
    /// no-drift reason as [`SchemaChange::describe`].
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            ConsumerVia::Import => "import",
            ConsumerVia::StructLiteral => "struct literal",
            ConsumerVia::ConstructorKwarg => "constructor keyword",
            ConsumerVia::EnumConst => "enum constant",
            ConsumerVia::MethodCall => "method call",
            ConsumerVia::FieldAccess => "field access",
            ConsumerVia::JsonKey => "JSON key",
        }
    }
}

/// One file consuming one schema element, with its evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaConsumer {
    /// The consuming file, repo-root-relative.
    pub file: String,
    /// What evidences the consumption.
    pub via: ConsumerVia,
}

/// One schema break: a changed element joined to its consumers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaBreak {
    /// The `.proto` file the element lives in, repo-root-relative.
    pub schema_file: String,
    /// The stable element path: `package.Message.field`,
    /// `package.Enum.VALUE`, `package.Service.Method`, or
    /// `package.Message` for a whole-message break.
    pub element: String,
    /// What changed.
    pub change: SchemaChange,
    /// A reviewer note that never suppresses the break, e.g. that a
    /// removed field's number is now reserved.
    pub note: Option<String>,
    /// Proven consumers: A1 for message/enum/service breaks, A2 for
    /// field/enum-value breaks, A1 + MethodCall for method breaks.
    pub proven: Vec<SchemaConsumer>,
    /// Name-match consumers (Tier A3), flagged weaker evidence.
    pub named: Vec<SchemaConsumer>,
    /// Files transitively importing a proven consumer's file:
    /// informational, never gated.
    pub downstream: Vec<String>,
}

/// An element present only on the new side: informational, never gated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaElement {
    /// The `.proto` file the element lives in.
    pub schema_file: String,
    /// The stable element path.
    pub element: String,
}

/// The report `sephera schema-impact` renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SchemaImpactReport {
    /// The older reference.
    pub from_ref: String,
    /// The newer reference, or "working tree".
    pub to_ref: String,
    /// Changed elements, most exposed first.
    pub breaks: Vec<SchemaBreak>,
    /// New elements, informational only.
    pub added: Vec<SchemaElement>,
    /// Coverage notes: generated-code absence, assumed generator
    /// mappings, ambiguous mappings, vendored well-known protos.
    /// A run that cannot see consumers says so here instead of
    /// claiming "0 consumers".
    pub coverage: Vec<String>,
}

/// One field of one message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaField {
    /// The field name as written in the `.proto` file.
    pub name: String,
    /// The wire identity. A rename keeps this; a number change does not.
    pub number: u32,
    /// The type spelling: a scalar, a message/enum reference, or
    /// `map<K, V>`.
    pub type_name: String,
    /// `optional`, `repeated`, or empty for the proto3 default.
    pub label: String,
    /// The `json_name`: explicit when the field declares one, else the
    /// protobuf-derived camelCase.
    pub json_name: String,
    /// The oneof the field belongs to, if any.
    pub oneof: Option<String>,
    /// Whether the field is a `map<,>` field.
    pub is_map: bool,
}

/// One message: its fields, keyed by number and by name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchemaMessage {
    /// Fields by number.
    pub by_number: BTreeMap<u32, SchemaField>,
    /// Fields by name.
    pub by_name: BTreeMap<String, u32>,
    /// Reserved numbers.
    pub reserved_numbers: BTreeSet<u32>,
}

/// One enum: its values, keyed by number and by name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchemaEnum {
    /// Values by number.
    pub by_number: BTreeMap<i32, String>,
    /// Values by name.
    pub by_name: BTreeMap<String, i32>,
    /// Reserved numbers.
    pub reserved_numbers: BTreeSet<i32>,
}

/// One rpc method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaMethod {
    /// The input message reference.
    pub input: String,
    /// The output message reference.
    pub output: String,
}

/// One `.proto` file's schema.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchemaFile {
    /// The package, empty when the file declares none.
    pub package: String,
    /// Messages by fully qualified name (`package.Outer.Inner`).
    pub messages: BTreeMap<String, SchemaMessage>,
    /// Enums by fully qualified name.
    pub enums: BTreeMap<String, SchemaEnum>,
    /// Services by fully qualified name, each method by name.
    pub services: BTreeMap<String, BTreeMap<String, SchemaMethod>>,
    /// Whether the file vendors a well-known proto
    /// (`google/protobuf/*.proto`): indexed like any other file, with
    /// a coverage note saying so.
    pub vendors_well_known: bool,
}

/// The schema index of one tree: `.proto` files by repo-relative path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchemaIndex {
    /// Files by repo-relative path.
    pub files: BTreeMap<String, SchemaFile>,
}

/// A `.proto` file the parser rejected, or a schema walk failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    /// The file at fault, empty for walk-level failures.
    pub file: String,
    /// What went wrong, naming the construct.
    pub message: String,
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.file.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{}: {}", self.file, self.message)
        }
    }
}

impl std::error::Error for SchemaError {}

// ---------------------------------------------------------------------------
// The proto3 parser: strict, hand-written, no partial indexes.
// ---------------------------------------------------------------------------

/// One token of a `.proto` file. Comments are skipped by the tokenizer,
/// so they can never become evidence or a declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// An identifier or keyword.
    Ident(String),
    /// An integer literal, possibly negative.
    Number(i64),
    /// A string literal, unescaped.
    Str(String),
    /// A single punctuation character.
    Punct(char),
}

/// Tokenize a `.proto` file.
///
/// # Errors
///
/// Returns a [`SchemaError`] naming the file when the text contains a
/// character the tokenizer cannot read, an unterminated string, or an
/// unterminated comment.
fn tokenize(file: &str, text: &str) -> Result<Vec<Token>, SchemaError> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    let fail = |message: &str| SchemaError {
        file: file.to_owned(),
        message: message.to_owned(),
    };
    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            ' ' | '\t' | '\r' | '\n' => i += 1,
            '/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            '/' if bytes.get(i + 1) == Some(&b'*') => {
                let mut j = i + 2;
                let mut closed = false;
                while j + 1 < bytes.len() {
                    if bytes[j] == b'*' && bytes[j + 1] == b'/' {
                        closed = true;
                        break;
                    }
                    j += 1;
                }
                if !closed {
                    return Err(fail("unterminated block comment"));
                }
                i = j + 2;
            }
            '"' | '\'' => {
                let quote = bytes[i];
                let mut j = i + 1;
                let mut value = String::new();
                let mut closed = false;
                while j < bytes.len() {
                    let b = bytes[j];
                    if b == quote {
                        closed = true;
                        break;
                    }
                    if b == b'\\' && j + 1 < bytes.len() {
                        let escaped = bytes[j + 1] as char;
                        value.push(match escaped {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            other => other,
                        });
                        j += 2;
                    } else {
                        value.push(b as char);
                        j += 1;
                    }
                }
                if !closed {
                    return Err(fail("unterminated string literal"));
                }
                tokens.push(Token::Str(value));
                i = j + 1;
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let mut j = i;
                while j < bytes.len() {
                    let b = bytes[j] as char;
                    if b.is_ascii_alphanumeric() || b == '_' {
                        j += 1;
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Ident(text[i..j].to_owned()));
                i = j;
            }
            '0'..='9' => {
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                let literal = &text[i..j];
                let value: i64 = literal
                    .parse()
                    .map_err(|_| fail("integer literal out of range"))?;
                tokens.push(Token::Number(value));
                i = j;
            }
            '-' => {
                // A negative number only when a digit follows; a lone
                // `-` is a tokenizer error, not a construct to guess at.
                if bytes.get(i + 1).is_some_and(|b| b.is_ascii_digit()) {
                    let mut j = i + 1;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    let literal = &text[i..j];
                    let value: i64 = literal
                        .parse()
                        .map_err(|_| fail("integer literal out of range"))?;
                    tokens.push(Token::Number(value));
                    i = j;
                } else {
                    return Err(fail("unexpected `-`"));
                }
            }
            ';' | '{' | '}' | '(' | ')' | '[' | ']' | '<' | '>' | '='
            | '.' | ',' | ':' => {
                // `:` is not a proto construct the parser interprets,
                // but it appears inside text-format option aggregates
                // (`repeated = {unique: true, items: {...}}`), which
                // the parser skips rather than reads — and a skipped
                // region still has to tokenize. Without it, one
                // validating option in a real repo failed the whole
                // run at the tokenizer, before any parsing began.
                tokens.push(Token::Punct(c));
                i += 1;
            }
            _ => {
                return Err(fail(&format!(
                    "unexpected character `{c}`"
                )));
            }
        }
    }
    Ok(tokens)
}

/// A cursor over tokens with strict expectations: every mismatch is a
/// named error, never a skip-and-hope.
struct Cursor<'a> {
    file: &'a str,
    tokens: Vec<Token>,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(file: &'a str, tokens: Vec<Token>) -> Self {
        Self { file, tokens, pos: 0 }
    }

    fn error(&self, message: &str) -> SchemaError {
        SchemaError {
            file: self.file.to_owned(),
            message: message.to_owned(),
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn peek_ident(&self) -> Option<&str> {
        match self.peek() {
            Some(Token::Ident(name)) => Some(name.as_str()),
            _ => None,
        }
    }

    fn eat_ident(&mut self, name: &str) -> bool {
        if self.peek_ident() == Some(name) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_ident(&mut self, what: &str) -> Result<String, SchemaError> {
        match self.next() {
            Some(Token::Ident(name)) => Ok(name),
            other => Err(self.error(&format!(
                "expected {what}, found {}",
                describe(other.as_ref())
            ))),
        }
    }

    fn expect_punct(&mut self, want: char) -> Result<(), SchemaError> {
        match self.next() {
            Some(Token::Punct(c)) if c == want => Ok(()),
            other => Err(self.error(&format!(
                "expected `{want}`, found {}",
                describe(other.as_ref())
            ))),
        }
    }

    fn eat_punct(&mut self, want: char) -> bool {
        if self.peek() == Some(&Token::Punct(want)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// Skip a balanced region up to and including the closing punct,
    /// e.g. an enum-value option list or a service-method body.
    /// The caller has already consumed the opening punct, so the
    /// depth starts at one: starting at zero would drive the first
    /// closer to minus one, never return, and run to end of file
    /// reporting a block that was never unterminated — which is
    /// exactly what this function did until a real repo with
    /// method bodies proved it.
    fn skip_balanced(&mut self, open: char, close: char) -> Result<(), SchemaError> {
        let mut depth = 1;
        loop {
            match self.next() {
                Some(Token::Punct(c)) if c == open => depth += 1,
                Some(Token::Punct(c)) if c == close => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Some(_) => {}
                None => {
                    return Err(self.error(&format!(
                        "unterminated `{open}…{close}` block"
                    )));
                }
            }
        }
    }

    /// Skip to the next `;` at nesting depth zero, consuming it.
    fn skip_to_semicolon(&mut self) -> Result<(), SchemaError> {
        let mut depth = 0;
        loop {
            match self.next() {
                Some(Token::Punct('{'))
                | Some(Token::Punct('('))
                | Some(Token::Punct('[')) => depth += 1,
                Some(Token::Punct('}'))
                | Some(Token::Punct(')'))
                | Some(Token::Punct(']')) => depth -= 1,
                Some(Token::Punct(';')) if depth == 0 => return Ok(()),
                Some(_) => {}
                None => {
                    return Err(self.error(
                        "unterminated statement (expected `;`)",
                    ));
                }
            }
        }
    }
}

fn describe(token: Option<&Token>) -> String {
    match token {
        None => "end of file".to_owned(),
        Some(Token::Ident(name)) => format!("identifier `{name}`"),
        Some(Token::Number(value)) => format!("number `{value}`"),
        Some(Token::Str(_)) => "a string literal".to_owned(),
        Some(Token::Punct(c)) => format!("`{c}`"),
    }
}

/// The protobuf-derived `json_name`: lower camelCase of the field name.
#[must_use]
pub fn derived_json_name(field: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = false;
    for c in field.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse one `.proto` file's text into its schema.
///
/// # Errors
///
/// Returns a [`SchemaError`] naming the file when the file declares
/// proto2 (or no syntax, which defaults to proto2), uses a construct
/// the parser does not support, or is malformed in any way. There is
/// no partial result: an unparseable file aborts the run.
pub fn parse_schema_file(file: &str, text: &str) -> Result<SchemaFile, SchemaError> {
    let tokens = tokenize(file, text)?;
    let mut cursor = Cursor::new(file, tokens);
    let mut schema = SchemaFile::default();
    let mut saw_syntax = false;

    loop {
        match cursor.peek_ident() {
            None => break,
            Some("syntax") => {
                cursor.next();
                cursor.expect_punct('=')?;
                match cursor.next() {
                    Some(Token::Str(value)) if value == "proto3" => {
                        saw_syntax = true;
                    }
                    Some(Token::Str(value)) if value == "proto2" => {
                        return Err(cursor.error(
                            "proto2 is not supported in v1",
                        ));
                    }
                    Some(Token::Str(_)) => {
                        return Err(cursor.error(
                            "unsupported syntax version (expected \"proto3\")",
                        ));
                    }
                    other => {
                        return Err(cursor.error(&format!(
                            "expected a syntax string, found {}",
                            describe(other.as_ref())
                        )));
                    }
                }
                cursor.expect_punct(';')?;
            }
            Some("package") => {
                cursor.next();
                let mut package = cursor.expect_ident("a package name")?;
                while cursor.eat_punct('.') {
                    let part = cursor.expect_ident("a package segment")?;
                    package.push('.');
                    package.push_str(&part);
                }
                cursor.expect_punct(';')?;
                schema.package = package;
            }
            Some("import") => {
                cursor.next();
                // `public` / `weak` imports are recorded the same way:
                // the import list is informational in v1.
                cursor.eat_ident("public");
                cursor.eat_ident("weak");
                match cursor.next() {
                    Some(Token::Str(_)) => {}
                    other => {
                        return Err(cursor.error(&format!(
                            "expected an import path string, found {}",
                            describe(other.as_ref())
                        )));
                    }
                }
                cursor.expect_punct(';')?;
            }
            Some("option") => {
                cursor.next();
                cursor.skip_to_semicolon()?;
            }
            Some("message") => {
                cursor.next();
                let name = cursor.expect_ident("a message name")?;
                parse_message(&mut cursor, &mut schema, &name, "")?;
            }
            Some("enum") => {
                cursor.next();
                let name = cursor.expect_ident("an enum name")?;
                parse_enum(&mut cursor, &mut schema, &name, "")?;
            }
            Some("service") => {
                cursor.next();
                let name = cursor.expect_ident("a service name")?;
                parse_service(&mut cursor, &mut schema, &name, "")?;
            }
            Some("extend") => {
                cursor.next();
                // An extend block declares custom options on
                // another type (the usual shape is `extend
                // google.protobuf.MethodOptions { ... }`). It
                // introduces no message, enum, or service, so
                // v1's diff has nothing to track in it — skip
                // the block. Real repos declare all their custom
                // options this way; rejecting it failed whole
                // runs on files that are valid proto3.
                let _target = parse_type(&mut cursor)?;
                cursor.expect_punct('{')?;
                cursor.skip_balanced('{', '}')?;
            }
            Some(";") => {
                cursor.next();
            }
            Some(other) => {
                return Err(cursor.error(&format!(
                    "unsupported top-level construct `{other}`"
                )));
            }
        }
    }

    if !saw_syntax {
        return Err(cursor.error(
            "missing syntax declaration (a file without one defaults to proto2, which is not supported in v1)",
        ));
    }
    if file.starts_with("google/protobuf/") {
        schema.vendors_well_known = true;
    }
    Ok(schema)
}

/// The qualified name of a nested element: `package.Outer.Inner`, or
/// the bare name when there is no package and no prefix.
fn qualified(package: &str, prefix: &str, name: &str) -> String {
    let mut out = String::new();
    if !package.is_empty() {
        out.push_str(package);
        out.push('.');
    }
    if !prefix.is_empty() {
        out.push_str(prefix);
        out.push('.');
    }
    out.push_str(name);
    out
}

/// Parse a message body. `prefix` is the enclosing message path
/// (without package), empty at top level.
fn parse_message(
    cursor: &mut Cursor<'_>,
    schema: &mut SchemaFile,
    name: &str,
    prefix: &str,
) -> Result<(), SchemaError> {
    let full = qualified(&schema.package.clone(), prefix, name);
    cursor.expect_punct('{')?;
    let mut message = SchemaMessage::default();
    // The nested prefix for children: `Outer` inside package becomes
    // `Outer`, and `Inner` inside `Outer` becomes `Outer.Inner`.
    let child_prefix = if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}.{name}")
    };
    loop {
        // A body whose next token is the closer is empty, not
        // unterminated: `peek_ident` sees no identifier in `}`,
        // so without this check an empty body fell into the
        // `None` arm and errored. Real repos have empty messages
        // (`message TaskServiceDeleteResponse {}`), empty services,
        // and empty enums; all three loops below share this shape.
        if cursor.eat_punct('}') {
            break;
        }
        match cursor.peek_ident() {
            None => {
                return Err(cursor
                    .error("unterminated message body (expected `}`)"));
            }
            Some("message") => {
                cursor.next();
                let nested = cursor.expect_ident("a message name")?;
                parse_message(cursor, schema, &nested, &child_prefix)?;
            }
            Some("enum") => {
                cursor.next();
                let nested = cursor.expect_ident("an enum name")?;
                parse_enum(cursor, schema, &nested, &child_prefix)?;
            }
            Some("oneof") => {
                cursor.next();
                let oneof = cursor.expect_ident("a oneof name")?;
                cursor.expect_punct('{')?;
                loop {
                    // Same empty-body shape as the message loop.
                    if cursor.eat_punct('}') {
                        break;
                    }
                    match cursor.peek_ident() {
                        None => {
                            return Err(cursor.error(
                                "unterminated oneof body (expected `}`)",
                            ));
                        }
                        Some("option") => {
                            cursor.next();
                            cursor.skip_to_semicolon()?;
                        }
                        Some(";") => {
                            cursor.next();
                        }
                        Some(_) => {
                            parse_field(cursor, &mut message, Some(&oneof))?;
                        }
                    }
                }
            }
            Some("reserved") => {
                cursor.next();
                parse_reserved(cursor, &mut message.reserved_numbers, None)?;
            }
            Some("option") => {
                cursor.next();
                cursor.skip_to_semicolon()?;
            }
            Some("extensions") | Some("extend") | Some("group") => {
                return Err(cursor.error(
                    "proto2-only construct in a proto3 file",
                ));
            }
            Some(";") => {
                cursor.next();
            }
            Some(_) => {
                parse_field(cursor, &mut message, None)?;
            }
        }
    }
    schema.messages.insert(full, message);
    Ok(())
}

/// Parse `reserved` numbers and names. Only numbers are kept: names
/// matter for future fields, not for the diff.
fn parse_reserved(
    cursor: &mut Cursor<'_>,
    numbers: &mut BTreeSet<u32>,
    _names: Option<&mut BTreeSet<String>>,
) -> Result<(), SchemaError> {
    loop {
        match cursor.next() {
            Some(Token::Number(value)) => {
                let start = u32::try_from(value).map_err(|_| {
                    cursor.error("reserved number out of range")
                })?;
                if cursor.eat_ident("to") {
                    match cursor.next() {
                        Some(Token::Ident(max)) if max == "max" => {
                            numbers.insert(start);
                        }
                        Some(Token::Number(end)) => {
                            let end = u32::try_from(end).map_err(|_| {
                                cursor.error("reserved number out of range")
                            })?;
                            for number in start..=end {
                                numbers.insert(number);
                            }
                        }
                        other => {
                            return Err(cursor.error(&format!(
                                "expected a reserved range end, found {}",
                                describe(other.as_ref())
                            )));
                        }
                    }
                } else {
                    numbers.insert(start);
                }
            }
            Some(Token::Str(_)) => {}
            Some(Token::Punct(',')) => {}
            Some(Token::Punct(';')) => return Ok(()),
            other => {
                return Err(cursor.error(&format!(
                    "expected a reserved number or name, found {}",
                    describe(other.as_ref())
                )));
            }
        }
    }
}

/// Parse one field (or oneof member) into the message.
fn parse_field(
    cursor: &mut Cursor<'_>,
    message: &mut SchemaMessage,
    oneof: Option<&str>,
) -> Result<(), SchemaError> {
    let mut label = String::new();
    if let Some(word) = cursor.peek_ident() {
        match word {
            "optional" | "repeated" => {
                label = word.to_owned();
                cursor.next();
            }
            "required" => {
                return Err(cursor.error(
                    "required fields are proto2-only and not supported in v1",
                ));
            }
            _ => {}
        }
    }
    // The type: `map<K, V>`, a scalar, or a dotted message/enum ref.
    // A leading `.` means a fully qualified reference.
    let type_name: String;
    if cursor.eat_ident("map") {
        cursor.expect_punct('<')?;
        let key = cursor.expect_ident("a map key type")?;
        cursor.expect_punct(',')?;
        let value = parse_type(cursor)?;
        cursor.expect_punct('>')?;
        type_name = format!("map<{key}, {value}>");
        let field_name = cursor.expect_ident("a field name")?;
        return finish_field(
            cursor,
            message,
            field_name,
            type_name,
            label,
            oneof,
            true,
        );
    }
    type_name = parse_type(cursor)?;
    let field_name = cursor.expect_ident("a field name")?;
    finish_field(cursor, message, field_name, type_name, label, oneof, false)
}

/// Parse a type reference: dotted identifiers, with an optional leading dot.
fn parse_type(cursor: &mut Cursor<'_>) -> Result<String, SchemaError> {
    let mut type_name = String::new();
    if cursor.eat_punct('.') {
        type_name.push('.');
    }
    let first = cursor.expect_ident("a field type")?;
    type_name.push_str(&first);
    while cursor.eat_punct('.') {
        let part = cursor.expect_ident("a type segment")?;
        type_name.push('.');
        type_name.push_str(&part);
    }
    Ok(type_name)
}

/// Finish a field: `= number [options] ;`, recording it by number.
fn finish_field(
    cursor: &mut Cursor<'_>,
    message: &mut SchemaMessage,
    field_name: String,
    type_name: String,
    label: String,
    oneof: Option<&str>,
    is_map: bool,
) -> Result<(), SchemaError> {
    cursor.expect_punct('=')?;
    let number = match cursor.next() {
        Some(Token::Number(value)) => {
            u32::try_from(value).map_err(|_| {
                cursor.error("field number out of range")
            })?
        }
        other => {
            return Err(cursor.error(&format!(
                "expected a field number, found {}",
                describe(other.as_ref())
            )));
        }
    };
    if number == 0 || number > 536_870_911 {
        return Err(cursor.error("field number out of range"));
    }
    let mut json_name: Option<String> = None;
    if cursor.eat_punct('[') {
        // Field options: skip balanced brackets, fishing out an
        // explicit `json_name` if one is declared.
        let mut depth = 1;
        while depth > 0 {
            match cursor.next() {
                Some(Token::Punct('[')) => depth += 1,
                Some(Token::Punct(']')) => depth -= 1,
                Some(Token::Ident(name)) if name == "json_name" => {
                    cursor.expect_punct('=')?;
                    match cursor.next() {
                        Some(Token::Str(value)) => json_name = Some(value),
                        other => {
                            return Err(cursor.error(&format!(
                                "expected a json_name string, found {}",
                                describe(other.as_ref())
                            )));
                        }
                    }
                }
                Some(_) => {}
                None => {
                    return Err(cursor
                        .error("unterminated field options (expected `]`"));
                }
            }
        }
    }
    cursor.expect_punct(';')?;
    let json_name =
        json_name.unwrap_or_else(|| derived_json_name(&field_name));
    if message.by_number.contains_key(&number) {
        return Err(cursor.error(&format!(
            "duplicate field number {number} in one message"
        )));
    }
    if message.by_name.contains_key(&field_name) {
        return Err(cursor.error(&format!(
            "duplicate field name `{field_name}` in one message"
        )));
    }
    message.by_name.insert(field_name.clone(), number);
    message.by_number.insert(number, SchemaField {
        name: field_name,
        number,
        type_name,
        label,
        json_name,
        oneof: oneof.map(str::to_owned),
        is_map,
    });
    Ok(())
}

/// Parse an enum body.
fn parse_enum(
    cursor: &mut Cursor<'_>,
    schema: &mut SchemaFile,
    name: &str,
    prefix: &str,
) -> Result<(), SchemaError> {
    let full = qualified(&schema.package.clone(), prefix, name);
    cursor.expect_punct('{')?;
    let mut enumeration = SchemaEnum::default();
    loop {
        // Same empty-body shape as the message loop.
        if cursor.eat_punct('}') {
            break;
        }
        match cursor.peek_ident() {
            None => {
                return Err(
                    cursor.error("unterminated enum body (expected `}`)")
                );
            }
            Some("option") => {
                cursor.next();
                cursor.skip_to_semicolon()?;
            }
            Some("reserved") => {
                cursor.next();
                parse_reserved_enum(cursor, &mut enumeration)?;
            }
            Some(";") => {
                cursor.next();
            }
            Some(_) => {
                let value_name = cursor.expect_ident("an enum value name")?;
                cursor.expect_punct('=')?;
                let number = match cursor.next() {
                    Some(Token::Number(value)) => {
                        i32::try_from(value).map_err(|_| {
                            cursor.error("enum value number out of range")
                        })?
                    }
                    other => {
                        return Err(cursor.error(&format!(
                            "expected an enum value number, found {}",
                            describe(other.as_ref())
                        )));
                    }
                };
                if cursor.eat_punct('[') {
                    cursor.skip_balanced('[', ']')?;
                }
                cursor.expect_punct(';')?;
                if enumeration.by_number.contains_key(&number)
                    && enumeration.by_number.get(&number)
                        != Some(&value_name)
                {
                    // Two names sharing a number need `allow_alias`;
                    // without it the file is malformed proto.
                    return Err(cursor.error(&format!(
                        "duplicate enum number {number} without allow_alias"
                    )));
                }
                enumeration.by_name.insert(value_name.clone(), number);
                enumeration.by_number.insert(number, value_name);
            }
        }
    }
    schema.enums.insert(full, enumeration);
    Ok(())
}

/// Parse an enum's `reserved` clause (numbers, names, ranges).
fn parse_reserved_enum(
    cursor: &mut Cursor<'_>,
    enumeration: &mut SchemaEnum,
) -> Result<(), SchemaError> {
    loop {
        match cursor.next() {
            Some(Token::Number(value)) => {
                let start = i32::try_from(value).map_err(|_| {
                    cursor.error("reserved number out of range")
                })?;
                if cursor.eat_ident("to") {
                    match cursor.next() {
                        Some(Token::Ident(max)) if max == "max" => {
                            enumeration.reserved_numbers.insert(start);
                        }
                        Some(Token::Number(end)) => {
                            let end = i32::try_from(end).map_err(|_| {
                                cursor.error("reserved number out of range")
                            })?;
                            for number in start..=end {
                                enumeration.reserved_numbers.insert(number);
                            }
                        }
                        other => {
                            return Err(cursor.error(&format!(
                                "expected a reserved range end, found {}",
                                describe(other.as_ref())
                            )));
                        }
                    }
                } else {
                    enumeration.reserved_numbers.insert(start);
                }
            }
            Some(Token::Str(_)) => {}
            Some(Token::Punct(',')) => {}
            Some(Token::Punct(';')) => return Ok(()),
            other => {
                return Err(cursor.error(&format!(
                    "expected a reserved number or name, found {}",
                    describe(other.as_ref())
                )));
            }
        }
    }
}

/// Parse a service body: rpc methods with input/output messages.
fn parse_service(
    cursor: &mut Cursor<'_>,
    schema: &mut SchemaFile,
    name: &str,
    prefix: &str,
) -> Result<(), SchemaError> {
    let full = qualified(&schema.package.clone(), prefix, name);
    cursor.expect_punct('{')?;
    let mut methods = BTreeMap::new();
    loop {
        // Same empty-body shape as the message loop.
        if cursor.eat_punct('}') {
            break;
        }
        match cursor.peek_ident() {
            None => {
                return Err(cursor
                    .error("unterminated service body (expected `}`)"));
            }
            Some("option") => {
                cursor.next();
                cursor.skip_to_semicolon()?;
            }
            Some("rpc") => {
                cursor.next();
                let method = cursor.expect_ident("an rpc method name")?;
                cursor.expect_punct('(')?;
                cursor.eat_ident("stream");
                let input = parse_type(cursor)?;
                cursor.expect_punct(')')?;
                if !cursor.eat_ident("returns") {
                    return Err(cursor
                        .error("expected `returns` after an rpc input"));
                }
                cursor.expect_punct('(')?;
                cursor.eat_ident("stream");
                let output = parse_type(cursor)?;
                cursor.expect_punct(')')?;
                if cursor.eat_punct('{') {
                    cursor.skip_balanced('{', '}')?;
                } else {
                    cursor.expect_punct(';')?;
                }
                methods.insert(method, SchemaMethod { input, output });
            }
            Some(";") => {
                cursor.next();
            }
            Some(other) => {
                return Err(cursor.error(&format!(
                    "unsupported service construct `{other}`"
                )));
            }
        }
    }
    schema.services.insert(full, methods);
    Ok(())
}

// ---------------------------------------------------------------------------
// Index building: walking a tree for `.proto` files and consumer texts.
// ---------------------------------------------------------------------------

/// Collect every `.proto` file under `base`, as (relative path, text)
/// pairs, respecting the ignore matcher. Paths use `/` separators, the
/// graph's spelling.
///
/// # Errors
///
/// Returns a [`SchemaError`] when a `.proto` file cannot be read: the
/// schema side never silently drops a file it was supposed to index.
pub fn collect_schema_files(
    base: &Path,
    ignore: &IgnoreMatcher,
) -> Result<Vec<(String, String)>, SchemaError> {
    let mut files = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(relative) = path.strip_prefix(base) else {
                continue;
            };
            if ignore.is_ignored(relative) {
                continue;
            }
            if path.is_dir() {
                if path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
                {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "proto") {
                let relative = relative.to_string_lossy().replace('\\', "/");
                let text = std::fs::read_to_string(&path).map_err(|error| {
                    SchemaError {
                        file: relative.clone(),
                        message: format!("cannot read schema file: {error}"),
                    }
                })?;
                files.push((relative, text));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

/// Collect every Go and Python file under `base`, as (relative path,
/// text) pairs, respecting the ignore matcher. The consumer scan runs
/// over these texts; generated files are found among them by marker
/// or suffix, never by directory.
///
/// # Errors
///
/// Returns a [`SchemaError`] when a file cannot be read.
pub fn collect_consumer_texts(
    base: &Path,
    ignore: &IgnoreMatcher,
) -> Result<Vec<(String, String)>, SchemaError> {
    let mut files = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(relative) = path.strip_prefix(base) else {
                continue;
            };
            if ignore.is_ignored(relative) {
                continue;
            }
            if path.is_dir() {
                if path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
                {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|ext| {
                ext == "go" || ext == "py" || ext == "pyi"
            }) {
                let relative = relative.to_string_lossy().replace('\\', "/");
                let text = std::fs::read_to_string(&path).map_err(|error| {
                    SchemaError {
                        file: relative.clone(),
                        message: format!("cannot read source file: {error}"),
                    }
                })?;
                files.push((relative, text));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

/// Build a schema index from collected `.proto` texts.
///
/// # Errors
///
/// Returns the first [`SchemaError`] from any file: one unparseable
/// file aborts the whole index, because a partial schema index would
/// report breaks it cannot see the other side of.
pub fn build_schema_index(
    files: &[(String, String)],
) -> Result<SchemaIndex, SchemaError> {
    let mut index = SchemaIndex::default();
    for (path, text) in files {
        let schema = parse_schema_file(path, text)?;
        index.files.insert(path.clone(), schema);
    }
    Ok(index)
}

// ---------------------------------------------------------------------------
// The diff: old index vs new index, with the field number as identity.
// ---------------------------------------------------------------------------

/// The CamelCase a generator makes of a `snake_case` field name:
/// `user_id` → `UserId`, `id` → `Id`. This is `protoc-gen-go`'s rule
/// (no initialism handling: `http_server` → `HttpServer`).
#[must_use]
pub fn go_field_name(field: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = true;
    for c in field.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            out.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// A schema change with its element path, before consumers are joined.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingBreak {
    schema_file: String,
    element: String,
    change: SchemaChange,
    note: Option<String>,
    /// The generated symbol the break is about, for the consumer join:
    /// a message, enum, or service qualified name, or a
    /// `message:field` / `enum:value` / `service:method` pair.
    target: BreakTarget,
}

/// What a break's consumers are joined through.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BreakTarget {
    /// A whole message: consumers are A1 message namers.
    Message(String),
    /// A whole enum: consumers are A1 enum namers.
    Enum(String),
    /// A whole service: consumers are A1 service-symbol namers.
    Service(String),
    /// One field: consumers are A2 field-anchored + A3 name-match.
    /// Both names are the OLD side's: consumers are old code, so a
    /// rename's consumers name the old field and the old JSON key,
    /// never the new ones.
    Field {
        /// The message's qualified name.
        message: String,
        /// The field's name on the old side.
        field: String,
        /// The field's `json_name` on the old side.
        json_name: String,
    },
    /// One enum value: consumers are A2 enum-constant namers.
    EnumValue {
        /// The enum's qualified name.
        enumeration: String,
        /// The value's name on the old side.
        value: String,
    },
    /// One rpc method: consumers are A1 service namers + MethodCall.
    Method {
        /// The service's qualified name.
        service: String,
        /// The method's name.
        method: String,
    },
}

/// Diff two schema indexes into pending breaks and informational adds.
/// The consumer join happens in [`measure_schema_impact`]; this is the
/// pure schema half, so the corpus can test it without a code graph.
fn diff_indexes(
    old: &SchemaIndex,
    new: &SchemaIndex,
) -> (Vec<PendingBreak>, Vec<SchemaElement>) {
    let mut breaks = Vec::new();
    let mut added = Vec::new();
    for (path, old_file) in &old.files {
        let Some(new_file) = new.files.get(path) else {
            // A whole file gone: every message, enum, and service in
            // it reads as removed, each with its own consumers.
            for message in old_file.messages.keys() {
                breaks.push(PendingBreak {
                    schema_file: path.clone(),
                    element: message.clone(),
                    change: SchemaChange::Removed,
                    note: None,
                    target: BreakTarget::Message(message.clone()),
                });
            }
            for enumeration in old_file.enums.keys() {
                breaks.push(PendingBreak {
                    schema_file: path.clone(),
                    element: enumeration.clone(),
                    change: SchemaChange::Removed,
                    note: None,
                    target: BreakTarget::Enum(enumeration.clone()),
                });
            }
            for service in old_file.services.keys() {
                breaks.push(PendingBreak {
                    schema_file: path.clone(),
                    element: service.clone(),
                    change: SchemaChange::Removed,
                    note: None,
                    target: BreakTarget::Service(service.clone()),
                });
            }
            continue;
        };
        diff_file(path, old_file, new_file, &mut breaks, &mut added);
    }
    // Files present only on the new side: their elements are added.
    for (path, new_file) in &new.files {
        if old.files.contains_key(path) {
            continue;
        }
        for message in new_file.messages.keys() {
            added.push(SchemaElement {
                schema_file: path.clone(),
                element: message.clone(),
            });
        }
        for enumeration in new_file.enums.keys() {
            added.push(SchemaElement {
                schema_file: path.clone(),
                element: enumeration.clone(),
            });
        }
        for service in new_file.services.keys() {
            added.push(SchemaElement {
                schema_file: path.clone(),
                element: service.clone(),
            });
        }
    }
    (breaks, added)
}

/// Diff one file present on both sides.
fn diff_file(
    path: &str,
    old_file: &SchemaFile,
    new_file: &SchemaFile,
    breaks: &mut Vec<PendingBreak>,
    added: &mut Vec<SchemaElement>,
) {
    for (message, old_message) in &old_file.messages {
        let Some(new_message) = new_file.messages.get(message) else {
            breaks.push(PendingBreak {
                schema_file: path.to_owned(),
                element: message.clone(),
                change: SchemaChange::Removed,
                note: None,
                target: BreakTarget::Message(message.clone()),
            });
            continue;
        };
        diff_message(
            path, message, old_message, new_message, breaks, added,
        );
    }
    for message in new_file.messages.keys() {
        if !old_file.messages.contains_key(message) {
            added.push(SchemaElement {
                schema_file: path.to_owned(),
                element: message.clone(),
            });
        }
    }
    for (enumeration, old_enum) in &old_file.enums {
        let Some(new_enum) = new_file.enums.get(enumeration) else {
            breaks.push(PendingBreak {
                schema_file: path.to_owned(),
                element: enumeration.clone(),
                change: SchemaChange::Removed,
                note: None,
                target: BreakTarget::Enum(enumeration.clone()),
            });
            continue;
        };
        diff_enum(path, enumeration, old_enum, new_enum, breaks, added);
    }
    for enumeration in new_file.enums.keys() {
        if !old_file.enums.contains_key(enumeration) {
            added.push(SchemaElement {
                schema_file: path.to_owned(),
                element: enumeration.clone(),
            });
        }
    }
    for (service, old_methods) in &old_file.services {
        let Some(new_methods) = new_file.services.get(service) else {
            breaks.push(PendingBreak {
                schema_file: path.to_owned(),
                element: service.clone(),
                change: SchemaChange::Removed,
                note: None,
                target: BreakTarget::Service(service.clone()),
            });
            continue;
        };
        for (method, old_method) in old_methods {
            let Some(new_method) = new_methods.get(method) else {
                breaks.push(PendingBreak {
                    schema_file: path.to_owned(),
                    element: format!("{service}.{method}"),
                    change: SchemaChange::Removed,
                    note: None,
                    target: BreakTarget::Method {
                        service: service.clone(),
                        method: method.clone(),
                    },
                });
                continue;
            };
            if old_method.input != new_method.input {
                breaks.push(PendingBreak {
                    schema_file: path.to_owned(),
                    element: format!("{service}.{method}"),
                    change: SchemaChange::InputChanged {
                        from: old_method.input.clone(),
                        to: new_method.input.clone(),
                    },
                    note: None,
                    target: BreakTarget::Method {
                        service: service.clone(),
                        method: method.clone(),
                    },
                });
            }
            if old_method.output != new_method.output {
                breaks.push(PendingBreak {
                    schema_file: path.to_owned(),
                    element: format!("{service}.{method}"),
                    change: SchemaChange::OutputChanged {
                        from: old_method.output.clone(),
                        to: new_method.output.clone(),
                    },
                    note: None,
                    target: BreakTarget::Method {
                        service: service.clone(),
                        method: method.clone(),
                    },
                });
            }
        }
        for method in new_methods.keys() {
            if !old_methods.contains_key(method) {
                added.push(SchemaElement {
                    schema_file: path.to_owned(),
                    element: format!("{service}.{method}"),
                });
            }
        }
    }
    for service in new_file.services.keys() {
        if !old_file.services.contains_key(service) {
            added.push(SchemaElement {
                schema_file: path.to_owned(),
                element: service.clone(),
            });
        }
    }
}

/// Diff one message present on both sides, field by field. The order
/// of checks is the identity rule: same number + name first, then
/// same name (number change), then same number (rename or reuse),
/// then absence (removal).
fn diff_message(
    path: &str,
    message: &str,
    old_message: &SchemaMessage,
    new_message: &SchemaMessage,
    breaks: &mut Vec<PendingBreak>,
    added: &mut Vec<SchemaElement>,
) {
    for (number, old_field) in &old_message.by_number {
        if let Some(new_field) = new_message.by_number.get(number) {
            if new_field.name == old_field.name {
                diff_same_field(
                    path, message, old_field, new_field, breaks,
                );
                continue;
            }
            // Same number, different name: a rename only when the
            // type matches and the old name is gone entirely;
            // otherwise the number was reused, which is removal +
            // addition, never a rename.
            if new_field.type_name == old_field.type_name
                && !new_message.by_name.contains_key(&old_field.name)
            {
                breaks.push(PendingBreak {
                    schema_file: path.to_owned(),
                    element: format!("{message}.{}", old_field.name),
                    change: SchemaChange::Renamed {
                        from: old_field.name.clone(),
                        to: new_field.name.clone(),
                    },
                    note: reserved_note(new_message, *number),
                    target: BreakTarget::Field {
                        message: message.to_owned(),
                        field: old_field.name.clone(),
                        json_name: old_field.json_name.clone(),
                    },
                });
            } else {
                breaks.push(PendingBreak {
                    schema_file: path.to_owned(),
                    element: format!("{message}.{}", old_field.name),
                    change: SchemaChange::Removed,
                    note: reserved_note(new_message, *number),
                    target: BreakTarget::Field {
                        message: message.to_owned(),
                        field: old_field.name.clone(),
                        json_name: old_field.json_name.clone(),
                    },
                });
                // The new field at this number is an addition, not a
                // rename: it is only added when its name is new.
                if !old_message.by_name.contains_key(&new_field.name) {
                    added.push(SchemaElement {
                        schema_file: path.to_owned(),
                        element: format!("{message}.{}", new_field.name),
                    });
                }
            }
            continue;
        }
        if let Some(new_number) = new_message.by_name.get(&old_field.name) {
            breaks.push(PendingBreak {
                schema_file: path.to_owned(),
                element: format!("{message}.{}", old_field.name),
                change: SchemaChange::NumberChanged {
                    from: i64::from(*number),
                    to: i64::from(*new_number),
                },
                note: reserved_note(new_message, *number),
                target: BreakTarget::Field {
                    message: message.to_owned(),
                    field: old_field.name.clone(),
                    json_name: old_field.json_name.clone(),
                },
            });
            continue;
        }
        breaks.push(PendingBreak {
            schema_file: path.to_owned(),
            element: format!("{message}.{}", old_field.name),
            change: SchemaChange::Removed,
            note: reserved_note(new_message, *number),
            target: BreakTarget::Field {
                message: message.to_owned(),
                field: old_field.name.clone(),
                json_name: old_field.json_name.clone(),
            },
        });
    }
    for (number, new_field) in &new_message.by_number {
        if old_message.by_number.contains_key(number) {
            continue;
        }
        if old_message.by_name.contains_key(&new_field.name) {
            continue; // Covered by the NumberChanged branch above.
        }
        added.push(SchemaElement {
            schema_file: path.to_owned(),
            element: format!("{message}.{}", new_field.name),
        });
    }
}

/// Compare one field present under the same number and name.
fn diff_same_field(
    path: &str,
    message: &str,
    old_field: &SchemaField,
    new_field: &SchemaField,
    breaks: &mut Vec<PendingBreak>,
) {
    let element = format!("{message}.{}", old_field.name);
    let target = || BreakTarget::Field {
        message: message.to_owned(),
        field: old_field.name.clone(),
        json_name: old_field.json_name.clone(),
    };
    if old_field.type_name != new_field.type_name {
        breaks.push(PendingBreak {
            schema_file: path.to_owned(),
            element: element.clone(),
            change: SchemaChange::TypeChanged {
                from: old_field.type_name.clone(),
                to: new_field.type_name.clone(),
            },
            note: None,
            target: target(),
        });
    }
    if old_field.label != new_field.label {
        breaks.push(PendingBreak {
            schema_file: path.to_owned(),
            element: element.clone(),
            change: SchemaChange::LabelChanged {
                from: label_or_default(&old_field.label),
                to: label_or_default(&new_field.label),
            },
            note: None,
            target: target(),
        });
    }
    if old_field.oneof != new_field.oneof {
        breaks.push(PendingBreak {
            schema_file: path.to_owned(),
            element,
            change: SchemaChange::OneofChanged {
                from: old_field.oneof.clone(),
                to: new_field.oneof.clone(),
            },
            note: None,
            target: target(),
        });
    }
}

/// A proto3 field with no label is singular: the label reads as such.
fn label_or_default(label: &str) -> String {
    if label.is_empty() {
        "singular".to_owned()
    } else {
        label.to_owned()
    }
}

/// A deletion whose number is now reserved is still a break — the
/// reservation blocks wire reuse, it does not unbreak source
/// consumers — but the reviewer deserves the note.
fn reserved_note(message: &SchemaMessage, number: u32) -> Option<String> {
    if message.reserved_numbers.contains(&number) {
        Some(format!("field number {number} is reserved in the new side"))
    } else {
        None
    }
}

/// Diff one enum present on both sides, value by value.
fn diff_enum(
    path: &str,
    enumeration: &str,
    old_enum: &SchemaEnum,
    new_enum: &SchemaEnum,
    breaks: &mut Vec<PendingBreak>,
    added: &mut Vec<SchemaElement>,
) {
    for (number, old_value) in &old_enum.by_number {
        if let Some(new_value) = new_enum.by_number.get(number) {
            if new_value == old_value {
                continue;
            }
            // Same number, different name: a rename only when the
            // old name is gone entirely.
            if new_enum.by_name.contains_key(old_value) {
                breaks.push(PendingBreak {
                    schema_file: path.to_owned(),
                    element: format!("{enumeration}.{old_value}"),
                    change: SchemaChange::Removed,
                    note: None,
                    target: BreakTarget::EnumValue {
                        enumeration: enumeration.to_owned(),
                        value: old_value.clone(),
                    },
                });
            } else {
                breaks.push(PendingBreak {
                    schema_file: path.to_owned(),
                    element: format!("{enumeration}.{old_value}"),
                    change: SchemaChange::Renamed {
                        from: old_value.clone(),
                        to: new_value.clone(),
                    },
                    note: None,
                    target: BreakTarget::EnumValue {
                        enumeration: enumeration.to_owned(),
                        value: old_value.clone(),
                    },
                });
            }
            continue;
        }
        if let Some(new_number) = new_enum.by_name.get(old_value) {
            breaks.push(PendingBreak {
                schema_file: path.to_owned(),
                element: format!("{enumeration}.{old_value}"),
                change: SchemaChange::NumberChanged {
                    from: i64::from(*number),
                    to: i64::from(*new_number),
                },
                note: None,
                target: BreakTarget::EnumValue {
                    enumeration: enumeration.to_owned(),
                    value: old_value.clone(),
                },
            });
            continue;
        }
        breaks.push(PendingBreak {
            schema_file: path.to_owned(),
            element: format!("{enumeration}.{old_value}"),
            change: SchemaChange::Removed,
            note: None,
            target: BreakTarget::EnumValue {
                enumeration: enumeration.to_owned(),
                value: old_value.clone(),
            },
        });
    }
    for (number, new_value) in &new_enum.by_number {
        if old_enum.by_number.contains_key(number) {
            continue;
        }
        if old_enum.by_name.contains_key(new_value) {
            continue;
        }
        added.push(SchemaElement {
            schema_file: path.to_owned(),
            element: format!("{enumeration}.{new_value}"),
        });
    }
}

// ---------------------------------------------------------------------------
// Generated-code discovery and the generator mapping.
// ---------------------------------------------------------------------------

/// The language a generated file was produced for. v1 covers Go and
/// Python only; a TypeScript mapping is v1.1 work with its own golden
/// tests, not a small follow-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GeneratedLang {
    Go,
    Python,
}

/// One generated file found in the new tree.
#[derive(Debug, Clone)]
struct GeneratedFile {
    /// Repo-relative path.
    path: String,
    /// The language's generator family.
    lang: GeneratedLang,
    /// True when the file carries a generated suffix but no header
    /// marker: the pinned mapping applies, with a disclosed note.
    assumed: bool,
}

/// Whether a file's text carries a generator header marker.
fn generated_marker(text: &str, lang: GeneratedLang) -> bool {
    match lang {
        GeneratedLang::Go => {
            text.contains("Code generated") && text.contains("DO NOT EDIT")
        }
        GeneratedLang::Python => {
            text.contains("Generated by the protocol buffer compiler")
                && text.contains("DO NOT EDIT")
        }
    }
}

/// Find generated files among collected consumer texts: marker or
/// generated suffix. A hand-written file under `gen/` is not
/// generated; a generated file elsewhere still is — the rule is about
/// markers and the generator's own suffixes, never about directories.
fn find_generated(texts: &[(String, String)]) -> Vec<GeneratedFile> {
    let mut generated = Vec::new();
    for (path, text) in texts {
        let lang = if path.ends_with(".go") {
            GeneratedLang::Go
        } else if path.ends_with(".py") || path.ends_with(".pyi") {
            GeneratedLang::Python
        } else {
            continue;
        };
        let suffix_generated = path.ends_with(".pb.go")
            || path.ends_with("_pb2.py")
            || path.ends_with("_pb2.pyi")
            || path.ends_with("_pb2_grpc.py");
        let marked = generated_marker(text, lang);
        if marked || suffix_generated {
            generated.push(GeneratedFile {
                path: path.clone(),
                lang,
                assumed: suffix_generated && !marked,
            });
        }
    }
    generated
}

/// One field's generated spellings, per language.
#[derive(Debug, Clone)]
struct GenField {
    /// The proto name, as written.
    proto: String,
    /// The Go struct field name (`CamelCase`).
    go: String,
    /// The Python attribute name (as written).
    py: String,
    /// The `json_name` (explicit or derived).
    json: String,
}

/// One message's generated shape.
#[derive(Debug, Clone, Default)]
struct GenMessage {
    /// Fields by proto name.
    fields: BTreeMap<String, GenField>,
    /// The Go type name, computed at build time when the package
    /// is known: the scan sites compare Go source text against
    /// it and have no package to derive it from.
    go_name: String,
}

/// One enum's generated shape.
#[derive(Debug, Clone, Default)]
struct GenEnum {
    /// Values by proto name.
    values: BTreeSet<String>,
    /// The Go type name, as for `GenMessage::go_name`.
    go_name: String,
}

/// One service's generated shape.
#[derive(Debug, Clone, Default)]
struct GenService {
    /// Its rpc method names.
    methods: BTreeSet<String>,
}

/// One generated module: a generated file linked to the schema
/// elements it implements.
#[derive(Debug, Clone, Default)]
struct GenModule {
    /// The generated file, repo-relative.
    file: String,
    /// The language's generator family.
    lang: Option<GeneratedLang>,
    /// Messages by qualified schema name.
    messages: BTreeMap<String, GenMessage>,
    /// Enums by qualified schema name.
    enums: BTreeMap<String, GenEnum>,
    /// Services by qualified schema name.
    services: BTreeMap<String, GenService>,
    /// The Go package clause, for default import aliases.
    go_package: Option<String>,
    /// The Python dotted module path (`foo.bar_pb2`).
    py_module: Option<String>,
}

/// Build generated modules from the schema index and the generated
/// files found in the new tree.
///
/// Python links by path: proto `foo/bar.proto` generates
/// `foo/bar_pb2.py` (and the `_pb2.pyi` / `_pb2_grpc.py` siblings),
/// which is the generator's own rule, not a heuristic. Go links by
/// declared struct/interface name: a `.pb.go` file implements the
/// schema message whose Go struct name it declares. Ambiguity (one
/// struct name matching messages in two packages with no directory
/// signal to choose between) links nothing and notes it, rather than
/// guessing.
/// Build the consumer-join catalog: which generated file implements
/// which schema element, and each element's generated spellings.
///
/// The catalog comes from the OLD schema and the generated files
/// from the NEW tree's texts, deliberately: consumers are old code,
/// so they name old elements — a field the new side removed is
/// still named by every consumer that has not migrated yet, and a
/// catalog built from the new schema would have no entry for it
/// and would report the removal with no consumers at all. A
/// generated file added in the new side pairs to no old proto and
/// is skipped silently: it cannot be a break target, so indexing
/// it would only add noise.
fn build_modules(
    old_schema: &SchemaIndex,
    new_schema: &SchemaIndex,
    generated: &[GeneratedFile],
    texts: &BTreeMap<String, String>,
    coverage: &mut Vec<String>,
) -> Vec<GenModule> {
    // All schema messages/enums/services, for Go name matching.
    // Each entry carries its file's package: the Go type name
    // strips the whole package, and only the file knows where
    // the package ends and the message path begins.
    let mut messages: BTreeMap<String, (&SchemaMessage, &str)> =
        BTreeMap::new();
    let mut enums: BTreeMap<String, (&SchemaEnum, &str)> = BTreeMap::new();
    let mut services: BTreeMap<String, BTreeMap<String, SchemaMethod>> =
        BTreeMap::new();
    for file in old_schema.files.values() {
        for (name, message) in &file.messages {
            messages.insert(name.clone(), (message, file.package.as_str()));
        }
        for (name, enumeration) in &file.enums {
            enums.insert(name.clone(), (enumeration, file.package.as_str()));
        }
        for (name, methods) in &file.services {
            services.insert(name.clone(), methods.clone());
        }
    }

    let mut modules = Vec::new();
    for generated_file in generated {
        let Some(text) = texts.get(&generated_file.path) else {
            continue;
        };
        match generated_file.lang {
            GeneratedLang::Python => {
                let proto_path = generated_file
                    .path
                    .strip_suffix("_pb2.py")
                    .or_else(|| {
                        generated_file.path.strip_suffix("_pb2.pyi")
                    })
                    .or_else(|| {
                        generated_file.path.strip_suffix("_pb2_grpc.py")
                    })
                    .map(|stem| format!("{stem}.proto"));
                let Some(proto_path) = proto_path else {
                    continue;
                };
                let schema_file = old_schema
                    .files
                    .get(&proto_path)
                    .or_else(|| suffix_proto(old_schema, &proto_path));
                let Some(schema_file) = schema_file
                else {
                    // A generated file for a proto added on the new
                    // side has no old elements to join: skip it
                    // silently rather than crying about a file that
                    // cannot be a break target.
                    if new_schema.files.contains_key(&proto_path) {
                        continue;
                    }
                    coverage.push(format!(
                        "generated file {} has no matching .proto in the tree; its elements are not indexed",
                        generated_file.path
                    ));
                    continue;
                };
                let mut module = GenModule {
                    file: generated_file.path.clone(),
                    lang: Some(GeneratedLang::Python),
                    ..Default::default()
                };
                module.py_module = Some(
                    generated_file
                        .path
                        .trim_end_matches(".py")
                        .trim_end_matches(".pyi")
                        .replace('/', "."),
                );
                for (name, message) in &schema_file.messages {
                    let mut gen_message = GenMessage::default();
                    gen_message.go_name =
                        go_type_name(&schema_file.package, name);
                    for field in message.by_number.values() {
                        gen_message.fields.insert(field.name.clone(), GenField {
                            proto: field.name.clone(),
                            go: go_field_name(&field.name),
                            py: field.name.clone(),
                            json: field.json_name.clone(),
                        });
                    }
                    module.messages.insert(name.clone(), gen_message);
                }
                for (name, enumeration) in &schema_file.enums {
                    let mut gen_enum = GenEnum::default();
                    gen_enum.go_name =
                        go_type_name(&schema_file.package, name);
                    for value in enumeration.by_number.values() {
                        gen_enum.values.insert(value.clone());
                    }
                    module.enums.insert(name.clone(), gen_enum);
                }
                for (name, methods) in &schema_file.services {
                    let mut gen_service = GenService::default();
                    for method in methods.keys() {
                        gen_service.methods.insert(method.clone());
                    }
                    module.services.insert(name.clone(), gen_service);
                }
                modules.push(module);
            }
            GeneratedLang::Go => {
                let declared = go_declarations(text);
                let package = go_package_clause(text);
                let mut module = GenModule {
                    file: generated_file.path.clone(),
                    lang: Some(GeneratedLang::Go),
                    go_package: package,
                    ..Default::default()
                };
                let generated_dir =
                    generated_file.path.rsplit_once('/').map_or(
                        "",
                        |(dir, _)| dir,
                    );
                for (name, (message, package)) in &messages {
                    let go_name = go_type_name(package, name);
                    if !declared.structs.contains(&go_name) {
                        continue;
                    }
                    // A struct name shared by two packages links only
                    // with a directory signal: the generated file's
                    // directory sharing a suffix with the proto file's
                    // directory. Without one, ambiguity links nothing
                    // — silently. An earlier version pushed a note
                    // per declined (file, message) pair, but that
                    // note fires for every cross-package decline
                    // even when another file maps the message, so
                    // on a real repo it buried the report under
                    // dozens of lines about mappings that were
                    // never lost. The loss that matters is per
                    // break, and the per-element unjoined note in
                    // measure_schema_impact already reports exactly
                    // that.
                    if !go_name_unique(&messages, &go_name)
                        && !dir_matches_proto(
                            old_schema, name, generated_dir,
                        )
                    {
                        continue;
                    }
                    let mut gen_message = GenMessage::default();
                    gen_message.go_name = go_name.clone();
                    for field in message.by_number.values() {
                        gen_message.fields.insert(field.name.clone(), GenField {
                            proto: field.name.clone(),
                            go: go_field_name(&field.name),
                            py: field.name.clone(),
                            json: field.json_name.clone(),
                        });
                    }
                    module.messages.insert(name.clone(), gen_message);
                }
                for (name, (enumeration, package)) in &enums {
                    if !declared.structs.contains(&go_type_name(package, name))
                    {
                        continue;
                    }
                    let mut gen_enum = GenEnum::default();
                    gen_enum.go_name = go_type_name(package, name);
                    for value in enumeration.by_number.values() {
                        gen_enum.values.insert(value.clone());
                    }
                    module.enums.insert(name.clone(), gen_enum);
                }
                for (name, methods) in &services {
                    let short =
                        name.rsplit_once('.').map_or(name.as_str(), |(_, l)| l);
                    let client = format!("{short}Client");
                    let server = format!("{short}Server");
                    if !declared.interfaces.contains(&client)
                        && !declared.interfaces.contains(&server)
                    {
                        continue;
                    }
                    let mut gen_service = GenService::default();
                    for method in methods.keys() {
                        gen_service.methods.insert(method.clone());
                    }
                    module.services.insert(name.clone(), gen_service);
                }
                if !module.messages.is_empty()
                    || !module.enums.is_empty()
                    || !module.services.is_empty()
                {
                    modules.push(module);
                }
            }
        }
    }
    modules
}

/// The Go type name for a qualified schema name: strip the whole
/// package prefix, join nesting with underscores. protoc-gen-go
/// names a type by its message path, not its package path: for
/// package metalstack.admin.v2 the struct is
/// `MachineServiceListRequest`, not the
/// `admin_v2_MachineServiceListRequest` the old derivation
/// produced by dropping only the first package segment. The old
/// derivation passed only because every corpus fixture used a
/// one-segment package, where the two agree.
fn go_type_name(package: &str, qualified: &str) -> String {
    let path = qualified
        .strip_prefix(package)
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(qualified);
    path.replace('.', "_")
}

/// Whether exactly one schema message maps to a Go type name.
fn go_name_unique(
    messages: &BTreeMap<String, (&SchemaMessage, &str)>,
    go_name: &str,
) -> bool {
    messages
        .iter()
        .filter(|(name, (_, package))| go_type_name(package, name) == go_name)
        .count()
        == 1
}

/// The proto a generated file maps to when the exact path misses:
/// protoc's out-dir layout puts generated files under their own
/// root (python/, go/) while protos live under proto/, so the
/// path relative to each root is the same. Compare
/// path-minus-first-segment; an ambiguous suffix maps nothing,
/// and a root-level file never suffix-matches (the exact rule
/// already covers it).
fn suffix_proto<'a>(
    schema: &'a SchemaIndex,
    proto_path: &str,
) -> Option<&'a SchemaFile> {
    let stem = proto_path.strip_suffix(".proto")?;
    let (_, suffix) = stem.split_once('/')?;
    if suffix.is_empty() {
        return None;
    }
    let mut found = None;
    for (path, file) in &schema.files {
        let Some(path_stem) = path.strip_suffix(".proto") else {
            continue;
        };
        let Some((_, path_suffix)) = path_stem.split_once('/') else {
            continue;
        };
        if path_suffix == suffix {
            if found.is_some() {
                return None;
            }
            found = Some(file);
        }
    }
    found
}

/// Whether a generated file's directory disambiguates a struct
/// name shared by two packages. Among the proto files declaring
/// the message, exactly one directory must win: either it is the
/// same directory as the generated file, or it shares the longest
/// trailing run of path segments with it and that run is at
/// least two segments. A one-segment tail like `v2` is shared by
/// every versioned package and proves nothing. The case this
/// exists for is the sibling-root layout — protos under
/// `proto/metalstack/admin/v2`, generated code under
/// `go/metalstack/admin/v2` — where the old equality-or-nesting
/// rule saw no signal at all and left every shared struct
/// unmapped.
fn dir_matches_proto(
    schema: &SchemaIndex,
    message: &str,
    generated_dir: &str,
) -> bool {
    if generated_dir.is_empty() {
        return false;
    }
    let gen_segments: Vec<&str> = generated_dir
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    let mut same_dir = 0;
    let mut best = 0;
    let mut best_count = 0;
    for (path, file) in &schema.files {
        if !file.messages.contains_key(message) {
            continue;
        }
        let proto_dir = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        if proto_dir.is_empty() {
            continue;
        }
        if proto_dir == generated_dir {
            same_dir += 1;
            continue;
        }
        let proto_segments: Vec<&str> = proto_dir
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        let shared = proto_segments
            .iter()
            .rev()
            .zip(gen_segments.iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        if shared > best {
            best = shared;
            best_count = 1;
        } else if shared == best && shared > 0 {
            best_count += 1;
        }
    }
    same_dir == 1 || (best >= 2 && best_count == 1)
}

/// Struct and interface names a Go file declares, via tree-sitter-go.
struct GoDeclarations {
    structs: BTreeSet<String>,
    interfaces: BTreeSet<String>,
}

/// Parse Go declarations out of a file's text. A file that fails to
/// parse yields no declarations rather than an error: declaration
/// matching is best-effort, and the strict side is the schema parser,
/// not the generated-code reader.
fn go_declarations(text: &str) -> GoDeclarations {
    let mut out = GoDeclarations {
        structs: BTreeSet::new(),
        interfaces: BTreeSet::new(),
    };
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .is_err()
    {
        return out;
    }
    let Some(tree) = parser.parse(text, None) else {
        return out;
    };
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "type_declaration" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "type_spec" {
                    let mut name = None;
                    let mut is_interface = false;
                    let mut inner = child.walk();
                    for part in child.children(&mut inner) {
                        if part.kind() == "type_identifier" && name.is_none()
                        {
                            name = Some(node_slice(text, &part));
                        }
                        if part.kind() == "interface_type" {
                            is_interface = true;
                        }
                    }
                    if let Some(name) = name {
                        if is_interface {
                            out.interfaces.insert(name);
                        } else {
                            out.structs.insert(name);
                        }
                    }
                }
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    out
}

/// The `package X` clause of a Go file's text, if one is present.
fn go_package_clause(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("package ") {
            let name = rest.trim();
            if !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Some(name.to_owned());
            }
        }
    }
    None
}

/// The text of a tree-sitter node, lossy on invalid UTF-8 boundaries.
fn node_slice<'a>(text: &'a str, node: &tree_sitter::Node<'_>) -> String {
    let start = node.start_byte().min(text.len());
    let end = node.end_byte().min(text.len());
    if start >= end {
        return String::new();
    }
    text.get(start..end).unwrap_or("").trim().to_owned()
}

// ---------------------------------------------------------------------------
// The consumer scan: import-gated AST evidence over the new tree.
// ---------------------------------------------------------------------------

/// Everything one file evidences, keyed by element so the measure fn
/// can join it to breaks without re-walking.
#[derive(Debug, Default)]
struct FileEvidence {
    /// Messages named: (module file, qualified message).
    message_refs: BTreeSet<(String, String)>,
    /// Enums named: (module file, qualified enum).
    enum_refs: BTreeSet<(String, String)>,
    /// Services named: (module file, qualified service).
    service_refs: BTreeSet<(String, String)>,
    /// Struct literals: (module file, qualified message, proto field).
    struct_literals: BTreeSet<(String, String, String)>,
    /// Constructor kwargs: (module file, qualified message, proto field).
    kwargs: BTreeSet<(String, String, String)>,
    /// Enum constants: (module file, qualified enum, proto value).
    enum_consts: BTreeSet<(String, String, String)>,
    /// Method calls: (module file, qualified service, method).
    method_calls: BTreeSet<(String, String, String)>,
    /// Field accesses: (module file, qualified message, proto field).
    field_accesses: BTreeSet<(String, String, String)>,
    /// JSON keys: (module file, qualified message, proto field).
    json_keys: BTreeSet<(String, String, String)>,
}

/// One resolved import of a generated module in a consumer file.
#[derive(Debug, Clone)]
struct ResolvedImport {
    /// The generated module's file.
    module_file: String,
    /// The alias the consumer uses for it, if any.
    alias: Option<String>,
    /// Names a from-import binds, if any.
    bound_names: Vec<String>,
}

/// Split a dotted chain into segments.
fn chain_segments(chain: &str) -> Vec<String> {
    chain
        .split('.')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Match a dotted chain against a file's resolved generated imports.
/// Returns the module file and the remaining segments after the
/// import prefix, if any prefix matches. The longest prefix wins, so
/// `foo.bar_pb2.User` matches the module `foo.bar_pb2` before the
/// alias `foo` could claim it.
fn match_chain(
    segments: &[String],
    imports: &[ResolvedImport],
    modules: &BTreeMap<String, GenModule>,
) -> Option<(String, Vec<String>)> {
    for len in (1..=segments.len()).rev() {
        let prefix = segments[..len].join(".");
        for import in imports {
            if let Some(alias) = &import.alias {
                if *alias == prefix {
                    return Some((
                        import.module_file.clone(),
                        segments[len..].to_vec(),
                    ));
                }
            }
            if let Some(module) = modules.get(&import.module_file) {
                if let Some(py_module) = &module.py_module {
                    if *py_module == prefix {
                        return Some((
                            import.module_file.clone(),
                            segments[len..].to_vec(),
                        ));
                    }
                }
            }
            // A from-import binds its names directly: `User` alone
            // is the message when the file did `from x_pb2 import User`.
            if len == 1 && import.bound_names.contains(&prefix) {
                return Some((
                    import.module_file.clone(),
                    segments[len..].to_vec(),
                ));
            }
        }
    }
    None
}

/// Collect a Go file's imports: (path, alias).
fn go_imports(root: &tree_sitter::Node<'_>, text: &str) -> Vec<(String, Option<String>)> {
    let mut imports = Vec::new();
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        if node.kind() == "import_spec" {
            let mut path = None;
            let mut alias = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                match child.kind() {
                    "interpreted_string_literal" => {
                        let raw = node_slice(text, &child);
                        path = Some(raw.trim_matches('"').to_owned());
                    }
                    "package_identifier" => {
                        alias = Some(node_slice(text, &child));
                    }
                    _ => {}
                }
            }
            if let Some(path) = path {
                imports.push((path, alias));
            }
            continue;
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    imports
}

/// Resolve a Go file's imports to generated modules: a graph edge
/// resolved to the generated file's directory wins; otherwise the
/// import path's trailing segments match the generated directory.
fn resolve_go_imports(
    file: &str,
    imports: &[(String, Option<String>)],
    modules: &[GenModule],
    report: &GraphReport,
) -> Vec<ResolvedImport> {
    let mut resolved = Vec::new();
    // Graph edges from this file, by import path.
    let mut edge_targets: BTreeMap<String, String> = BTreeMap::new();
    for edge in &report.edges {
        if edge.from == file
            && edge.resolved
            && let Some(to) = &edge.to
        {
            edge_targets
                .entry(edge.import_path.clone())
                .or_insert_with(|| to.clone());
        }
    }
    for (path, alias) in imports {
        let mut module_file: Option<String> = None;
        if let Some(target) = edge_targets.get(path) {
            let target_dir =
                target.rsplit_once('/').map_or("", |(dir, _)| dir);
            for module in modules {
                if module.lang != Some(GeneratedLang::Go) {
                    continue;
                }
                let module_dir =
                    module.file.rsplit_once('/').map_or("", |(dir, _)| dir);
                if module_dir == target_dir {
                    module_file = Some(module.file.clone());
                    break;
                }
            }
        }
        if module_file.is_none() {
            let path_segments: Vec<&str> = path.split('/').collect();
            let mut candidates = Vec::new();
            for module in modules {
                if module.lang != Some(GeneratedLang::Go) {
                    continue;
                }
                let module_dir =
                    module.file.rsplit_once('/').map_or("", |(dir, _)| dir);
                let dir_segments: Vec<&str> =
                    module_dir.split('/').filter(|s| !s.is_empty()).collect();
                if dir_segments.is_empty() {
                    continue;
                }
                let suffix_match = path_segments.len() >= dir_segments.len()
                    && path_segments[path_segments.len() - dir_segments.len()..]
                        == dir_segments[..];
                let last_match = path_segments.last()
                    == dir_segments.last();
                if suffix_match || last_match {
                    candidates.push(module.file.clone());
                }
            }
            candidates.sort();
            candidates.dedup();
            if candidates.len() == 1 {
                module_file = Some(candidates.into_iter().next().unwrap_or_default());
            }
        }
        if let Some(module_file) = module_file {
            let alias = alias.clone().or_else(|| {
                modules.iter().find(|m| m.file == module_file).and_then(|m| {
                    m.go_package.clone()
                })
            });
            resolved.push(ResolvedImport {
                module_file,
                alias,
                bound_names: Vec::new(),
            });
        }
    }
    resolved
}

/// Scan a Go file's AST for schema evidence.
fn scan_go(
    root: &tree_sitter::Node<'_>,
    file: &str,
    text: &str,
    imports: &[ResolvedImport],
    modules: &BTreeMap<String, GenModule>,
    evidence: &mut FileEvidence,
) {
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "composite_literal" => {
                scan_go_literal(
                    &node, file, text, imports, modules, evidence,
                );
            }
            "call_expression" => {
                scan_go_call(&node, text, imports, modules, evidence);
            }
            "selector_expression" => {
                scan_go_selector(&node, text, imports, modules, evidence);
            }
            "interpreted_string_literal" => {
                let literal =
                    node_slice(text, &node).trim_matches('"').to_owned();
                for import in imports {
                    let Some(module) = modules.get(&import.module_file)
                    else {
                        continue;
                    };
                    for (message, gen_message) in &module.messages {
                        for field in gen_message.fields.values() {
                            if field.json == literal {
                                // The join matches JSON keys against the
                                // field's json_name, so the evidence
                                // carries the JSON spelling, not the
                                // proto name: for `user_id` the key
                                // is `userId`, and storing the proto
                                // name would never match it.
                                evidence.json_keys.insert((
                                    import.module_file.clone(),
                                    message.clone(),
                                    field.json.clone(),
                                ));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
}

/// A Go composite literal: its type names the message, its keys name
/// the fields. `&pb.User{UserId: 1}` and `pb.User{UserId: 1}` share
/// the shape; the unary `&` sits outside the literal node. A bare
/// type (`User{...}`) matches only a generated module in the same
/// directory — same package, same package clause by construction.
fn scan_go_literal(
    node: &tree_sitter::Node<'_>,
    file: &str,
    text: &str,
    imports: &[ResolvedImport],
    modules: &BTreeMap<String, GenModule>,
    evidence: &mut FileEvidence,
) {
    let Some(type_node) = node.child_by_field_name("type") else {
        return;
    };
    let type_text = node_slice(text, &type_node);
    let type_segments = chain_segments(&type_text);
    if type_segments.is_empty() {
        return;
    }
    let mut message: Option<(String, String)> = None;
    if type_segments.len() == 1 {
        let consumer_dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
        let name = &type_segments[0];
        for import in imports {
            let Some(module) = modules.get(&import.module_file) else {
                continue;
            };
            let module_dir = import
                .module_file
                .rsplit_once('/')
                .map_or("", |(dir, _)| dir);
            if module_dir != consumer_dir {
                continue;
            }
            if let Some(qualified) = module
                .messages
                .iter()
                .find(|(qualified, gm)| {
                    *qualified == name || gm.go_name == *name
                })
                .map(|(qualified, _)| qualified)
            {
                message =
                    Some((import.module_file.clone(), qualified.clone()));
            }
        }
    }
    if type_segments.len() >= 2 {
        let alias = type_segments[..type_segments.len() - 1].join(".");
        let name = type_segments.last().cloned().unwrap_or_default();
        for import in imports {
            if import.alias.as_deref() == Some(alias.as_str()) {
                if let Some(module) = modules.get(&import.module_file) {
                    if module.messages.contains_key(&name)
                        || module
                            .messages
                            .values()
                            .any(|gm| gm.go_name == name)
                    {
                        let qualified = module
                            .messages
                            .iter()
                            .find(|(qualified, gm)| {
                                *qualified == &name || gm.go_name == name
                            })
                            .map(|(qualified, _)| qualified.clone());
                        if let Some(qualified) = qualified {
                            message = Some((
                                import.module_file.clone(),
                                qualified,
                            ));
                        }
                    }
                }
            }
        }
    }
    let Some((module_file, qualified)) = message else {
        return;
    };
    let Some(module) = modules.get(&module_file) else {
        return;
    };
    let Some(gen_message) = module.messages.get(&qualified) else {
        return;
    };
    let Some(body) = node.child_by_field_name("body") else {
        return;
    };
    let mut key_stack = vec![body];
    while let Some(key_node) = key_stack.pop() {
        if key_node.kind() == "keyed_element" {
            if let Some(key) = key_node.child_by_field_name("key") {
                let key_text = node_slice(text, &key);
                for field in gen_message.fields.values() {
                    if field.go == key_text {
                        evidence.struct_literals.insert((
                            module_file.clone(),
                            qualified.clone(),
                            field.proto.clone(),
                        ));
                    }
                }
            }
            continue;
        }
        let mut cursor = key_node.walk();
        for child in key_node.children(&mut cursor) {
            key_stack.push(child);
        }
    }
}

/// A Go call: a selector naming a service method in call position is
/// a method call, when the receiver is not the package alias (a
/// constructor like `pb.NewXxxClient` is not a method call).
fn scan_go_call(
    node: &tree_sitter::Node<'_>,
    text: &str,
    imports: &[ResolvedImport],
    modules: &BTreeMap<String, GenModule>,
    evidence: &mut FileEvidence,
) {
    let Some(function) = node.child_by_field_name("function") else {
        return;
    };
    if function.kind() != "selector_expression" {
        return;
    }
    let Some(field) = function.child_by_field_name("field") else {
        return;
    };
    let method = node_slice(text, &field);
    let Some(operand) = function.child_by_field_name("operand") else {
        return;
    };
    let operand_text = node_slice(text, &operand);
    for import in imports {
        if import.alias.as_deref() == Some(operand_text.as_str()) {
            continue;
        }
        let Some(module) = modules.get(&import.module_file) else {
            continue;
        };
        for (service, gen_service) in &module.services {
            if gen_service.methods.contains(&method) {
                evidence.method_calls.insert((
                    import.module_file.clone(),
                    service.clone(),
                    method.clone(),
                ));
            }
        }
    }
}

/// A Go selector outside call position: `alias.Message` names the
/// message, `alias.Enum_VALUE` names the enum value, and any other
/// selector naming a generated field name is a field access (the
/// receiver's type is unknown — Tier A3, flagged as such).
fn scan_go_selector(
    node: &tree_sitter::Node<'_>,
    text: &str,
    imports: &[ResolvedImport],
    modules: &BTreeMap<String, GenModule>,
    evidence: &mut FileEvidence,
) {
    let Some(field) = node.child_by_field_name("field") else {
        return;
    };
    let field_text = node_slice(text, &field);
    let Some(operand) = node.child_by_field_name("operand") else {
        return;
    };
    let operand_text = node_slice(text, &operand);
    for import in imports {
        let Some(module) = modules.get(&import.module_file) else {
            continue;
        };
        if import.alias.as_deref() == Some(operand_text.as_str()) {
            // A package-qualified symbol: a message, a service
            // interface, or an enum constant.
            for (message, gm) in &module.messages {
                if *message == field_text || gm.go_name == field_text {
                    evidence.message_refs.insert((
                        import.module_file.clone(),
                        message.clone(),
                    ));
                }
            }
            for (enumeration, ge) in &module.enums {
                // The enum constant pattern uses the Go type name,
                // not the last package segment: `Color_RED`, and
                // `Outer_Inner_RED` for a nested enum.
                let short = ge.go_name.as_str();
                if *enumeration == field_text || short == field_text {
                    evidence.enum_refs.insert((
                        import.module_file.clone(),
                        enumeration.clone(),
                    ));
                }
                for value in module.enums[enumeration].values.iter() {
                    if field_text == format!("{short}_{value}") {
                        evidence.enum_consts.insert((
                            import.module_file.clone(),
                            enumeration.clone(),
                            value.clone(),
                        ));
                    }
                }
            }
            for service in module.services.keys() {
                let short = service
                    .rsplit_once('.')
                    .map_or(service.as_str(), |(_, last)| last);
                if field_text == format!("{short}Client")
                    || field_text == format!("{short}Server")
                {
                    evidence.service_refs.insert((
                        import.module_file.clone(),
                        service.clone(),
                    ));
                }
            }
            continue;
        }
        // Any other receiver: a field access, if the field name
        // matches a generated field of this module.
        for (message, gen_message) in &module.messages {
            for field in gen_message.fields.values() {
                if field.go == field_text {
                    evidence.field_accesses.insert((
                        import.module_file.clone(),
                        message.clone(),
                        field.proto.clone(),
                    ));
                }
            }
        }
    }
}

/// Collect a Python file's imports: plain imports (dotted path, alias)
/// and from-imports (module, bound names with aliases).
fn python_imports(
    root: &tree_sitter::Node<'_>,
    text: &str,
) -> (Vec<(String, Option<String>)>, Vec<(String, Vec<String>)>) {
    let mut plain = Vec::new();
    let mut from_imports = Vec::new();
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "import_statement" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "dotted_name" {
                        plain.push((
                            node_slice(text, &child),
                            None,
                        ));
                    } else if child.kind() == "aliased_import" {
                        let mut name = None;
                        let mut alias = None;
                        let mut inner = child.walk();
                        for part in child.children(&mut inner) {
                            if part.kind() == "dotted_name"
                                && name.is_none()
                            {
                                name = Some(node_slice(text, &part));
                            } else if part.kind() == "identifier" {
                                alias = Some(node_slice(text, &part));
                            }
                        }
                        if let Some(name) = name {
                            plain.push((name, alias));
                        }
                    }
                }
                continue;
            }
            "import_from_statement" => {
                let mut module = None;
                let mut names = Vec::new();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    match child.kind() {
                        "dotted_name" if module.is_none() => {
                            module = Some(node_slice(text, &child));
                        }
                        "dotted_name" => {
                            names.push(node_slice(text, &child));
                        }
                        "aliased_import" => {
                            let mut inner = child.walk();
                            for part in child.children(&mut inner) {
                                if part.kind() == "identifier" {
                                    names.push(node_slice(text, &part));
                                    break;
                                }
                            }
                        }
                        "identifier" => {
                            names.push(node_slice(text, &child));
                        }
                        "wildcard_import" => {
                            // A wildcard import binds no single name:
                            // it is never evidence for one message.
                        }
                        _ => {}
                    }
                }
                if let Some(module) = module {
                    from_imports.push((module, names));
                }
                continue;
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    (plain, from_imports)
}

/// Resolve a Python file's imports to generated modules: a graph edge
/// resolved to the generated file wins; otherwise the module's dotted
/// path matches the generated file's dotted path exactly or as a
/// trailing suffix, uniquely.
fn resolve_python_imports(
    plain: &[(String, Option<String>)],
    from_imports: &[(String, Vec<String>)],
    modules: &[GenModule],
    report: &GraphReport,
    file: &str,
) -> Vec<ResolvedImport> {
    let mut resolved = Vec::new();
    let mut edge_targets: BTreeMap<String, String> = BTreeMap::new();
    for edge in &report.edges {
        if edge.from == file
            && edge.resolved
            && let Some(to) = &edge.to
        {
            edge_targets
                .entry(edge.import_path.clone())
                .or_insert_with(|| to.clone());
        }
    }
    let match_module = |dotted: &str, edge_targets: &BTreeMap<String, String>| -> Option<String> {
        if let Some(target) = edge_targets.get(dotted) {
            for module in modules {
                if module.lang == Some(GeneratedLang::Python)
                    && &module.file == target
                {
                    return Some(module.file.clone());
                }
            }
        }
        let mut candidates = Vec::new();
        for module in modules {
            if module.lang != Some(GeneratedLang::Python) {
                continue;
            }
            if let Some(py_module) = &module.py_module {
                if py_module == dotted
                    || py_module.ends_with(&format!(".{dotted}"))
                {
                    candidates.push(module.file.clone());
                }
            }
        }
        candidates.sort();
        candidates.dedup();
        if candidates.len() == 1 {
            candidates.into_iter().next()
        } else {
            None
        }
    };
    for (dotted, alias) in plain {
        if let Some(module_file) = match_module(dotted, &edge_targets) {
            resolved.push(ResolvedImport {
                module_file,
                alias: alias.clone(),
                bound_names: Vec::new(),
            });
        }
    }
    for (dotted, names) in from_imports {
        if let Some(module_file) = match_module(dotted, &edge_targets) {
            resolved.push(ResolvedImport {
                module_file,
                alias: None,
                bound_names: names.clone(),
            });
        }
    }
    resolved
}

/// Scan a Python file's AST for schema evidence.
fn scan_python(
    root: &tree_sitter::Node<'_>,
    text: &str,
    imports: &[ResolvedImport],
    modules: &BTreeMap<String, GenModule>,
    evidence: &mut FileEvidence,
) {
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "import_statement" | "import_from_statement" => {
                // Imports are collected separately; their names are
                // not evidence.
                continue;
            }
            "call" => {
                scan_python_call(&node, text, imports, modules, evidence);
            }
            "attribute" => {
                scan_python_attribute(
                    &node, text, imports, modules, evidence,
                );
            }
            "identifier" => {
                let name = node_slice(text, &node);
                for import in imports {
                    if !import.bound_names.contains(&name) {
                        continue;
                    }
                    let Some(module) = modules.get(&import.module_file)
                    else {
                        continue;
                    };
                    for message in module.messages.keys() {
                        let short = message
                            .rsplit_once('.')
                            .map_or(message.as_str(), |(_, last)| last);
                        if short == name || *message == *name {
                            evidence.message_refs.insert((
                                import.module_file.clone(),
                                message.clone(),
                            ));
                        }
                    }
                    for enumeration in module.enums.keys() {
                        let short = enumeration
                            .rsplit_once('.')
                            .map_or(enumeration.as_str(), |(_, last)| last);
                        if short == name || *enumeration == *name {
                            evidence.enum_refs.insert((
                                import.module_file.clone(),
                                enumeration.clone(),
                            ));
                        }
                    }
                    for service in module.services.keys() {
                        let short = service
                            .rsplit_once('.')
                            .map_or(service.as_str(), |(_, last)| last);
                        if name == format!("{short}Stub") {
                            evidence.service_refs.insert((
                                import.module_file.clone(),
                                service.clone(),
                            ));
                        }
                    }
                }
            }
            "string" => {
                // A string literal equal to a field's json_name, in a
                // file importing the generated module, is a JSON-key
                // name match (Tier A3, flagged).
                let mut content = None;
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "string_content" {
                        content = Some(node_slice(text, &child));
                    }
                }
                if let Some(content) = content {
                    for import in imports {
                        let Some(module) = modules.get(&import.module_file)
                        else {
                            continue;
                        };
                        for (message, gen_message) in &module.messages {
                            for field in gen_message.fields.values() {
                                if field.json == content {
                                    // Same spelling rule as the Go
                                    // branch above: the join matches
                                    // on json_name.
                                    evidence.json_keys.insert((
                                        import.module_file.clone(),
                                        message.clone(),
                                        field.json.clone(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
}

/// The dotted chain of a Python attribute or call function, e.g.
/// `foo.bar_pb2.User.Status` → ["foo", "bar_pb2", "User", "Status"].
fn python_chain(node: &tree_sitter::Node<'_>, text: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = *node;
    loop {
        match current.kind() {
            "attribute" => {
                if let Some(attribute) =
                    current.child_by_field_name("attribute")
                {
                    segments.push(node_slice(text, &attribute));
                }
                if let Some(object) = current.child_by_field_name("object") {
                    current = object;
                } else {
                    break;
                }
            }
            "identifier" => {
                segments.push(node_slice(text, &current));
                break;
            }
            _ => break,
        }
    }
    segments.reverse();
    segments
}

/// A Python call: a constructor call to a generated message with a
/// keyword naming the field is a constructor-kwarg (A2); a call to an
/// attribute naming a service method, in a file importing the
/// service's grpc module, is a method call (proven for method breaks).
fn scan_python_call(
    node: &tree_sitter::Node<'_>,
    text: &str,
    imports: &[ResolvedImport],
    modules: &BTreeMap<String, GenModule>,
    evidence: &mut FileEvidence,
) {
    let Some(function) = node.child_by_field_name("function") else {
        return;
    };
    let chain = python_chain(&function, text);
    if chain.is_empty() {
        return;
    }
    // Constructor kwargs: the function must resolve to exactly a
    // generated message — either as the last segment after a module
    // prefix (`api.user_pb2.User(...)`) or as a bound name on its
    // own (`User(...)` after a from-import, where the chain is the
    // binding itself and the remainder is empty). Without the
    // second shape, the most common Python constructor form named
    // nothing.
    if let Some((module_file, rest)) =
        match_chain(&chain, imports, modules)
    {
        let message_name: Option<&String> = if rest.len() == 1 {
            Some(&rest[0])
        } else if rest.is_empty() && chain.len() == 1 {
            Some(&chain[0])
        } else {
            None
        };
        if let Some(message_name) = message_name {
            let qualified = modules.get(&module_file).and_then(|module| {
                module.messages.keys().find(|qualified| {
                    let short = qualified
                        .rsplit_once('.')
                        .map_or(qualified.as_str(), |(_, last)| last);
                    short == message_name || *qualified == message_name
                })
            });
            if let Some(qualified) = qualified.cloned()
                && let Some(arguments) = node.child_by_field_name("arguments")
            {
                let mut cursor = arguments.walk();
                for child in arguments.children(&mut cursor) {
                    if child.kind() != "keyword_argument" {
                        continue;
                    }
                    if let Some(name) = child.child_by_field_name("name") {
                        let kwarg = node_slice(text, &name);
                        if let Some(gen_message) =
                            modules[&module_file].messages.get(&qualified)
                        {
                            for field in gen_message.fields.values() {
                                if field.py == kwarg {
                                    evidence.kwargs.insert((
                                        module_file.clone(),
                                        qualified.clone(),
                                        field.proto.clone(),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Method calls: the function's last segment names a service
    // method, in a file importing the service's module.
    if let Some(method) = chain.last() {
        for import in imports {
            let Some(module) = modules.get(&import.module_file) else {
                continue;
            };
            for (service, gen_service) in &module.services {
                if gen_service.methods.contains(method) {
                    evidence.method_calls.insert((
                        import.module_file.clone(),
                        service.clone(),
                        method.clone(),
                    ));
                }
            }
        }
    }
}

/// A Python attribute chain: `mod.Msg` names the message (A1),
/// `mod.Msg.Enum.VALUE` / `mod.Enum.VALUE` names the enum value
/// (A2), and any other chain ending in a generated field name is a
/// field access (A3, receiver unknown).
fn scan_python_attribute(
    node: &tree_sitter::Node<'_>,
    text: &str,
    imports: &[ResolvedImport],
    modules: &BTreeMap<String, GenModule>,
    evidence: &mut FileEvidence,
) {
    let chain = python_chain(node, text);
    if chain.len() < 2 {
        return;
    }
    let Some((module_file, rest)) = match_chain(&chain, imports, modules)
    else {
        return;
    };
    let Some(module) = modules.get(&module_file) else {
        return;
    };
    // Message reference: the remainder is exactly the message, the
    // enum, or the service's Stub class.
    if rest.len() == 1 {
        let name = &rest[0];
        for message in module.messages.keys() {
            let short = message
                .rsplit_once('.')
                .map_or(message.as_str(), |(_, last)| last);
            if short == name || *message == *name {
                evidence.message_refs.insert((
                    module_file.clone(),
                    message.clone(),
                ));
            }
        }
        for enumeration in module.enums.keys() {
            let short = enumeration
                .rsplit_once('.')
                .map_or(enumeration.as_str(), |(_, last)| last);
            if short == name || *enumeration == *name {
                evidence.enum_refs.insert((
                    module_file.clone(),
                    enumeration.clone(),
                ));
            }
        }
        for service in module.services.keys() {
            let short = service
                .rsplit_once('.')
                .map_or(service.as_str(), |(_, last)| last);
            if *name == format!("{short}Stub") {
                evidence.service_refs.insert((
                    module_file.clone(),
                    service.clone(),
                ));
            }
        }
        // A from-imported root with one segment left: `Color.RED`
        // where Color was from-imported, or `User.name` where User
        // was. The chain root is the bound name, so the leaf is an
        // enum value of that enum or a field access on that message
        // — the same evidence the plain-import chain gives, through
        // the binding instead of the module path. Without this, the
        // most common Python enum-value form names nothing.
        if chain.len() == 2 {
            let root = &chain[0];
            for enumeration in module.enums.keys() {
                let short = enumeration
                    .rsplit_once('.')
                    .map_or(enumeration.as_str(), |(_, last)| last);
                if (short == root || *enumeration == *root)
                    && module.enums[enumeration].values.contains(name)
                {
                    evidence.enum_consts.insert((
                        module_file.clone(),
                        enumeration.clone(),
                        name.clone(),
                    ));
                }
            }
            for (message, gen_message) in &module.messages {
                let short = message
                    .rsplit_once('.')
                    .map_or(message.as_str(), |(_, last)| last);
                if short == root || *message == *root {
                    for field in gen_message.fields.values() {
                        if field.py == *name {
                            evidence.field_accesses.insert((
                                module_file.clone(),
                                message.clone(),
                                field.proto.clone(),
                            ));
                        }
                    }
                }
            }
        }
        return;
    }
    // Enum constant: the remainder ends in Enum + VALUE, optionally
    // rooted at the message the enum nests in.
    if rest.len() >= 2 {
        let (enum_name, value) =
            (&rest[rest.len() - 2], &rest[rest.len() - 1]);
        for enumeration in module.enums.keys() {
            let short = enumeration
                .rsplit_once('.')
                .map_or(enumeration.as_str(), |(_, last)| last);
            if short == enum_name || *enumeration == *enum_name {
                if module.enums[enumeration].values.contains(value) {
                    evidence.enum_consts.insert((
                        module_file.clone(),
                        enumeration.clone(),
                        value.clone(),
                    ));
                    return;
                }
            }
        }
    }
    // Anything else ending in a generated field name is a field
    // access on an unknown receiver (Tier A3).
    if let Some(field_name) = chain.last() {
        for (message, gen_message) in &module.messages {
            for field in gen_message.fields.values() {
                if field.py == *field_name {
                    evidence.field_accesses.insert((
                        module_file.clone(),
                        message.clone(),
                        field.proto.clone(),
                    ));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The measurement: diff joined to consumers, tier by tier.
// ---------------------------------------------------------------------------

/// Rank a `via` for deterministic consumer ordering: proven evidence
/// before name-match, in the design's listing order.
fn via_rank(via: ConsumerVia) -> u8 {
    match via {
        ConsumerVia::Import => 0,
        ConsumerVia::StructLiteral => 1,
        ConsumerVia::ConstructorKwarg => 2,
        ConsumerVia::EnumConst => 3,
        ConsumerVia::MethodCall => 4,
        ConsumerVia::FieldAccess => 5,
        ConsumerVia::JsonKey => 6,
    }
}

/// Sort consumers by file, then evidence rank, deduplicated.
fn sort_consumers(consumers: &mut Vec<SchemaConsumer>) {
    consumers.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then_with(|| via_rank(a.via).cmp(&via_rank(b.via)))
    });
    consumers.dedup_by(|a, b| a.file == b.file && a.via == b.via);
}

/// Measure schema impact: diff two schema indexes, join every break
/// to its consumers in the new tree's code.
///
/// The schema indexes come from the caller (the CLI builds them from
/// the two trees' `.proto` files); the code side comes from the two
/// trees' graphs plus the new tree's Go/Python texts, which the scan
/// reads for import-gated AST evidence. A1 for Python from-imports
/// comes from `breaking`'s `consumers()`; A1 for Go and for Python
/// plain imports comes from the AST message check, because
/// `consumers()` cannot see whole-module imports (see the module
/// docs). Generated files are never consumers, and namespace
/// (wildcard) imports are never evidence.
#[must_use]
pub fn measure_schema_impact(
    old_schema: &SchemaIndex,
    new_schema: &SchemaIndex,
    old_report: &GraphReport,
    new_report: &GraphReport,
    new_texts: &[(String, String)],
    from_ref: &str,
    to_ref: &str,
) -> SchemaImpactReport {
    let (pending, mut added) = diff_indexes(old_schema, new_schema);
    let mut coverage: Vec<String> = Vec::new();

    let texts: BTreeMap<String, String> =
        new_texts.iter().cloned().collect();
    let generated = find_generated(new_texts);
    let generated_files: BTreeSet<String> =
        generated.iter().map(|file| file.path.clone()).collect();

    if !old_schema.files.is_empty()
        || !new_schema.files.is_empty()
    {
        if generated.is_empty() {
            coverage.push(
                "Tier A is inapplicable: no generated Go or Python code \
                 was found in the tree, so no generated-code consumers \
                 could be seen. This report never claims \"0 consumers\" \
                 for a tree it could not see."
                    .to_owned(),
            );
        }
    }
    for file in &generated {
        if file.assumed {
            coverage.push(format!(
                "generator mapping assumed for {}: generated-code markers \
                 not detected, so the pinned mapping applies on the file \
                 suffix alone",
                file.path
            ));
        }
    }
    for (path, file) in &new_schema.files {
        if file.vendors_well_known {
            coverage.push(format!(
                "file {path} vendors well-known protos \
                 (google/protobuf/) and is indexed like any other file"
            ));
        }
    }

    let modules_vec = build_modules(
        old_schema,
        new_schema,
        &generated,
        &texts,
        &mut coverage,
    );
    let modules: BTreeMap<String, GenModule> = modules_vec
        .iter()
        .map(|module| (module.file.clone(), module.clone()))
        .collect();

    // A break whose element maps to no generated module in the new
    // tree cannot have joined consumers — the Go regenerated-removal
    // case, where the struct declaration the mapping keys on is
    // gone. The report says so per element instead of printing a
    // bare break that reads as "no consumers". When no generated
    // code exists at all, the Tier-A-inapplicable note above
    // already says it, so this only fires when generated files
    // exist but this element is unmapped.
    if !generated.is_empty() {
        for pending_break in &pending {
            if primary_generated_file(pending_break, &modules).is_none() {
                coverage.push(format!(
                    "consumers of {} could not be joined: no generated module in the new tree maps to it",
                    pending_break.element
                ));
            }
        }
    }

    // Scan every non-generated Go/Python file once.
    let mut evidence_by_file: BTreeMap<String, FileEvidence> =
        BTreeMap::new();
    let mut any_generated_import = false;
    for (path, text) in new_texts {
        if generated_files.contains(path) {
            continue;
        }
        if !(path.ends_with(".go")
            || path.ends_with(".py")
            || path.ends_with(".pyi"))
        {
            continue;
        }
        let lang = if path.ends_with(".go") {
            GeneratedLang::Go
        } else {
            GeneratedLang::Python
        };
        let mut parser = tree_sitter::Parser::new();
        let language_fn = match lang {
            GeneratedLang::Go => tree_sitter_go::LANGUAGE,
            GeneratedLang::Python => tree_sitter_python::LANGUAGE,
        };
        if parser.set_language(&language_fn.into()).is_err() {
            continue;
        }
        let Some(tree) = parser.parse(text, None) else {
            continue;
        };
        let root = tree.root_node();
        let imports = match lang {
            GeneratedLang::Go => {
                let specs = go_imports(&root, text);
                resolve_go_imports(path, &specs, &modules_vec, new_report)
            }
            GeneratedLang::Python => {
                let (plain, from_imports) = python_imports(&root, text);
                resolve_python_imports(
                    &plain,
                    &from_imports,
                    &modules_vec,
                    new_report,
                    path,
                )
            }
        };
        if !imports.is_empty() {
            any_generated_import = true;
        }
        let mut evidence = FileEvidence::default();
        match lang {
            GeneratedLang::Go => {
                scan_go(&root, path, text, &imports, &modules, &mut evidence);
            }
            GeneratedLang::Python => {
                scan_python(&root, text, &imports, &modules, &mut evidence);
            }
        }
        evidence_by_file.insert(path.clone(), evidence);
    }
    if !modules_vec.is_empty() && !any_generated_import {
        coverage.push(
            "no Go or Python code in the tree imports generated code; \
             Tier A has no consumers to report"
                .to_owned(),
        );
    }

    let oracle = ResolutionOracle::from_report(old_report);
    let mut breaks = Vec::new();
    for pending in pending {
        let (mut proven, mut named) =
            join_consumers(&pending, &modules, &evidence_by_file, new_report, &oracle);
        sort_consumers(&mut proven);
        sort_consumers(&mut named);
        let seeds: BTreeSet<&str> = proven
            .iter()
            .map(|consumer| consumer.file.as_str())
            .collect();
        let primary_file = primary_generated_file(&pending, &modules);
        let mut downstream = primary_file.map_or_else(Vec::new, |file| {
            downstream_of(new_report, &file, &seeds)
        });
        downstream.retain(|file| !generated_files.contains(file));
        breaks.push(SchemaBreak {
            schema_file: pending.schema_file,
            element: pending.element,
            change: pending.change,
            note: pending.note,
            proven,
            named,
            downstream,
        });
    }
    breaks.sort_by(|a, b| {
        a.element.cmp(&b.element).then_with(|| a.schema_file.cmp(&b.schema_file))
    });
    added.sort_by(|a, b| {
        a.element.cmp(&b.element).then_with(|| a.schema_file.cmp(&b.schema_file))
    });
    coverage.sort();
    coverage.dedup();
    SchemaImpactReport {
        from_ref: from_ref.to_owned(),
        to_ref: to_ref.to_owned(),
        breaks,
        added,
        coverage,
    }
}

/// The first generated file implementing a break's element, for the
/// downstream walk's exclusion seed.
fn primary_generated_file(
    pending: &PendingBreak,
    modules: &BTreeMap<String, GenModule>,
) -> Option<String> {
    let qualified = match &pending.target {
        BreakTarget::Message(name)
        | BreakTarget::Enum(name)
        | BreakTarget::Service(name) => name.clone(),
        BreakTarget::Field { message, .. } => message.clone(),
        BreakTarget::EnumValue { enumeration, .. } => enumeration.clone(),
        BreakTarget::Method { service, .. } => service.clone(),
    };
    for module in modules.values() {
        if module.messages.contains_key(&qualified)
            || module.enums.contains_key(&qualified)
            || module.services.contains_key(&qualified)
        {
            return Some(module.file.clone());
        }
    }
    None
}

/// Join one pending break to its consumers, tier by tier.
fn join_consumers(
    pending: &PendingBreak,
    modules: &BTreeMap<String, GenModule>,
    evidence_by_file: &BTreeMap<String, FileEvidence>,
    new_report: &GraphReport,
    oracle: &ResolutionOracle<'_>,
) -> (Vec<SchemaConsumer>, Vec<SchemaConsumer>) {
    let mut proven = Vec::new();
    let mut named = Vec::new();
    match &pending.target {
        BreakTarget::Message(message)
        | BreakTarget::Enum(message)
        | BreakTarget::Service(message) => {
            // A1: consumers() for Python from-imports, plus the AST
            // message check for Go and Python plain imports. The
            // symbol is the short name: that is what import
            // statements bind.
            let short = message
                .rsplit_once('.')
                .map_or(message.as_str(), |(_, last)| last);
            let is_service =
                matches!(pending.target, BreakTarget::Service(_));
            for module in modules.values() {
                let implements = if is_service {
                    module.services.contains_key(message)
                } else if matches!(pending.target, BreakTarget::Enum(_)) {
                    module.enums.contains_key(message)
                } else {
                    module.messages.contains_key(message)
                };
                if !implements {
                    continue;
                }
                let (direct, _) =
                    consumers(new_report, oracle, &module.file, short);
                for user in direct {
                    proven.push(SchemaConsumer {
                        file: user.file,
                        via: ConsumerVia::Import,
                    });
                }
            }
            for (file, evidence) in evidence_by_file {
                let refs = if is_service {
                    &evidence.service_refs
                } else if matches!(pending.target, BreakTarget::Enum(_)) {
                    &evidence.enum_refs
                } else {
                    &evidence.message_refs
                };
                if refs.iter().any(|(_, qualified)| qualified == message) {
                    proven.push(SchemaConsumer {
                        file: file.clone(),
                        via: ConsumerVia::Import,
                    });
                }
            }
        }
        BreakTarget::Field {
            message,
            field: old_field,
            json_name: old_json_name,
            ..
        } => {
            for (file, evidence) in evidence_by_file {
                for (_, qualified, field) in &evidence.struct_literals {
                    if qualified == message && field == old_field {
                        proven.push(SchemaConsumer {
                            file: file.clone(),
                            via: ConsumerVia::StructLiteral,
                        });
                    }
                }
                for (_, qualified, field) in &evidence.kwargs {
                    if qualified == message && field == old_field {
                        proven.push(SchemaConsumer {
                            file: file.clone(),
                            via: ConsumerVia::ConstructorKwarg,
                        });
                    }
                }
                for (_, qualified, field) in &evidence.field_accesses {
                    if qualified == message && field == old_field {
                        named.push(SchemaConsumer {
                            file: file.clone(),
                            via: ConsumerVia::FieldAccess,
                        });
                    }
                }
                for (_, qualified, field) in &evidence.json_keys {
                    if qualified == message && field == old_json_name {
                        named.push(SchemaConsumer {
                            file: file.clone(),
                            via: ConsumerVia::JsonKey,
                        });
                    }
                }
            }
        }
        BreakTarget::EnumValue { enumeration, value } => {
            for (file, evidence) in evidence_by_file {
                for (_, qualified, const_value) in &evidence.enum_consts {
                    if qualified == enumeration && const_value == value {
                        proven.push(SchemaConsumer {
                            file: file.clone(),
                            via: ConsumerVia::EnumConst,
                        });
                    }
                }
            }
        }
        BreakTarget::Method { service, method } => {
            for (file, evidence) in evidence_by_file {
                for (_, qualified, called) in &evidence.method_calls {
                    if qualified == service && called == method {
                        proven.push(SchemaConsumer {
                            file: file.clone(),
                            via: ConsumerVia::MethodCall,
                        });
                    }
                }
            }
        }
    }
    (proven, named)
}
