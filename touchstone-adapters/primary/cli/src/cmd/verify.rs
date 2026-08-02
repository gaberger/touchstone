//! `touchstone verify` — are this bundle's `verified` claims true?
//!
//! Read-only. Checks every concept claiming `human:` verification against the signed manifest,
//! and reports the ones that are not backed by a valid signature over their current bytes.
//!
//! This is the command that makes the trust invariant checkable by someone who did not write
//! the bundle — which is the only situation where it matters, since the format exists to be
//! shared.

use touchstone_ports::{ConceptParser, ConceptRepository, RawStore, VersionControl};
use touchstone_usecases::{verify_bundle, AttestStatus};

pub fn run<F, P>(files: &F, parser: &P, vc: &dyn VersionControl) -> i32
where
    F: ConceptRepository + RawStore,
    P: ConceptParser,
{
    let report = verify_bundle(files, parser, vc);

    for (path, status) in &report.problems {
        let (label, why) = match status {
            AttestStatus::Unbacked => ("UNBACKED", "claims human verification with no attestation"),
            AttestStatus::Stale => ("STALE", "signed, but the concept changed since -- the verified bytes are not these bytes"),
            AttestStatus::BadSignature => ("BAD SIGNATURE", "signature invalid, or the signer is not in attest/allowed_signers"),
            AttestStatus::NotClaimed | AttestStatus::Backed => continue,
        };
        println!("{label}: {path}\n  {why}");
    }

    if report.checked == 0 {
        println!("no `human:` claims in this bundle -- nothing to verify");
        return 0;
    }
    println!("\n{} of {} human claims backed", report.backed, report.checked);
    if report.is_clean() {
        0
    } else {
        // Non-zero so CI can gate on it. An unbacked `verified` outranks machine-generated
        // text in every search, so shipping one is worse than shipping none.
        1
    }
}
