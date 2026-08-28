use std::path::{Path, PathBuf};
use std::process::Command;

use konstruktor_core::git;

/// A dev hub's checkouts, exercised against real git repositories.
///
/// Nothing here mocks git. The whole point of the module under test is what git actually
/// does with a clone that has one local branch and several remote ones — which is exactly
/// the shape `create_hub` leaves behind, and exactly where a naive `git checkout <name>`
/// gives a detached HEAD instead of a tracking branch.

fn git_in(at: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(at)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("konstruktor-checkouts-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temp dir");
    dir
}

/// An "origin" with `main` and `next`, and a clone of it — the shape `create_hub` writes.
fn a_clone(name: &str) -> (PathBuf, PathBuf) {
    let root = scratch(name);
    let origin = root.join("origin");
    std::fs::create_dir_all(&origin).expect("the origin folder");

    git_in(&origin, &["init", "--initial-branch=main", "--quiet"]);
    git_in(&origin, &["config", "user.email", "test@example.org"]);
    git_in(&origin, &["config", "user.name", "Test"]);
    std::fs::write(origin.join("run.sh"), "echo main\n").expect("a file to commit");
    git_in(&origin, &["add", "."]);
    git_in(&origin, &["commit", "--quiet", "-m", "main"]);

    git_in(&origin, &["checkout", "--quiet", "-b", "next"]);
    std::fs::write(origin.join("run.sh"), "echo next\n").expect("a file to commit");
    git_in(&origin, &["commit", "--quiet", "-am", "next"]);
    git_in(&origin, &["checkout", "--quiet", "main"]);

    let checkout = root.join("mounts").join("rekuest");
    std::fs::create_dir_all(checkout.parent().expect("mounts")).expect("the mounts folder");
    let cloned = git::clone_service("rekuest", &origin.to_string_lossy(), None, &checkout)
        .expect("the clone succeeds");
    assert!(cloned, "a fresh folder is cloned into");

    (root, checkout)
}

#[test]
fn reads_the_branch_a_checkout_is_on() {
    let (_root, checkout) = a_clone("reads");

    let state = git::read_checkout("rekuest", "origin", &checkout);
    assert_eq!(state.error, None);
    assert_eq!(state.branch.as_deref(), Some("main"));
    assert!(!state.detached);
    assert!(!state.dirty);
    assert!(state.head.is_some());
}

#[test]
fn offers_branches_that_exist_only_on_the_remote() {
    let (_root, checkout) = a_clone("offers");

    // `next` has no local ref in a fresh clone; it is still somewhere the user can go.
    let names = git::branches(&checkout).expect("the branches list");
    assert_eq!(names, vec!["main".to_string(), "next".to_string()]);
}

#[test]
fn switching_to_a_remote_only_branch_creates_it_tracking_origin() {
    let (_root, checkout) = a_clone("switch");

    git::switch_branch("rekuest", &checkout, "next").expect("the switch succeeds");

    let state = git::read_checkout("rekuest", "origin", &checkout);
    // A bare `git checkout next` is what gives a detached HEAD here — the branch must be
    // a real local branch, and it must track.
    assert!(!state.detached, "a detached HEAD is not a switch");
    assert_eq!(state.branch.as_deref(), Some("next"));
    assert_eq!(
        std::fs::read_to_string(checkout.join("run.sh")).expect("the file"),
        "echo next\n",
        "the working tree holds the other branch's content"
    );

    // And back again, which now takes the local-branch path instead.
    git::switch_branch("rekuest", &checkout, "main").expect("switching back succeeds");
    assert_eq!(
        git::read_checkout("rekuest", "origin", &checkout).branch.as_deref(),
        Some("main")
    );
}

#[test]
fn refuses_to_switch_over_uncommitted_work() {
    let (_root, checkout) = a_clone("dirty");
    std::fs::write(checkout.join("run.sh"), "echo mine\n").expect("an edit");

    let state = git::read_checkout("rekuest", "origin", &checkout);
    assert!(state.dirty);

    let error = git::switch_branch("rekuest", &checkout, "next")
        .expect_err("uncommitted work is not thrown away");
    assert!(
        error.to_string().contains("uncommitted"),
        "the refusal says why: {error}"
    );
    // And the edit is still there.
    assert_eq!(
        std::fs::read_to_string(checkout.join("run.sh")).expect("the file"),
        "echo mine\n"
    );
}

#[test]
fn untracked_files_do_not_count_as_uncommitted_work() {
    let (_root, checkout) = a_clone("untracked");
    // What a container writes into the mount. Counting these would refuse every switch
    // on any dev hub that had ever been started.
    std::fs::create_dir_all(checkout.join("__pycache__")).expect("a cache dir");
    std::fs::write(checkout.join("__pycache__/x.pyc"), "").expect("a cache file");

    assert!(!git::read_checkout("rekuest", "origin", &checkout).dirty);
    git::switch_branch("rekuest", &checkout, "next").expect("the switch still succeeds");
}

#[test]
fn a_missing_or_unversioned_checkout_reports_rather_than_fails() {
    let root = scratch("missing");

    let absent = git::read_checkout("rekuest", "origin", &root.join("mounts/rekuest"));
    assert!(absent.error.is_some(), "a missing checkout says so");
    assert_eq!(absent.branch, None);

    let plain = root.join("plain");
    std::fs::create_dir_all(&plain).expect("a plain folder");
    let unversioned = git::read_checkout("rekuest", "origin", &plain);
    assert!(
        unversioned.error.unwrap().contains("not a git repository"),
        "a folder that is not a repository says so"
    );
}

#[test]
fn a_branch_that_exists_nowhere_is_refused_by_name() {
    let (_root, checkout) = a_clone("nosuch");

    let error = git::switch_branch("rekuest", &checkout, "no-such-branch")
        .expect_err("an unknown branch is not created");
    assert!(error.to_string().contains("no-such-branch"), "{error}");
    assert_eq!(
        git::read_checkout("rekuest", "origin", &checkout).branch.as_deref(),
        Some("main")
    );
}
