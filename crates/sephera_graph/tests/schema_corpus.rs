//! Biconditional corpus for `sephera schema-impact`.
//!
//! Every scenario spells an old proto, a new proto, the new tree's
//! Go/Python texts, and the new/old graphs, then labels the exact
//! expected breaks: element, change, proven consumers (file, via),
//! name-match consumers (file, via), downstream files, and added
//! elements. The runner asserts the predicted report EQUALS the
//! labelled one — no predicted break outside the label, no labelled
//! break unpredicted, no consumer the label does not name. A
//! scenario that only checked "this consumer is found" would pass
//! a join that also claimed ten files it should not; this one
//! does not.
//!
//! The fixtures spell code the way the generators leave it:
//! Python from-imports carry an edge with items plus the
//! unresolved item half the real resolver leaves beside it, Go
//! imports carry no items (a whole-module import records no
//! names), and generated files carry their real markers. A fixture
//! that relied on anything subtler would test the easy case and
//! miss the ones this command exists for: a removal whose
//! consumers name old elements, and a rename whose consumers name
//! the old name while migrated files name the new one.

use std::path::PathBuf;

use sephera_graph::schema::{
    SchemaChange, build_schema_index, measure_schema_impact,
};
use sephera_graph::types::{
    GraphEdge, GraphMetrics, GraphNode, GraphReport, ImportKind,
};

fn edge(from: &str, to: &str, path: &str, items: &[&str]) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: Some(to.to_owned()),
        import_path: path.to_owned(),
        items: items.iter().map(|item| (*item).to_owned()).collect(),
        resolved: true,
        kind: ImportKind::Dependency,
        local_gap: false,
        cfg_gated: false,
    }
}

/// An unresolved item half: the shape a Python item import leaves
/// beside its module edge. The half names the symbol but never
/// resolves as its own edge.
fn gap_edge(from: &str, path: &str) -> GraphEdge {
    GraphEdge {
        from: from.to_owned(),
        to: None,
        import_path: path.to_owned(),
        items: Vec::new(),
        resolved: false,
        kind: ImportKind::Dependency,
        local_gap: true,
        cfg_gated: false,
    }
}

fn node(path: &str) -> GraphNode {
    GraphNode {
        file_path: path.to_owned(),
        language: None,
        imports_count: 0,
        imported_by_count: 0,
    }
}

fn report(files: &[&str], edges: Vec<GraphEdge>) -> GraphReport {
    GraphReport {
        base_path: PathBuf::from("."),
        focus_paths: vec![],
        depth: None,
        query: None,
        nodes: files.iter().copied().map(node).collect(),
        edges,
        boundary_files: Vec::new(),
        metrics: GraphMetrics {
            total_files: 0,
            total_internal_edges: 0,
            self_references: 0,
            total_external_edges: 0,
            unresolved_local_edges: 0,
            unresolved_local_samples: Vec::new(),
            cfg_gated_edges: 0,
            dependencies: Vec::new(),
            declared_dependency_edges: 0,
            local_crate_edges: 0,
            builtin_edges: 0,
            circular_dependencies: 0,
            most_importing: vec![],
            most_imported: vec![],
            cycles: vec![],
        },
    }
}

/// One labelled break: the element, the change, and exactly the
/// consumers the join may claim, split by tier.
struct ExpBreak {
    element: &'static str,
    change: SchemaChange,
    proven: Vec<(&'static str, &'static str)>,
    named: Vec<(&'static str, &'static str)>,
    downstream: Vec<&'static str>,
    note: Option<&'static str>,
}

/// One corpus scenario.
struct Case {
    name: &'static str,
    old_protos: Vec<(&'static str, &'static str)>,
    new_protos: Vec<(&'static str, &'static str)>,
    /// New tree's Go/Python texts: generated files first, then
    /// consumers. The proto texts are NOT in here — the schema
    /// index is built from the proto pairs above.
    texts: Vec<(&'static str, &'static str)>,
    old_edges: Vec<GraphEdge>,
    new_edges: Vec<GraphEdge>,
    expected: Vec<ExpBreak>,
    expected_added: Vec<&'static str>,
    /// Coverage substrings that must appear in the report.
    expected_notes: Vec<&'static str>,
}

