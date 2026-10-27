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
#[cfg(unix)]
fn install_force_refuses_a_symlinked_skill_file() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    assert!(
        sephera(&["skill", "install", "--path", path])
            .status
            .success()
    );
    let skill = dir
        .path()
        .join(".claude")
        .join("skills")
        .join("sephera-change-safety");
    let victim = dir.path().join("victim.txt");
    std::fs::write(&victim, "do not touch\n").unwrap();
    let skill_file = skill.join("SKILL.md");
    std::fs::remove_file(&skill_file).unwrap();
    symlink(&victim, &skill_file).unwrap();
    let output = sephera(&["skill", "install", "--path", path, "--force"]);
    assert!(
        !output.status.success(),
        "install over a symlink must fail, got: {}",
        stdout(&output)
    );
    assert!(stderr(&output).contains("symlink"));
    assert_eq!(
        std::fs::read_to_string(&victim).unwrap(),
        "do not touch\n",
        "the link's target must be untouched"
    );
}

#[test]
#[cfg(unix)]
fn install_force_refuses_a_symlinked_skill_dir() {
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let skills = dir.path().join(".claude").join("skills");
    std::fs::create_dir_all(&skills).unwrap();
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    symlink(&elsewhere, skills.join("sephera-change-safety")).unwrap();
    let output = sephera(&["skill", "install", "--path", path, "--force"]);
    assert!(
        !output.status.success(),
        "install over a symlinked dir must fail"
    );
    assert!(stderr(&output).contains("symlink"));
    assert!(
        !elsewhere.join("SKILL.md").exists(),
        "nothing may be written through the link"
    );
}

#[test]
#[cfg(unix)]
fn regression_install_refuses_a_symlinked_skills_dir() {
    // The old symlink check stopped at the target, so a link at
    // `.claude/skills` itself passed it and the install wrote the skill
    // into the link's destination. A sweep run proved the escape with a
    // live binary before this test existed.
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let victim = dir.path().join("victim");
    std::fs::create_dir_all(&victim).unwrap();
    std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
    symlink(&victim, dir.path().join(".claude").join("skills")).unwrap();
    let output = sephera(&["skill", "install", "--path", path, "--force"]);
    assert!(
        !output.status.success(),
        "install through a symlinked skills dir must fail, got: {}",
        stdout(&output)
    );
    assert!(stderr(&output).contains("symlink"));
    assert!(
        !victim.join("sephera-change-safety").exists(),
        "nothing may be written through the link"
    );
}

#[test]
#[cfg(unix)]
fn regression_install_refuses_a_symlinked_claude_dir() {
    // Same escape one level higher: `.claude` itself as a symlink used
    // to pass the check and redirect the whole install.
    use std::os::unix::fs::symlink;
    let dir = tempdir().unwrap();
    let path = dir.path().to_str().unwrap();
    let victim = dir.path().join("victim");
    std::fs::create_dir_all(&victim).unwrap();
    symlink(&victim, dir.path().join(".claude")).unwrap();
    let output = sephera(&["skill", "install", "--path", path, "--force"]);
    assert!(
        !output.status.success(),
        "install through a symlinked .claude dir must fail, got: {}",
        stdout(&output)
    );
    assert!(stderr(&output).contains("symlink"));
    assert!(
        !victim.join("skills").exists(),
        "nothing may be written through the link"
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
