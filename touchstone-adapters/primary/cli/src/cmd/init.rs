//! `touchstone init` — create a bundle you can actually start using.
//!
//! The first step that did not exist. Every other command assumed a bundle already there, and
//! the shortest honest path from "cloned the repo" to "an agent is using this" ran through
//! reading the source.
//!
//! Deliberately does NOT write your MCP client config. It prints the snippet instead: writing
//! into someone's `.mcp.json` or `~/.claude.json` from a CLI is presumptuous, and the one time
//! it guesses the wrong file it has edited a config it does not own.

use touchstone_ports::{ConceptRepository, ConceptSink};
use touchstone_usecases::{RAW_DIR, SIGNERS_REL};

pub fn run<F>(bundle: &std::path::Path, files: &F) -> i32
where
    F: ConceptRepository + ConceptSink,
{
    // `.gitkeep` because an empty directory does not survive git, and a bundle whose raw/ layer
    // vanished on clone would fail its first ingest for a reason nobody would guess.
    let seeds: [(&str, &str); 3] = [
        (
            "index.md",
            "<!-- replaced by `touchstone index` -->\n# Bundle\n\n_No concepts yet._\n",
        ),
        (&format!("{RAW_DIR}/.gitkeep"), ""),
        ("attest/.gitkeep", ""),
    ];

    let mut created = 0;
    for (rel, body) in seeds {
        if files.exists(rel) {
            continue;
        }
        if let Err(e) = files.write(rel, body.as_bytes()) {
            eprintln!("cannot write {rel}: {e}");
            return 1;
        }
        created += 1;
    }

    let existing = files.paths().len();
    println!("bundle ready at {}", bundle.display());
    if existing > 0 {
        println!("  {existing} concepts already here");
    }
    if created == 0 {
        println!("  (already initialised)");
    }

    println!(
        "\nNext:\n\
         \x20 touchstone --bundle {b} ingest <file>...   put source material in raw/\n\
         \x20 touchstone --bundle {b} unprocessed        what nothing cites yet\n\
         \x20 touchstone --bundle {b} index              build the derived plane\n",
        b = bundle.display()
    );

    if !files.exists(SIGNERS_REL) {
        println!(
            "To make `verified` claims checkable, declare who may sign:\n\
             \x20 printf 'human:you %s\\n' \"$(cat ~/.ssh/id_ed25519.pub)\" > {}/{SIGNERS_REL}\n",
            bundle.display()
        );
    }

    println!(
        "For an agent, add to .mcp.json -- printed rather than written, because this is your \
         config and not mine:\n\n\
         {{\n  \"mcpServers\": {{\n    \"touchstone\": {{\n      \"command\": \"touchstone\",\n\
         \x20     \"args\": [\"--bundle\", \"{}\", \"mcp\"]\n    }}\n  }}\n}}",
        bundle.display()
    );
    0
}
