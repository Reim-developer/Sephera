//! End-to-end behaviour of `sephera skill`.
//!
//! These drive the real binary against temporary directories, because
//! the command's claim is about files landing in the right place
//! with the right contents — and a refusal that must not clobber a
//! skill the user tuned by hand. Nothing here needs a git repo:
//! installing a skill analyses no tree.

use std::process::{Command, Output};

use tempfile::tempdir;

fn sephera(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sephera"))
        .args(args)
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The four files an install must produce, relative to the skill dir.
const SKILL_FILES: [&str; 4] = [
    "SKILL.md",
    "references/commands.md",
    "references/tools.md",
    "references/thresholds.md",
];

#[test]
fn print_reports_the_install_path_and_summary_without_writing() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let output = sephera(&["skill", "--print", "--path", path]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(
        text.contains(".claude/skills/sephera-change-safety"),
        "print must name the install path, got: {text}"
    );
    assert!(
        text.contains("sephera-change-safety:"),
        "print must carry the one-line summary, got: {text}"
    );
    assert!(
        !dir.path().join(".claude").exists(),
        "--print must not create anything"
    );
}

#[test]
fn bare_skill_prints_the_same_summary_as_print() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let output = sephera(&["skill", "--path", path]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains(".claude/skills/sephera-change-safety"));
}

#[test]
fn install_writes_the_four_skill_files() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let output = sephera(&["skill", "install", "--path", path]);
    assert!(output.status.success(), "{}", stderr(&output));
    let skill = dir
        .path()
        .join(".claude")
        .join("skills")
        .join("sephera-change-safety");
    for relative in SKILL_FILES {
        assert!(skill.join(relative).is_file(), "missing {relative}");
    }
    let main = std::fs::read_to_string(skill.join("SKILL.md")).unwrap();
    assert!(main.contains("name: sephera-change-safety"));
    assert!(main.contains("Before editing a file"));
}

#[test]
fn install_refuses_to_overwrite_without_force() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    assert!(
        sephera(&["skill", "install", "--path", path])
            .status
            .success()
    );
    let second = sephera(&["skill", "install", "--path", path]);
    assert!(
        !second.status.success(),
        "reinstall must fail without --force"
    );
    assert!(stderr(&second).contains("already exists"));
}

#[test]
fn install_force_overwrites_a_tuned_skill() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    assert!(
        sephera(&["skill", "install", "--path", path])
            .status
            .success()
    );
    let skill_file = dir
        .path()
        .join(".claude")
        .join("skills")
        .join("sephera-change-safety")
        .join("SKILL.md");
    std::fs::write(&skill_file, "tuned by hand\n").unwrap();
    let output = sephera(&["skill", "install", "--path", path, "--force"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let restored = std::fs::read_to_string(&skill_file).unwrap();
    assert!(restored.contains("name: sephera-change-safety"));
    assert!(!restored.contains("tuned by hand"));
}

#[test]
fn install_honours_a_relative_dir_override() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let output = sephera(&[
        "skill",
        "install",
        "--path",
        path,
        "--dir",
        ".agents/skills",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let skill = dir
        .path()
        .join(".agents")
        .join("skills")
        .join("sephera-change-safety");
    assert!(skill.join("SKILL.md").is_file());
    assert!(
        !dir.path().join(".claude").exists(),
        "an override must replace the default, not add to it"
    );
}

#[test]
fn print_and_install_together_are_an_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let output = sephera(&["skill", "--print", "install", "--path", path]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("cannot be combined"));
}
