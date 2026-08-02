//! `touchstone capture "<text>"` — thought to saved concept, in one command.
//!
//! **This is the command A3 is actually testing.** PROTOTYPE.md puts the bar at ~20 seconds
//! from "I want to record this" to "it is saved", and observes that capture dies above it. The
//! existing path was: open a terminal, `touchstone new Note "title"`, open the file, write the
//! body, save, remember to index. That is not twenty seconds, and measuring adoption against it
//! would have measured the friction rather than the idea.
//!
//! So: no editor, no title required, no frontmatter to hand-write. Give it a sentence.
//!
//! The title is the first line or the first sentence, and it is deliberately not clever. A
//! wrong-but-obvious title you can fix later beats a prompt that stops you mid-thought — the
//! whole point is that nothing interrupts the thing you were trying to record.

use crate::args::CaptureArgs;
use touchstone_ports::{Clock, ConceptParser, ConceptSink, EventLog, RawStore};
use touchstone_usecases::{capture_concept_logged, CaptureRequest};

/// First sentence or first line, whichever is shorter, capped so a filename stays sane.
fn title_from(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or(text).trim();
    let candidate = first_line
        .split_once(". ")
        .map(|(head, _)| head)
        .unwrap_or(first_line)
        .trim_end_matches('.');

    let mut out: String = candidate.chars().take(72).collect();
    if candidate.chars().count() > 72 {
        // Cut at a word boundary rather than mid-word.
        if let Some(i) = out.rfind(' ') {
            out.truncate(i);
        }
    }
    if out.trim().is_empty() { "Untitled capture".to_string() } else { out.trim().to_string() }
}

pub fn run<P, W>(args: &CaptureArgs, parser: &P, sink: &W, clock: &dyn Clock) -> i32
where
    P: ConceptParser,
    W: ConceptSink + RawStore + EventLog,
{
    let text = args.text.join(" ");
    if text.trim().is_empty() {
        eprintln!("nothing to capture.\n\n  touchstone capture \"the thing you were thinking\"");
        return 1;
    }

    let req = CaptureRequest {
        concept_type: args.concept_type.clone(),
        title: args.title.clone().unwrap_or_else(|| title_from(&text)),
        description: String::new(),
        tags: args.tags.clone(),
        subdir: args.dir.clone(),
        // Captured by a human at a terminal. `generated` stays absent, so the tier is
        // `unattributed` rather than `machine` -- this is your thought, not an agent's, and it
        // is still not `human` because nobody has verified it.
        generated_by: None,
    };

    match capture_concept_logged(&req, parser, sink, clock, sink, "cli") {
        Ok(path) => {
            // Append the body after scaffolding, so the frontmatter still comes from the YAML
            // dumper rather than string concatenation (FINDINGS E3d).
            if let Some(raw) = sink.raw_bytes(&path) {
                let mut doc = String::from_utf8_lossy(&raw).to_string();
                if !doc.ends_with('\n') {
                    doc.push('\n');
                }
                doc.push_str(&text);
                doc.push('\n');
                if let Err(e) = sink.write(&path, doc.as_bytes()) {
                    eprintln!("captured {path} but could not write the body: {e}");
                    return 1;
                }
            }
            println!("{path}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::title_from;

    #[test]
    fn a_sentence_becomes_its_first_clause() {
        assert_eq!(
            title_from("Columnar was the wrong answer. The bottleneck was writes."),
            "Columnar was the wrong answer"
        );
    }

    #[test]
    fn a_bare_fragment_is_used_whole() {
        assert_eq!(title_from("check the WAL growth"), "check the WAL growth");
    }

    #[test]
    fn a_long_thought_is_cut_at_a_word_boundary() {
        let t = title_from(&"word ".repeat(40));
        assert!(t.len() <= 72, "title too long for a filename: {t}");
        assert!(!t.ends_with("wor"), "cut mid-word: {t}");
    }

    /// Empty input must not produce an empty filename.
    #[test]
    fn whitespace_still_yields_a_title() {
        assert_eq!(title_from("   \n  "), "Untitled capture");
    }
}
