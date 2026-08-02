//! `touchstone stats` — bundle summary (concepts by type, trust, status; link counts).

use crate::store::CliStore;
use std::path::Path;

pub fn run(bundle: &Path, store: &dyn CliStore) -> i32 {
    let s = store.stats();
    println!("bundle: {}", bundle.display());
    println!("concepts: {}", s.total);

    println!("\nby type:");
    for (t, n) in &s.by_type {
        let label = if t.is_empty() { "(none)" } else { t.as_str() };
        println!("  {:>5}  {label}", n);
    }

    println!("\nby trust tier:");
    for (t, n) in &s.by_trust {
        println!("  {:>5}  {t}", n);
    }

    println!("\nby status:");
    for (t, n) in &s.by_status {
        println!("  {:>5}  {t}", n);
    }

    let broken = s.broken_link_count;
    println!("\nlinks: {} ({broken} broken -- legal per spec)", s.link_count);

    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{BundleStats, CliSearchFilter, IndexRecord, SearchHit};
    use std::collections::{HashMap, HashSet};

    struct S;
    impl CliStore for S {
        fn prev_digests(&self) -> HashMap<String, String> { HashMap::new() }
        fn upsert(&mut self, _: &IndexRecord, _: &HashSet<String>) -> Result<(), String> { Ok(()) }
        fn remove(&mut self, _: &str) -> Result<(), String> { Ok(()) }
        fn reresolve(&mut self) -> Result<(), String> { Ok(()) }
        fn commit(&mut self) -> Result<(), String> { Ok(()) }
        fn broken_link_count(&self) -> usize { 2 }
        fn search(&self, _: &CliSearchFilter) -> Result<Vec<SearchHit>, String> { Ok(vec![]) }
        fn stats(&self) -> BundleStats {
            BundleStats {
                total: 5,
                by_type: vec![("Note".into(), 3), ("Decision".into(), 2)],
                by_trust: vec![("unattributed".into(), 5)],
                by_status: vec![("stable".into(), 4), ("draft".into(), 1)],
                link_count: 10,
                broken_link_count: 2,
            }
        }
        fn all_paths(&self) -> Vec<String> { vec![] }
    }

    #[test]
    fn stats_returns_0() {
        assert_eq!(run(Path::new("/tmp/bundle"), &S), 0);
    }
}
