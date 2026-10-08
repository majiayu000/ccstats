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

    assert!(publish.contains("needs: [preflight, build, desktop]"));
    assert!(release.contains("needs: [build, desktop, publish]"));
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

#[test]
fn changelog_names_every_source_added_across_the_nine_provider_batches() {
    let changelog = fs::read_to_string("CHANGELOG.md").expect("changelog");
    let release = changelog
        .split_once("## [0.5.2]")
        .expect("0.5.2 release")
        .1
        .split_once("## [0.5.0]")
        .expect("0.5.0 release follows 0.5.2")
        .0;

    assert!(release.contains("5 to 29 sources across nine provider batches"));
    for batch in [
        "Gemini CLI, Amp, Qwen Code, Cline, Roo Code, and Kilo Code",
        "OpenCode, MiMo Code, and Kilo CLI",
        "Pi, Senpi, and Kimchi",
        "Gajae Code, Prime Agent, and Oh My Pi",
        "GitHub Copilot CLI and Goose",
        "OpenClaw, Xum, and Hermes Agent",
        "Reasonix and Vercel Fx",
        "Unsloth Studio",
        "DeepSeek Harness",
    ] {
        assert!(release.contains(batch), "0.5.2 does not name batch {batch}");
    }
}

#[test]
fn crate_docs_name_the_final_two_registered_sources() {
    let crate_docs = fs::read_to_string("src/lib.rs").expect("crate docs");

    assert!(crate_docs.contains("Unsloth Studio"));
    assert!(crate_docs.contains("DeepSeek Harness"));
}
