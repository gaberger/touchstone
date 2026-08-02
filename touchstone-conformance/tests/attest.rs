//! Attestation drills — is a `verified` claim actually true?
//!
//! `verified[].by` starting with `human:` promotes a concept above machine-generated text in
//! every ranking decision. For the whole life of this project that promotion rested on text a
//! human typed, checked by nothing: no CI, no signature, and `export` carried the claim to a
//! stranger with no way to test it. On a public repository that inverts — anyone forks the
//! bundle, adds a `human:` line, and the tier that "separates a curated brain from a pile of
//! plausible text" becomes decoration.
//!
//! These drills hold the four cases that matter apart, because collapsing them is how a
//! verifier ends up reporting "fine" for a bundle that has been edited since it was signed.

use touchstone_conformance::*;
use std::fs;
use std::path::Path;
use std::process::Command;

/// A bundle with one `human:`-claiming concept and a freshly generated signing key.
fn claim_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::TempDir::new().expect("temp");
    let root = tmp.path().join("b");
    fs::create_dir_all(root.join("notes")).unwrap();
    fs::write(
        root.join("notes/claim.md"),
        "---\ntype: Note\ntitle: \"Checked by a person\"\nverified:\n  - by: human:gary\n    at: 2026-08-02T00:00:00Z\n---\n\nBody.\n",
    )
    .unwrap();
    let key = tmp.path().join("key");
    let ok = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-f", key.to_str().unwrap(), "-N", "", "-C", "human:gary"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "ssh-keygen unavailable -- these drills need it to sign anything");
    (tmp, root)
}

fn run(root: &Path, args: &[&str]) -> (i32, String) {
    let out = Command::new(touchstone_bin())
        .arg("--bundle")
        .arg(root)
        .args(args)
        .output()
        .expect("spawn touchstone");
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
}

fn trust_key(tmp: &Path, root: &Path) {
    let pubkey = fs::read_to_string(tmp.join("key.pub")).unwrap();
    fs::create_dir_all(root.join("attest")).unwrap();
    fs::write(root.join("attest/allowed_signers"), format!("human:gary {pubkey}")).unwrap();
}

/// A1: an unsigned claim must not pass. This is the state the whole project shipped in.
#[test]
fn a1_an_unbacked_claim_fails_verification() {
    let (_tmp, root) = claim_bundle();
    let (code, out) = run(&root, &["verify"]);
    assert_eq!(code, 1, "an unbacked `human:` claim must fail:\n{out}");
    assert!(out.contains("UNBACKED"), "expected UNBACKED, got:\n{out}");
}

/// A2: a signature by a listed key over the current bytes passes.
#[test]
fn a2_a_valid_signature_backs_the_claim() {
    let (tmp, root) = claim_bundle();
    let key = tmp.path().join("key");
    let (code, _) = run(&root, &["attest", "notes/claim.md", "--key", key.to_str().unwrap()]);
    assert_eq!(code, 0, "attest should succeed");
    trust_key(tmp.path(), &root);

    let (code, out) = run(&root, &["verify"]);
    assert_eq!(code, 0, "a validly signed claim must verify:\n{out}");
    assert!(out.contains("1 of 1"), "expected 1 of 1 backed, got:\n{out}");
}

/// A3: editing a concept after signing must invalidate the attestation.
///
/// The most important of the four. The claim is real and the signature is real — and the bytes
/// are not the ones anybody verified. A verifier that keyed on the path rather than the content
/// would report this as fine.
#[test]
fn a3_editing_after_signing_makes_the_attestation_stale() {
    let (tmp, root) = claim_bundle();
    let key = tmp.path().join("key");
    run(&root, &["attest", "notes/claim.md", "--key", key.to_str().unwrap()]);
    trust_key(tmp.path(), &root);
    assert_eq!(run(&root, &["verify"]).0, 0, "should verify before the edit");

    let p = root.join("notes/claim.md");
    let mut text = fs::read_to_string(&p).unwrap();
    text.push_str("\nAn edit nobody verified.\n");
    fs::write(&p, text).unwrap();

    let (code, out) = run(&root, &["verify"]);
    assert_eq!(code, 1, "an edited concept must not keep its attestation:\n{out}");
    assert!(out.contains("STALE"), "expected STALE, got:\n{out}");
}

/// A4: the forgery case, and the reason this exists on a public repo.
///
/// An attacker forks the bundle, adds their own `human:` claim, and signs it with a key they
/// control. It must fail — not because the signature is malformed, but because the bundle does
/// not list them.
#[test]
fn a4_a_signature_from_an_unlisted_key_is_rejected() {
    let (tmp, root) = claim_bundle();
    let key = tmp.path().join("key");
    run(&root, &["attest", "notes/claim.md", "--key", key.to_str().unwrap()]);
    trust_key(tmp.path(), &root);

    let evil = tmp.path().join("evil");
    Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-f", evil.to_str().unwrap(), "-N", "", "-C", "attacker"])
        .status()
        .unwrap();
    fs::write(
        root.join("notes/forged.md"),
        "---\ntype: Note\ntitle: \"Forged\"\nverified:\n  - by: human:gary\n    at: 2026-08-02T00:00:00Z\n---\n\nNot checked by anyone.\n",
    )
    .unwrap();
    run(&root, &["attest", "notes/forged.md", "--key", evil.to_str().unwrap()]);

    let (code, out) = run(&root, &["verify"]);
    assert_eq!(code, 1, "a signature from an unlisted key must be rejected:\n{out}");
    assert!(out.contains("BAD SIGNATURE"), "expected BAD SIGNATURE, got:\n{out}");
    assert!(out.contains("1 of 2"), "the genuine claim must still pass:\n{out}");
}

/// A5: attestations travel with the bundle.
///
/// A signature that does not survive `export` protects nobody, because the exported bundle is
/// precisely the artifact a stranger receives.
#[test]
fn a5_attestations_survive_export() {
    let (tmp, root) = claim_bundle();
    let key = tmp.path().join("key");
    run(&root, &["attest", "notes/claim.md", "--key", key.to_str().unwrap()]);
    trust_key(tmp.path(), &root);

    let out_dir = tmp.path().join("exported");
    run(&root, &["export", out_dir.to_str().unwrap(), "--force"]);

    for rel in ["attest/manifest.jsonl", "attest/allowed_signers"] {
        assert!(out_dir.join(rel).exists(), "{rel} did not survive export -- the claim is unverifiable downstream");
    }
    let (code, out) = run(&out_dir, &["verify"]);
    assert_eq!(code, 0, "the exported bundle must verify on its own:\n{out}");
}
