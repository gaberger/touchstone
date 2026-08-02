//! `touchstone search <query>` — structured prefilter, BM25, one graph hop, trust rank.
//!
//! Calls `BundleIndex::search` directly rather than through a use case, and that is deliberate:
//! the whole pipeline lives inside the query (the prefilter MUST be applied there — post-filtering
//! an approximate index destroys recall at shallow depth, ADR-2608010920). There is no
//! orchestration left to share, so a use-case wrapper would be ceremony rather than sharing. The
//! MCP surface calls the same port method; that is what parity requires.
//!
//! `touchstone search <query>` — structured prefilter → BM25 → expansion → trust rank.

use crate::args::SearchArgs;
use crate::store::{CliSearchFilter, CliStore, SearchVia, Trust};

pub fn run(args: &SearchArgs, store: &dyn CliStore) -> i32 {
    let filter = CliSearchFilter {
        text: args.query.clone(),
        concept_type: args.concept_type.clone(),
        tag: args.tag.clone(),
        status: args.status.clone(),
        trust: args.trust.as_deref().and_then(Trust::from_label),
        limit: args.limit,
        expand: !args.no_expand,
    };

    let hits = match store.search(&filter) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };

    if hits.is_empty() {
        println!("no results");
        return 1;
    }

    for h in &hits {
        let mark = match h.trust.label() {
            "human" => "*",
            "machine" => "~",
            _ => " ",
        };
        let via = if h.via == SearchVia::Direct { "" } else { "  (via link)" };
        println!("{mark} {}{via}", h.path);
        println!("    {}  [{}]", h.title, h.concept_type);
        if !h.description.is_empty() {
            println!("    {}", h.description);
        }
    }
    println!("\n* human-verified   ~ machine-generated");

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{BundleStats, CliSearchFilter, IndexRecord, SearchHit};
    use std::collections::{HashMap, HashSet};

    struct StubStore(Vec<SearchHit>);
    impl CliStore for StubStore {
        fn prev_digests(&self) -> HashMap<String, String> { HashMap::new() }
        fn upsert(&mut self, _: &IndexRecord, _: &HashSet<String>) -> Result<(), String> { Ok(()) }
        fn remove(&mut self, _: &str) -> Result<(), String> { Ok(()) }
        fn reresolve(&mut self) -> Result<(), String> { Ok(()) }
        fn commit(&mut self) -> Result<(), String> { Ok(()) }
        fn broken_link_count(&self) -> usize { 0 }
        fn search(&self, _: &CliSearchFilter) -> Result<Vec<SearchHit>, String> {
            Ok(self.0.clone())
        }
        fn stats(&self) -> BundleStats {
            BundleStats { total: 0, by_type: vec![], by_trust: vec![], by_status: vec![], link_count: 0, broken_link_count: 0 }
        }
        fn all_paths(&self) -> Vec<String> { vec![] }
    }

    fn make_args(query: &str) -> SearchArgs {
        SearchArgs {
            query: query.to_string(),
            concept_type: None,
            tag: None,
            status: None,
            trust: None,
            limit: 10,
            no_expand: false,
        }
    }

    #[test]
    fn empty_results_returns_1() {
        let store = StubStore(vec![]);
        assert_eq!(run(&make_args("anything"), &store), 1);
    }

    #[test]
    fn hit_returns_0() {
        let store = StubStore(vec![SearchHit {
            path: "notes/x.md".into(),
            title: "X".into(),
            description: "".into(),
            concept_type: "Note".into(),
            trust: Trust::Unknown,
            via: SearchVia::Direct,
        }]);
        assert_eq!(run(&make_args("x"), &store), 0);
    }
}
