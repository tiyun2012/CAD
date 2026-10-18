//! A feature pattern's copies applied at once: an extrude's copies in one boolean and each
//! chamfer's copies in one blend (`pattern-bosses.json`: a plate, a rounded boss joined under
//! it, its root and tip chamfered, and a 3 × 1 pattern of the three).

use std::cell::Cell;

use crate::*;

thread_local! {
    /// Patterns' chamfers and extrudes whose copies were applied at once on this thread.
    static AT_ONCE: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

pub(super) fn count(chamfers: usize, extrusions: usize) {
    AT_ONCE.with(|c| {
        let (a, b) = c.get();
        c.set((a + chamfers, b + extrusions));
    });
}

#[test]
fn a_patterns_copies_are_applied_at_once() {
    let doc = Document::from_json(include_str!("eval_pattern_tests/pattern-bosses.json")).unwrap();
    AT_ONCE.with(|c| c.set((0, 0)));
    let mut m = Model::new();
    m.evaluate(&doc);
    assert!(m.results.iter().all(|r| r.error.is_none()), "{:?}", m.results.iter().map(|r| &r.error).collect::<Vec<_>>());
    assert_eq!(AT_ONCE.with(Cell::get), (2, 1), "both chamfers' copies at once, the extrude's copies in one join");
}