fn run(case: &Case) -> Result<(), String> {
    let old_index = build_schema_index(
        &case
            .old_protos
            .iter()
            .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
            .collect::<Vec<_>>(),
    )
    .map_err(|error| format!("{}: old schema failed to parse: {error}", case.name))?;
    let new_index = build_schema_index(
        &case
            .new_protos
            .iter()
            .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
            .collect::<Vec<_>>(),
    )
    .map_err(|error| format!("{}: new schema failed to parse: {error}", case.name))?;
    let files: Vec<&str> = case
        .texts
        .iter()
        .map(|(path, _)| *path)
        .collect();
    let measured = measure_schema_impact(
        &old_index,
        &new_index,
        &report(&files, case.old_edges.clone()),
        &report(&files, case.new_edges.clone()),
        &case
            .texts
            .iter()
            .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
            .collect::<Vec<_>>(),
        "old",
        "new",
    );

    let mut actual: Vec<(
        String,
        SchemaChange,
        Vec<(String, String)>,
        Vec<(String, String)>,
        Vec<String>,
        Option<String>,
    )> = measured
        .breaks
        .iter()
        .map(|broken| {
            (
                broken.element.clone(),
                broken.change.clone(),
                broken
                    .proven
                    .iter()
                    .map(|consumer| {
                        (
                            consumer.file.clone(),
                            consumer.via.describe().to_owned(),
                        )
                    })
                    .collect(),
                broken
                    .named
                    .iter()
                    .map(|consumer| {
                        (
                            consumer.file.clone(),
                            consumer.via.describe().to_owned(),
                        )
                    })
                    .collect(),
                broken.downstream.clone(),
                broken.note.clone(),
            )
        })
        .collect();
    actual.sort_by(|a, b| a.0.cmp(&b.0));
    let mut expected: Vec<(
        String,
        SchemaChange,
        Vec<(String, String)>,
        Vec<(String, String)>,
        Vec<String>,
        Option<String>,
    )> = case
        .expected
        .iter()
        .map(|broken| {
            (
                broken.element.to_owned(),
                broken.change.clone(),
                broken
                    .proven
                    .iter()
                    .map(|(file, via)| ((*file).to_owned(), (*via).to_owned()))
                    .collect(),
                broken
                    .named
                    .iter()
                    .map(|(file, via)| ((*file).to_owned(), (*via).to_owned()))
                    .collect(),
                broken
                    .downstream
                    .iter()
                    .map(|file| (*file).to_owned())
                    .collect(),
                broken.note.map(str::to_owned),
            )
        })
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    if actual != expected {
        return Err(format!(
            "{}: predicted breaks != labelled breaks\npredicted: {actual:#?}\nlabelled: {expected:#?}",
            case.name
        ));
    }

    let mut actual_added: Vec<&str> = measured
        .added
        .iter()
        .map(|element| element.element.as_str())
        .collect();
    actual_added.sort_unstable();
    let mut expected_added = case.expected_added.to_vec();
    expected_added.sort_unstable();
    if actual_added != expected_added {
        return Err(format!(
            "{}: predicted added != labelled added\npredicted: {actual_added:?}\nlabelled: {expected_added:?}",
            case.name
        ));
    }

    for note in &case.expected_notes {
        if !measured.coverage.iter().any(|found| found.contains(note)) {
            return Err(format!(
                "{}: expected a coverage note containing {note:?}, got {:?}",
                case.name, measured.coverage
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Proto fixtures. The base proto is one file, `api/user.proto`, so
// every variant below is a small, readable diff from it.
// ---------------------------------------------------------------------------

const OLD_PROTO: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

/// Like `OLD_PROTO`, but the rpc method carries a body of custom
/// options — the shape a real repo (metal-stack/api's
/// `wrongproject.proto`) uses. `skip_balanced` never handled this
/// until the depth fix: it started at zero after the caller had
/// already consumed the opener, so the first closer drove it to
/// minus one and it ran to end of file.
const OLD_METHOD_BODY_OPTS: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User) {
    option (tenant_roles) = TENANT_ROLE_OWNER;
    option (metalstack.api.v2.auditing) = AUDITING_INCLUDED;
  }
}
"#;

/// The method-body service with the method removed; the service
/// keeps an option so the body is non-empty and parses.
const NEW_METHOD_BODY_REMOVED: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  option deprecated = true;
}
"#;

/// Like `OLD_PROTO`, but the `RED` value carries a custom option.
const OLD_ENUM_VALUE_OPTS: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1 [(custom.opt) = true];
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

/// The enum-option proto with `RED` removed.
const NEW_ENUM_VALUE_OPTS_REMOVED: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_REMOVE_NAME: &str = r#"syntax = "proto3";
package api;
message User {
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_RENAME_NAME: &str = r#"syntax = "proto3";
package api;
message User {
  string full_name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_RENAME_NICK: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string handle = 5;
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_TYPE_NAME: &str = r#"syntax = "proto3";
package api;
message User {
  int32 name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_NUMBER_NAME: &str = r#"syntax = "proto3";
package api;
message User {
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
  string name = 9;
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_LABEL_AGE: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  repeated int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_ONEOF_EMAIL: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  oneof contact {
    string email = 3;
  }
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_REMOVE_USER: &str = r#"syntax = "proto3";
package api;
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_REMOVE_RED: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_RENAME_RED: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  CRIMSON = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_REMOVE_COLOR: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_REMOVE_SERVICE: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
"#;

const NEW_REMOVE_METHOD: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  option deprecated = true;
}
"#;

const NEW_INPUT: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (User) returns (User);
}
"#;

const NEW_OUTPUT: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (GetUserRequest);
}
"#;

const NEW_RENAME_METHOD: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc FetchUser (GetUserRequest) returns (User);
}
"#;

const NEW_RENAME_MESSAGE: &str = r#"syntax = "proto3";
package api;
message Account {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (Account);
}
"#;

const NEW_ADD_FIELD: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
  string phone = 6;
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_ADD_MESSAGE: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
message Address {
  string street = 1;
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_REUSE_NUMBER: &str = r#"syntax = "proto3";
package api;
message User {
  int32 code = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_RESERVED: &str = r#"syntax = "proto3";
package api;
message User {
  reserved 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_REMOVE_USER_ID: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string nickname = 5 [json_name = "nick"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const NEW_JSON_ONLY: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  int32 age = 2;
  string email = 3;
  string user_id = 4;
  string nickname = 5 [json_name = "nick2"];
}
enum Color {
  COLOR_UNSPECIFIED = 0;
  RED = 1;
  BLUE = 2;
}
message GetUserRequest {
  string user_id = 1;
}
service UserService {
  rpc GetUser (GetUserRequest) returns (User);
}
"#;

const OLD_MAP_PROTO: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  map<string, string> labels = 6;
}
"#;

const NEW_MAP_PROTO: &str = r#"syntax = "proto3";
package api;
message User {
  string name = 1;
  map<string, int32> labels = 6;
}
"#;

const OLD_NESTED_PROTO: &str = r#"syntax = "proto3";
package api;
message Outer {
  message Inner {
    string city = 1;
  }
}
"#;

const NEW_NESTED_PROTO: &str = r#"syntax = "proto3";
package api;
message Outer {
  message Inner {
    reserved 1;
  }
}
"#;

const OLD_AMBIG_A: &str = r#"syntax = "proto3";
package a;
message Shared {
  string name = 1;
}
"#;

const OLD_AMBIG_B: &str = r#"syntax = "proto3";
package b;
message Shared {
  string name = 1;
}
"#;

const NEW_AMBIG_A: &str = r#"syntax = "proto3";
package a;
message Shared {
  reserved 1;
}
"#;

const OLD_EXTRA_PROTO: &str = r#"syntax = "proto3";
package api;
message OldMsg {
  string name = 1;
}
"#;

// ---------------------------------------------------------------------------
// Generated-code fixtures. Python modules pair to protos by path
// (`api/user.proto` <-> `api/user_pb2.py`); Go modules pair by the
// struct/interface names their generated file declares.
// ---------------------------------------------------------------------------

const PY_GEN: &str = "# Generated by the protocol buffer compiler.  DO NOT EDIT!\n";

const PY_GRPC_GEN: &str = "# Generated by the protocol buffer compiler.  DO NOT EDIT!\n";

const PYI_GEN: &str = "# Generated by the protocol buffer compiler.  DO NOT EDIT!\n";

const GO_GEN: &str = r#"// Code generated by protoc-gen-go. DO NOT EDIT.
package api

type User struct{}
type Color int32
type GetUserRequest struct{}
type UserServiceClient interface {
	GetUser()
}
"#;

const GO_GEN_NO_USER: &str = r#"// Code generated by protoc-gen-go. DO NOT EDIT.
package api

type Color int32
type GetUserRequest struct{}
type UserServiceClient interface {
	GetUser()
}
"#;

const GO_GEN_WITH_LITERAL: &str = r#"// Code generated by protoc-gen-go. DO NOT EDIT.
package api

type User struct{}
type Color int32
type GetUserRequest struct{}
type UserServiceClient interface {
	GetUser()
}

var _ = User{Name: "generated"}
"#;

const GO_GEN_AMBIG: &str = r#"// Code generated by protoc-gen-go. DO NOT EDIT.
package shared

type Shared struct{}
"#;

// ---------------------------------------------------------------------------
// Consumer fixtures.
// ---------------------------------------------------------------------------

const PY_FROM_KWARGS: &str = r#"from api.user_pb2 import User


def make():
    return User(name="Ada", age=36)
"#;

const PY_PLAIN_KWARGS: &str = r#"import api.user_pb2


def make():
    return api.user_pb2.User(name="Ada")
"#;

const PY_PLAIN_MSG: &str = r#"import api.user_pb2


def get():
    return api.user_pb2.User
"#;

const PY_MSG_REF: &str = r#"from api.user_pb2 import User


def get():
    return User
"#;

const PY_PLAIN_ATTR: &str = r#"import api.user_pb2


def get():
    return api.user_pb2.User.name
"#;

const PY_FROM_ATTR: &str = r#"from api.user_pb2 import User


def get():
    return User.name
"#;

const PY_FROM_ENUM: &str = r#"from api.user_pb2 import Color


def get():
    return Color.RED
"#;

const PY_PLAIN_ENUM: &str = r#"import api.user_pb2


def get():
    return api.user_pb2.Color.RED
"#;

const PY_SERVICE_CALL: &str = r#"from api.user_pb2_grpc import UserServiceStub


def call(stub):
    return stub.GetUser(req)


def make_stub(channel):
    return UserServiceStub(channel)
"#;

const PY_NOINPORT: &str = r#"def make():
    return User(name="Ada")
"#;

const PY_OTHER_IMPORT: &str = r#"import other


def make():
    return other.User(name="Ada")
"#;

const PY_COMMENT: &str = r#"from api.user_pb2 import User


# Old call, commented out: User(name="Ada")
def make():
    return None
"#;

const PY_SUBSTRING: &str = r#"from api.user_pb2 import User


def make():
    username = "Ada"
    return username
"#;

const PY_JSONKEY: &str = r#"import api.user_pb2


def get(data):
    return data["userId"]
"#;

const PY_NICK_JSON: &str = r#"import api.user_pb2


def get(data):
    return data["nick"]
"#;

const PY_WILDCARD: &str = r#"from api.user_pb2 import *


def make():
    return User(name="Ada")
"#;

const GO_LITERAL: &str = r#"package main

import apiv1 "example.com/api"

var u = apiv1.User{Name: "Ada", Age: 36}
"#;

const GO_LITERAL_BARE: &str = r#"package api

import apiv1 "example.com/api"

var u = User{Name: "Ada"}
"#;

const GO_MSG_REF: &str = r#"package main

import apiv1 "example.com/api"

x := apiv1.User
"#;

const GO_ACCESS: &str = r#"package main

import apiv1 "example.com/api"

func get(u *apiv1.User) string {
	return u.Name
}
"#;

const GO_ENUM: &str = r#"package main

import apiv1 "example.com/api"

var c = apiv1.Color_RED
"#;

const GO_SERVICE_REF: &str = r#"package main

import apiv1 "example.com/api"

x := apiv1.UserServiceClient
"#;

const GO_METHOD: &str = r#"package main

import apiv1 "example.com/api"

func call(client apiv1.UserServiceClient) {
	client.GetUser(ctx, req)
}
"#;

const GO_STRING_JSON: &str = r#"package main

import apiv1 "example.com/api"

var key = "userId"
"#;

const GO_MAP_LITERAL: &str = r#"package main

import apiv1 "example.com/api"

var u = apiv1.User{Labels: map[string]string{}}
"#;

const GO_NESTED_LITERAL: &str = r#"package main

import apiv1 "example.com/api"

var inner = apiv1.Outer_Inner{City: "Oslo"}
"#;

const PY_NESTED_KWARGS: &str = r#"from api.nested_pb2 import Outer


def make():
    return Outer.Inner(city="Oslo")
"#;

const GO_AMBIG_CONSUMER: &str = r#"package main

import shared "example.com/shared"

var s = shared.Shared{Name: "Ada"}
"#;

// ---------------------------------------------------------------------------
// The corpus. Edge conventions mirror the real resolver: a Python
// from-import leaves a resolved module edge plus the unresolved item
// half beside it; a Python plain import and a Go import leave one
// resolved edge with no items. `old_edges` feeds the oracle.
// ---------------------------------------------------------------------------

fn py_from_edges(consumer: &str, gen_file: &str, module: &str) -> Vec<GraphEdge> {
    vec![
        edge(consumer, gen_file, module, &[]),
        gap_edge(consumer, &format!("{module}.User")),
    ]
}

const USER: &str = "api/user.proto";
const PY_GEN_PATH: &str = "api/user_pb2.py";
const PY_GRPC_PATH: &str = "api/user_pb2_grpc.py";
const GO_GEN_PATH: &str = "api/user.pb.go";
const PY_CONSUMER: &str = "consumer.py";
const GO_CONSUMER: &str = "consumer.go";

fn cases() -> Vec<Case> {
    let user = USER;
    let py_gen = PY_GEN_PATH;
    let py_grpc = PY_GRPC_PATH;
    let go_gen = GO_GEN_PATH;
    let py_consumer = PY_CONSUMER;
    let go_consumer = GO_CONSUMER;
    vec![
        // 1. Field removed, Python constructor kwargs: the core A2
        // shape. Only the removed field's kwarg is claimed; the
        // age kwarg in the same call is not a consumer of this
        // break.
        Case {
            name: "py_field_removed_kwargs",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_KWARGS)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 2. Field removed, Go struct literal.
        Case {
            name: "go_field_removed_literal",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(go_gen, GO_GEN), (go_consumer, GO_LITERAL)],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![("consumer.go", "struct literal")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 3. Field removed, nobody uses it: the break is still
        // listed, and the report says the tree imports no generated
        // code rather than silently claiming zero consumers.
        Case {
            name: "field_removed_no_consumers",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_gen, PY_GEN)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec!["imports generated code"],
        },
        // 4. Field renamed, consumer names the OLD name: proven.
        // This is the rename-join regression: the join used to
        // match the new name, claiming migrated files and missing
        // the files that actually break.
        Case {
            name: "py_field_renamed_old_kwargs",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_RENAME_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_KWARGS)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Renamed {
                    from: "name".to_owned(),
                    to: "full_name".to_owned(),
                },
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 5. Field renamed, consumer already on the NEW name: not
        // a consumer of the break. A join that matched the new name
        // would claim this file; the old-name join does not.
        Case {
            name: "py_field_renamed_new_kwargs",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_RENAME_NAME)],
            texts: vec![
                (py_gen, PY_GEN),
                (
                    py_consumer,
                    "from api.user_pb2 import User\n\n\ndef make():\n    return User(full_name=\"Ada\")\n",
                ),
            ],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Renamed {
                    from: "name".to_owned(),
                    to: "full_name".to_owned(),
                },
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 6. Field renamed with an explicit old json_name: a JSON
        // key naming the OLD wire name is name-match evidence for
        // the rename. The join matches the old json_name, not the
        // new one.
        Case {
            name: "py_field_renamed_old_json",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_RENAME_NICK)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_NICK_JSON)],
            old_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            new_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            expected: vec![ExpBreak {
                element: "api.User.nickname",
                change: SchemaChange::Renamed {
                    from: "nickname".to_owned(),
                    to: "handle".to_owned(),
                },
                proven: vec![],
                named: vec![("consumer.py", "JSON key")],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 7. Field type changed, Go struct literal.
        Case {
            name: "go_field_type_changed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_TYPE_NAME)],
            texts: vec![(go_gen, GO_GEN), (go_consumer, GO_LITERAL)],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::TypeChanged {
                    from: "string".to_owned(),
                    to: "int32".to_owned(),
                },
                proven: vec![("consumer.go", "struct literal")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 8. Field number changed: source still names the field,
        // so the kwarg is proven — and the kind is its own, never
        // folded into a rename.
        Case {
            name: "py_field_number_changed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_NUMBER_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_KWARGS)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::NumberChanged { from: 1, to: 9 },
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 9. Label changed, Python kwargs.
        Case {
            name: "py_field_label_changed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_LABEL_AGE)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_KWARGS)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.age",
                change: SchemaChange::LabelChanged {
                    from: "singular".to_owned(),
                    to: "repeated".to_owned(),
                },
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 10. Oneof membership changed, Python kwargs.
        Case {
            name: "py_field_oneof_changed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_ONEOF_EMAIL)],
            texts: vec![
                (py_gen, PY_GEN),
                (
                    py_consumer,
                    "from api.user_pb2 import User\n\n\ndef make():\n    return User(email=\"a@b.c\")\n",
                ),
            ],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.email",
                change: SchemaChange::OneofChanged {
                    from: None,
                    to: Some("contact".to_owned()),
                },
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 11. Message removed, Python from-import: A1 through both
        // legs (consumers() via the item half, AST message ref),
        // deduplicated to one Import entry.
        Case {
            name: "py_message_removed_from",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_USER)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_MSG_REF)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "import")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 12. Message removed, Go package-qualified selector.
        Case {
            name: "go_message_removed_selector",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_USER)],
            texts: vec![(go_gen, GO_GEN), (go_consumer, GO_MSG_REF)],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.User",
                change: SchemaChange::Removed,
                proven: vec![("consumer.go", "import")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 13. Message removed, Python plain import + qualified use.
        Case {
            name: "py_message_removed_plain",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_USER)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_PLAIN_MSG)],
            old_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            new_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            expected: vec![ExpBreak {
                element: "api.User",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "import")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 14. Message removed, no consumers.
        Case {
            name: "message_removed_no_consumers",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_USER)],
            texts: vec![(py_gen, PY_GEN)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![ExpBreak {
                element: "api.User",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec!["imports generated code"],
        },
        // 15. Enum value removed, Python from-import: the most
        // common Python enum-value form, which the attribute scan
        // used to miss entirely (bound root, one segment left).
        Case {
            name: "py_enum_value_removed_from",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_RED)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_ENUM)],
            old_edges: vec![
                edge(py_consumer, py_gen, "api.user_pb2", &[]),
                gap_edge(py_consumer, "api.user_pb2.Color"),
            ],
            new_edges: vec![
                edge(py_consumer, py_gen, "api.user_pb2", &[]),
                gap_edge(py_consumer, "api.user_pb2.Color"),
            ],
            expected: vec![ExpBreak {
                element: "api.Color.RED",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "enum constant")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 16. Enum value removed, Python plain import.
        Case {
            name: "py_enum_value_removed_plain",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_RED)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_PLAIN_ENUM)],
            old_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            new_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            expected: vec![ExpBreak {
                element: "api.Color.RED",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "enum constant")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 17. Enum value removed, Go package-qualified constant.
        Case {
            name: "go_enum_value_removed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_RED)],
            texts: vec![(go_gen, GO_GEN), (go_consumer, GO_ENUM)],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.Color.RED",
                change: SchemaChange::Removed,
                proven: vec![("consumer.go", "enum constant")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 18. Enum value renamed, consumer names the old value.
        Case {
            name: "py_enum_value_renamed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_RENAME_RED)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_ENUM)],
            old_edges: vec![
                edge(py_consumer, py_gen, "api.user_pb2", &[]),
                gap_edge(py_consumer, "api.user_pb2.Color"),
            ],
            new_edges: vec![
                edge(py_consumer, py_gen, "api.user_pb2", &[]),
                gap_edge(py_consumer, "api.user_pb2.Color"),
            ],
            expected: vec![ExpBreak {
                element: "api.Color.RED",
                change: SchemaChange::Renamed {
                    from: "RED".to_owned(),
                    to: "CRIMSON".to_owned(),
                },
                proven: vec![("consumer.py", "enum constant")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 19. Whole enum removed, Python from-import: the enum
        // identifier itself is the A1 evidence.
        Case {
            name: "enum_removed_whole",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_COLOR)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_ENUM)],
            old_edges: vec![
                edge(py_consumer, py_gen, "api.user_pb2", &[]),
                gap_edge(py_consumer, "api.user_pb2.Color"),
            ],
            new_edges: vec![
                edge(py_consumer, py_gen, "api.user_pb2", &[]),
                gap_edge(py_consumer, "api.user_pb2.Color"),
            ],
            expected: vec![ExpBreak {
                element: "api.Color",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "import")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 20. Service removed, Python Stub import + method call:
        // the Stub identifier is the service evidence; the method
        // call is not double-counted as a service consumer.
        Case {
            name: "py_service_removed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_SERVICE)],
            texts: vec![(py_grpc, PY_GRPC_GEN), (py_consumer, PY_SERVICE_CALL)],
            old_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            new_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            expected: vec![ExpBreak {
                element: "api.UserService",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "import")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 21. Service removed, Go client interface reference.
        Case {
            name: "go_service_removed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_SERVICE)],
            texts: vec![(go_gen, GO_GEN), (go_consumer, GO_SERVICE_REF)],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.UserService",
                change: SchemaChange::Removed,
                proven: vec![("consumer.go", "import")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 22. Method removed, Python call-position attribute in a
        // file importing the grpc module.
        Case {
            name: "py_method_removed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_METHOD)],
            texts: vec![(py_grpc, PY_GRPC_GEN), (py_consumer, PY_SERVICE_CALL)],
            old_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            new_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            expected: vec![ExpBreak {
                element: "api.UserService.GetUser",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "method call")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 23. Method removed, Go call-position selector.
        Case {
            name: "go_method_removed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_METHOD)],
            texts: vec![(go_gen, GO_GEN), (go_consumer, GO_METHOD)],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.UserService.GetUser",
                change: SchemaChange::Removed,
                proven: vec![("consumer.go", "method call")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 24. Method input changed, Python call.
        Case {
            name: "py_method_input_changed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_INPUT)],
            texts: vec![(py_grpc, PY_GRPC_GEN), (py_consumer, PY_SERVICE_CALL)],
            old_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            new_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            expected: vec![ExpBreak {
                element: "api.UserService.GetUser",
                change: SchemaChange::InputChanged {
                    from: "GetUserRequest".to_owned(),
                    to: "User".to_owned(),
                },
                proven: vec![("consumer.py", "method call")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 25. Method output changed, Python call.
        Case {
            name: "py_method_output_changed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_OUTPUT)],
            texts: vec![(py_grpc, PY_GRPC_GEN), (py_consumer, PY_SERVICE_CALL)],
            old_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            new_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            expected: vec![ExpBreak {
                element: "api.UserService.GetUser",
                change: SchemaChange::OutputChanged {
                    from: "User".to_owned(),
                    to: "GetUserRequest".to_owned(),
                },
                proven: vec![("consumer.py", "method call")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 26. Method renamed: methods have no numbers, so this
        // reads as removal + addition — and the old method's
        // callers ARE proven consumers of the removal, because a
        // call is evidence, not an identity guess.
        Case {
            name: "method_renamed_reads_removal_addition",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_RENAME_METHOD)],
            texts: vec![(py_grpc, PY_GRPC_GEN), (py_consumer, PY_SERVICE_CALL)],
            old_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            new_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            expected: vec![ExpBreak {
                element: "api.UserService.GetUser",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "method call")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec!["api.UserService.FetchUser"],
            expected_notes: vec![],
        },
        // 27. Message renamed: messages have no numbers, so this
        // reads as removal + addition, never a guessed rename —
        // and the old message's importers are proven consumers of
        // the removal.
        Case {
            name: "message_renamed_reads_removal_addition",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_RENAME_MESSAGE)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_MSG_REF)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![
                ExpBreak {
                    element: "api.User",
                    change: SchemaChange::Removed,
                    proven: vec![("consumer.py", "import")],
                    named: vec![],
                    downstream: vec![],
                    note: None,
                },
                ExpBreak {
                    element: "api.UserService.GetUser",
                    change: SchemaChange::OutputChanged {
                        from: "User".to_owned(),
                        to: "Account".to_owned(),
                    },
                    proven: vec![],
                    named: vec![],
                    downstream: vec![],
                    note: None,
                },
            ],
            expected_added: vec!["api.Account"],
            expected_notes: vec![],
        },
        // 28. Field added: informational only, never a break.
        Case {
            name: "field_added_informational",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_ADD_FIELD)],
            texts: vec![(py_gen, PY_GEN)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![],
            expected_added: vec!["api.User.phone"],
            expected_notes: vec![],
        },
        // 29. Message added: informational only.
        Case {
            name: "message_added_informational",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_ADD_MESSAGE)],
            texts: vec![(py_gen, PY_GEN)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![],
            expected_added: vec!["api.Address"],
            expected_notes: vec![],
        },
        // 30. Number reuse with a different type: removal +
        // addition, never a rename. The rename proof needs same
        // number AND same type AND the old name absent.
        Case {
            name: "number_reuse_not_rename",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REUSE_NUMBER)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_KWARGS)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec!["api.User.code"],
            expected_notes: vec![],
        },
        // 31. Reserved number: the deletion still breaks source
        // consumers, but the note says wire reuse is blocked.
        Case {
            name: "reserved_number_note",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_RESERVED)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_KWARGS)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: Some("field number 1 is reserved in the new side"),
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 32. Map field value type changed: reported at the map
        // field itself; the synthetic entry message is never an
        // element, so no LabelsEntry break may appear.
        Case {
            name: "map_field_type_changed",
            old_protos: vec![(user, OLD_MAP_PROTO)],
            new_protos: vec![(user, NEW_MAP_PROTO)],
            texts: vec![(go_gen, GO_GEN), (go_consumer, GO_MAP_LITERAL)],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.User.labels",
                change: SchemaChange::TypeChanged {
                    from: "map<string, string>".to_owned(),
                    to: "map<string, int32>".to_owned(),
                },
                proven: vec![("consumer.go", "struct literal")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 33. Nested message field removed, Go struct literal with
        // the underscore-joined struct name.
        Case {
            name: "go_nested_field_removed",
            old_protos: vec![("api/nested.proto", OLD_NESTED_PROTO)],
            new_protos: vec![("api/nested.proto", NEW_NESTED_PROTO)],
            texts: vec![
                (
                    "api/nested.pb.go",
                    "// Code generated by protoc-gen-go. DO NOT EDIT.\npackage api\n\ntype Outer_Inner struct{}\n",
                ),
                (go_consumer, GO_NESTED_LITERAL),
            ],
            old_edges: vec![edge(
                go_consumer,
                "api/nested.pb.go",
                "example.com/api",
                &[],
            )],
            new_edges: vec![edge(
                go_consumer,
                "api/nested.pb.go",
                "example.com/api",
                &[],
            )],
            expected: vec![ExpBreak {
                element: "api.Outer.Inner.city",
                change: SchemaChange::Removed,
                proven: vec![("consumer.go", "struct literal")],
                named: vec![],
                downstream: vec![],
                note: Some("field number 1 is reserved in the new side"),
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 34. Nested message field removed, Python constructor
        // kwargs through the outer message.
        Case {
            name: "py_nested_field_removed",
            old_protos: vec![("api/nested.proto", OLD_NESTED_PROTO)],
            new_protos: vec![("api/nested.proto", NEW_NESTED_PROTO)],
            texts: vec![
                ("api/nested_pb2.py", PY_GEN),
                (py_consumer, PY_NESTED_KWARGS),
            ],
            old_edges: vec![
                edge(py_consumer, "api/nested_pb2.py", "api.nested_pb2", &[]),
                gap_edge(py_consumer, "api.nested_pb2.Outer"),
            ],
            new_edges: vec![
                edge(py_consumer, "api/nested_pb2.py", "api.nested_pb2", &[]),
                gap_edge(py_consumer, "api.nested_pb2.Outer"),
            ],
            expected: vec![ExpBreak {
                element: "api.Outer.Inner.city",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: Some("field number 1 is reserved in the new side"),
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 35. Downstream: a file importing a proven consumer is
        // downstream, not a consumer — it is listed apart, and the
        // consumer itself is not downstream of itself.
        Case {
            name: "downstream_listed_apart",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![
                (py_gen, PY_GEN),
                (py_consumer, PY_FROM_KWARGS),
                (
                    "app.py",
                    "import consumer\n\n\ndef run():\n    return consumer.make()\n",
                ),
            ],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: {
                let mut edges = py_from_edges(py_consumer, py_gen, "api.user_pb2");
                edges.push(edge("app.py", py_consumer, "consumer", &[]));
                edges
            },
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec!["app.py"],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 36. A generated file naming the field is not a
        // consumer: generated files are the declaration side.
        Case {
            name: "generated_file_not_consumer",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(go_gen, GO_GEN_WITH_LITERAL)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec!["imports generated code"],
        },
        // 37. No import, no evidence: a file that names the field
        // without importing the generated module is not a consumer
        // in any tier.
        Case {
            name: "py_noimport_not_consumer",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_NOINPORT)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec!["imports generated code"],
        },
        // 38. Importing a different module: the import gate is
        // per-module, not per-name.
        Case {
            name: "py_other_import_not_consumer",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_OTHER_IMPORT)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec!["imports generated code"],
        },
        // 39. A commented-out call is not evidence: comments are
        // never scanned.
        Case {
            name: "py_comment_not_evidence",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_COMMENT)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 40. Substring is not a match: `username` never evidences
        // the field `name`. Whole-identifier matching only.
        Case {
            name: "py_substring_not_evidence",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_SUBSTRING)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 41. Python field access, Tier A3: a chain rooted at the
        // imported module ending in the field name, receiver
        // unknown — flagged, never gated.
        Case {
            name: "py_field_access_a3",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_PLAIN_ATTR)],
            old_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            new_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![("consumer.py", "field access")],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 42. Python from-import field access, Tier A3: the bound
        // root form, which the attribute scan used to miss.
        Case {
            name: "py_from_field_access_a3",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_ATTR)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![("consumer.py", "field access")],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 43. JSON key, Tier A3: a string equal to the derived
        // json_name in a file importing the module. The field is
        // removed, so only the old-schema catalog can see the key
        // — a new-schema catalog would record nothing.
        Case {
            name: "py_jsonkey_a3",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_USER_ID)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_JSONKEY)],
            old_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            new_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            expected: vec![ExpBreak {
                element: "api.User.user_id",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![("consumer.py", "JSON key")],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 44. Go field access, Tier A3: a selector on an unknown
        // receiver naming a generated field.
        Case {
            name: "go_field_access_a3",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(go_gen, GO_GEN), (go_consumer, GO_ACCESS)],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![("consumer.go", "field access")],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 45. Go JSON key, Tier A3: a string literal equal to the
        // json_name in a file importing the module.
        Case {
            name: "go_jsonkey_a3",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_USER_ID)],
            texts: vec![(go_gen, GO_GEN), (go_consumer, GO_STRING_JSON)],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.User.user_id",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![("consumer.go", "JSON key")],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 46. New file importing for the first time: no module edge
        // in the new graph at all, only the item half — the AST
        // message ref still proves the import, which is what
        // covers consumers() when the graph half is missing.
        Case {
            name: "new_file_first_time_import",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_USER)],
            texts: vec![(py_gen, PY_GEN), ("newcomer.py", PY_MSG_REF)],
            old_edges: vec![edge(
                "oldconsumer.py",
                py_gen,
                "api.user_pb2",
                &[],
            )],
            new_edges: vec![gap_edge("newcomer.py", "api.user_pb2.User")],
            expected: vec![ExpBreak {
                element: "api.User",
                change: SchemaChange::Removed,
                proven: vec![("newcomer.py", "import")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 47. Generated code absent from the tree: Tier A is
        // inapplicable, said out loud — never a "0 consumers"
        // claim for a tree the tool could not see.
        Case {
            name: "generated_absent",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_consumer, PY_FROM_KWARGS)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec!["Tier A is inapplicable"],
        },
        // 48. Ambiguous Go struct name: two packages declare
        // `Shared`, the generated file sits in a third directory
        // with no directory signal — the mapping links nothing,
        // and the report says so instead of guessing a package.
        Case {
            name: "ambiguous_go_struct",
            old_protos: vec![
                ("api/a.proto", OLD_AMBIG_A),
                ("api/b.proto", OLD_AMBIG_B),
            ],
            new_protos: vec![
                ("api/a.proto", NEW_AMBIG_A),
                ("api/b.proto", OLD_AMBIG_B),
            ],
            texts: vec![
                ("gen/shared.pb.go", GO_GEN_AMBIG),
                (go_consumer, GO_AMBIG_CONSUMER),
            ],
            old_edges: vec![edge(
                go_consumer,
                "gen/shared.pb.go",
                "example.com/shared",
                &[],
            )],
            new_edges: vec![edge(
                go_consumer,
                "gen/shared.pb.go",
                "example.com/shared",
                &[],
            )],
            expected: vec![ExpBreak {
                element: "a.Shared.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: Some("field number 1 is reserved in the new side"),
            }],
            expected_added: vec![],
            expected_notes: vec!["no mapping applied"],
        },
        // 49. Generated file by suffix alone: a `.pyi` stub with
        // the marker maps like a `.py` module.
        Case {
            name: "pyi_generated",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![
                ("api/user_pb2.pyi", PYI_GEN),
                (py_consumer, PY_FROM_KWARGS),
            ],
            old_edges: vec![
                edge(py_consumer, "api/user_pb2.pyi", "api.user_pb2", &[]),
                gap_edge(py_consumer, "api.user_pb2.User"),
            ],
            new_edges: vec![
                edge(py_consumer, "api/user_pb2.pyi", "api.user_pb2", &[]),
                gap_edge(py_consumer, "api.user_pb2.User"),
            ],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 50. Go message removed, generated code regenerated: the
        // struct declaration the Go mapping keys on is gone, so
        // the break is listed with no consumers AND a note saying
        // the join could not see them — the documented limit of
        // the struct-name mapping, stated rather than hidden.
        Case {
            name: "go_regenerated_removal",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_USER)],
            texts: vec![(go_gen, GO_GEN_NO_USER), (go_consumer, GO_MSG_REF)],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.User",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec!["could not be joined"],
        },
        // 51. Whole proto file removed: every element in it reads
        // as removed, each with its own consumers — here none,
        // with the unmapped note, because the file's generated
        // module is gone with it.
        Case {
            name: "whole_file_removed",
            old_protos: vec![(user, OLD_PROTO), ("api/old.proto", OLD_EXTRA_PROTO)],
            new_protos: vec![(user, OLD_PROTO)],
            texts: vec![(py_gen, PY_GEN)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![ExpBreak {
                element: "api.OldMsg",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec!["could not be joined"],
        },
        // 52. json_name-only change: not a break in v1. The wire
        // name moved but the source contract did not, and v1 has
        // no kind for it — documented, not silently folded into
        // another kind.
        Case {
            name: "json_name_only_no_break",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_JSON_ONLY)],
            texts: vec![(py_gen, PY_GEN)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 53. Wildcard import: binds no single name, so it is
        // never evidence — the namespace rule, applied to Python.
        Case {
            name: "wildcard_never_evidence",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_WILDCARD)],
            old_edges: vec![],
            new_edges: vec![],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 54. Same-directory bare Go type: a composite literal
        // naming the message without a package qualifier matches
        // only a generated module in the same directory.
        Case {
            name: "go_bare_type_same_dir",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(go_gen, GO_GEN), ("api/helper.go", GO_LITERAL_BARE)],
            old_edges: vec![edge(
                "api/helper.go",
                go_gen,
                "example.com/api",
                &[],
            )],
            new_edges: vec![edge(
                "api/helper.go",
                go_gen,
                "example.com/api",
                &[],
            )],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![("api/helper.go", "struct literal")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 55. Vendored well-known protos: indexed like any other
        // file, with a coverage note saying so.
        Case {
            name: "vendored_well_known",
            old_protos: vec![
                (user, OLD_PROTO),
                (
                    "google/protobuf/empty.proto",
                    "syntax = \"proto3\";\npackage google.protobuf;\nmessage Empty {\n  reserved 1;\n}\n",
                ),
            ],
            new_protos: vec![
                (user, NEW_REMOVE_NAME),
                (
                    "google/protobuf/empty.proto",
                    "syntax = \"proto3\";\npackage google.protobuf;\nmessage Empty {\n  reserved 1;\n}\n",
                ),
            ],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_KWARGS)],
            old_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            new_edges: py_from_edges(py_consumer, py_gen, "api.user_pb2"),
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec!["vendors well-known"],
        },
        // 0b. Field removed, Python plain-import constructor
        // kwargs: the module-prefix chain form, distinct from the
        // from-import binding form.
        Case {
            name: "py_field_removed_plain_kwargs",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_PLAIN_KWARGS)],
            old_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            new_edges: vec![edge(py_consumer, py_gen, "api.user_pb2", &[])],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "constructor keyword")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 56. Suffix-only generated file: a `.pb.go` file without
        // markers still maps, with the assumed-mapping note.
        Case {
            name: "go_suffix_only_assumed",
            old_protos: vec![(user, OLD_PROTO)],
            new_protos: vec![(user, NEW_REMOVE_NAME)],
            texts: vec![
                (
                    go_gen,
                    "package api\n\ntype User struct{}\ntype Color int32\ntype GetUserRequest struct{}\ntype UserServiceClient interface {\n\tGetUser()\n}\n",
                ),
                (go_consumer, GO_LITERAL),
            ],
            old_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            new_edges: vec![edge(go_consumer, go_gen, "example.com/api", &[])],
            expected: vec![ExpBreak {
                element: "api.User.name",
                change: SchemaChange::Removed,
                proven: vec![("consumer.go", "struct literal")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec!["mapping assumed"],
        },
        // 57. Method removed from a service whose method carries a
        // body of custom options. Regression: skip_balanced started
        // at depth zero after the caller consumed the opener, so
        // any method body — and any enum-value option list — failed
        // the whole parse as "unterminated". Found on a real repo,
        // not a fixture: metal-stack/api's wrongproject.proto.
        Case {
            name: "method_body_options_parse",
            old_protos: vec![(user, OLD_METHOD_BODY_OPTS)],
            new_protos: vec![(user, NEW_METHOD_BODY_REMOVED)],
            texts: vec![(py_grpc, PY_GRPC_GEN), (py_consumer, PY_SERVICE_CALL)],
            old_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            new_edges: vec![
                edge(py_consumer, py_grpc, "api.user_pb2_grpc", &[]),
                gap_edge(py_consumer, "api.user_pb2_grpc.UserServiceStub"),
            ],
            expected: vec![ExpBreak {
                element: "api.UserService.GetUser",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "method call")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
        // 58. Enum value removed where the value carries a custom
        // option list. Same skip_balanced regression, second call
        // site: enum-value options also consumed their opener
        // before calling it.
        Case {
            name: "enum_value_options_parse",
            old_protos: vec![(user, OLD_ENUM_VALUE_OPTS)],
            new_protos: vec![(user, NEW_ENUM_VALUE_OPTS_REMOVED)],
            texts: vec![(py_gen, PY_GEN), (py_consumer, PY_FROM_ENUM)],
            old_edges: vec![
                edge(py_consumer, py_gen, "api.user_pb2", &[]),
                gap_edge(py_consumer, "api.user_pb2.Color"),
            ],
            new_edges: vec![
                edge(py_consumer, py_gen, "api.user_pb2", &[]),
                gap_edge(py_consumer, "api.user_pb2.Color"),
            ],
            expected: vec![ExpBreak {
                element: "api.Color.RED",
                change: SchemaChange::Removed,
                proven: vec![("consumer.py", "enum constant")],
                named: vec![],
                downstream: vec![],
                note: None,
            }],
            expected_added: vec![],
            expected_notes: vec![],
        },
    ]
}

#[test]
fn corpus_is_biconditional() {
    let failures: Vec<String> = cases()
        .iter()
        .filter_map(|case| run(case).err())
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} corpus scenarios failed:\n{}",
        failures.len(),
        cases().len(),
        failures.join("\n---\n")
    );
}

#[test]
fn proto2_is_an_error_not_a_partial_report() {
    let result = build_schema_index(&[(
        "api/user.proto".to_owned(),
        "syntax = \"proto2\";\npackage api;\nmessage User {\n  required string name = 1;\n}\n"
            .to_owned(),
    )]);
    assert!(result.is_err(), "proto2 must error, got {result:?}");
}

#[test]
fn missing_syntax_is_an_error() {
    let result = build_schema_index(&[(
        "api/user.proto".to_owned(),
        "package api;\nmessage User {\n  string name = 1;\n}\n".to_owned(),
    )]);
    assert!(result.is_err(), "a file without syntax must error, got {result:?}");
}

#[test]
fn corpus_count_clears_the_bar() {
    // The design's bar is >= 40 scenarios; the count is pinned so
    // a later edit cannot silently shrink the corpus below it.
    assert!(cases().len() >= 40, "corpus has {}", cases().len());
}
