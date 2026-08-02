//! `touchstone attest <path>` — sign a concept's `verified` claim.
//!
//! **Deliberately absent from the MCP surface.** Every other capability exists on both
//! adapters, and the parity test enforces that; this one is the exception, listed in
//! `CLI_ONLY` with this reason:
//!
//! An agent may never write `verified: {by: human:...}`. That is the invariant separating a
//! curated brain from a pile of plausible text. Exposing signing over MCP would hand a model
//! the one capability the whole design forbids it — and it would do so at the exact moment the
//! signature started to mean something, which is the worst possible time.
//!
//! Signing requires a private key the human holds. That is not an implementation detail; it is
//! the mechanism.

use crate::args::AttestArgs;
use touchstone_ports::{Attestation, Clock, ConceptParser, ConceptRepository, ConceptSink, RawStore, VersionControl};
use touchstone_usecases::{manifest_line, MANIFEST_REL, SIGNERS_REL};

pub fn run<F, P>(args: &AttestArgs, files: &F, parser: &P, vc: &dyn VersionControl, clock: &dyn Clock) -> i32
where
    F: ConceptRepository + RawStore + ConceptSink,
    P: ConceptParser,
{
    let path = args.path.trim_start_matches("./");
    let Some(raw) = files.raw_bytes(path) else {
        eprintln!("no such concept: {path}");
        return 1;
    };
    let parsed = parser.parse(path, &raw);

    // The claim must already be in the file. `attest` signs an existing statement; it does not
    // author one, because a command that both makes and signs a claim is just a claim.
    let humans: Vec<String> = parsed
        .verified_entries
        .iter()
        .filter_map(|e| e.by.clone())
        .filter(|by| by.starts_with("human:"))
        .collect();

    // A concept may name several human verifiers. Picking one silently means signing someone
    // else's claim with your key -- which verification then rejects, correctly, but only after
    // the manifest already carries a false entry. Refuse instead, and make the caller say who.
    let signer = match (&args.signer, humans.len()) {
        (Some(s), _) if humans.contains(s) => Some(s.clone()),
        (Some(s), _) => {
            eprintln!("{path} makes no `verified` claim by {s}.\nIt names: {}", humans.join(", "));
            return 1;
        }
        (None, 1) => humans.first().cloned(),
        (None, n) if n > 1 => {
            eprintln!(
                "{path} names {n} human verifiers: {}.\n\
                 Pass --as <signer> to say which claim you are signing. Signing another \
                 person's claim with your key produces an attestation that will be rejected.",
                humans.join(", ")
            );
            return 1;
        }
        _ => None,
    };
    let Some(signer) = signer else {
        eprintln!(
            "{path} makes no `human:` verification claim.\n\
             Add `verified: [{{by: human:you, at: ...}}]` to the concept first -- attest signs \
             a claim you have made, it does not make one for you."
        );
        return 1;
    };

    let digest = touchstone_ports::fnv64(&raw);
    let at = clock.now_iso8601();
    let payload = Attestation::payload(path, &digest, &signer, &at);

    let signature = match vc.sign(&payload, &args.key) {
        Ok(sig) => sig,
        Err(e) => {
            eprintln!("could not sign: {e}");
            return 1;
        }
    };

    let entry = Attestation { path: path.to_string(), digest, signer: signer.clone(), at, signature };
    let existing = files.raw_bytes(MANIFEST_REL).unwrap_or_default();
    let mut out = String::from_utf8_lossy(&existing).to_string();
    // Append-only: an attestation history is evidence, and rewriting it would discard the
    // record of what was verified when.
    out.push_str(&manifest_line(&entry));
    if let Err(e) = files.write(MANIFEST_REL, out.as_bytes()) {
        eprintln!("cannot write {MANIFEST_REL}: {e}");
        return 1;
    }

    // Self-check: if the bundle already declares its signers, confirm the signature we just
    // made actually verifies. Otherwise the first sign of "you signed as someone you are not"
    // is a failing `verify` later, with a manifest entry already written.
    if let Some(allowed) = files.raw_bytes(SIGNERS_REL) {
        let allowed = String::from_utf8_lossy(&allowed).to_string();
        let payload = Attestation::payload(&entry.path, &entry.digest, &entry.signer, &entry.at);
        if !matches!(vc.verify(&payload, &entry.signature, &entry.signer, &allowed), Ok(true)) {
            eprintln!(
                "warning: this attestation does not verify against {SIGNERS_REL}.\n\
                 The key you signed with is probably not the one listed for {signer}. The entry \
                 was written, but `touchstone verify` will reject it."
            );
        }
    }

    println!("attested {path} as {signer}");
    if files.raw_bytes(SIGNERS_REL).is_none() {
        println!(
            "\nnote: {SIGNERS_REL} does not exist, so nobody -- including you -- can verify this yet.\n\
             Add your public key in ssh allowed_signers format:\n\
             \n  {signer} $(cat ~/.ssh/id_ed25519.pub)\n"
        );
    }
    0
}
