use std::fs;

fn workflow_job<'a>(workflow: &'a str, name: &str, next: &str) -> &'a str {
    workflow
        .split_once(&format!("\n  {name}:"))
        .unwrap_or_else(|| panic!("missing {name} job"))
        .1
        .split_once(&format!("\n  {next}:"))
        .unwrap_or_else(|| panic!("missing {next} job after {name}"))
        .0
}

#[test]
fn crate_publish_waits_for_every_platform_build() {
    let workflow = fs::read_to_string(".github/workflows/release.yml").expect("release workflow");
    let publish = workflow_job(&workflow, "publish", "build");
    let release = workflow_job(&workflow, "release", "homebrew");
    let homebrew = workflow
        .split_once("\n  homebrew:")
        .expect("homebrew job")
        .1;

    assert!(publish.contains("needs: [preflight, build]"));
    assert!(release.contains("needs: [build, publish]"));
    assert!(homebrew.contains("needs: release"));
}

#[test]
fn crates_io_auth_action_is_pinned_to_the_reviewed_commit() {
    let workflow = fs::read_to_string(".github/workflows/release.yml").expect("release workflow");
    assert!(workflow.contains(
        "uses: rust-lang/crates-io-auth-action@c6f97d42243bad5fab37ca0427f495c86d5b1a18"
    ));
}

#[test]
fn public_docs_describe_the_full_registry_without_exact_setup_promises() {
    let readme = fs::read_to_string("README.md").expect("README");
    let privacy = fs::read_to_string("docs/PRIVACY.md").expect("privacy documentation");
    let changelog = fs::read_to_string("CHANGELOG.md").expect("changelog");
    let doctor = fs::read_to_string("src/doctor_cmd.rs").expect("doctor implementation");

    assert!(readme.contains("29 AI coding-agent data sources"));
    assert!(readme.contains("DeepSeek Harness"));
    assert!(readme.contains("Unsloth Studio"));
    assert!(!readme.contains("the exact setup step for anything missing"));
    assert!(privacy.contains("29 registered sources"));
    assert!(privacy.contains("DeepSeek Harness"));
    assert!(changelog.contains("all 29 registered sources"));
    assert!(doctor.contains("registered source diagnostics"));
}
