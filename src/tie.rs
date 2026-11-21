//! The engine's rule for close calls. Wherever it picks the nearest, the
//! shortest or the cheapest of several candidates, two that are equal on
//! paper (a symmetric pocket, two parallel loops) come out a few bits apart
//! in floating point, and without a rule that noise picks the winner.
//!
//! The rule: a candidate within `TIE` of the best is tied with it, and the
//! first tied candidate in the order given wins. A length within `TIE` of a
//! limit is within the limit. Every caller says what its order is.

/// How close two measures must be to count as equal, in millimetres (or mm²
/// for an area). Far above float noise, far below the 0.001 the G-code prints.
pub const TIE: f64 = 1e-6;

/// The first candidate, in the order given, whose measure is within `TIE` of
/// the least. `None` for no candidates.
pub fn first_least<T>(candidates: impl IntoIterator<Item = T>, measure: impl Fn(&T) -> f64) -> Option<T> {
    let all: Vec<T> = candidates.into_iter().collect();
    let least = all.iter().map(&measure).fold(f64::INFINITY, f64::min);
    all.into_iter().find(|c| measure(c) <= least + TIE)
}

/// Of the candidates tied for the least by `measure`, the least by `then`;
/// tied on both, the first in the order given. For a choice that has a
/// reason to prefer one tied candidate, such as the one nearest the tool.
pub fn first_least_then<T>(candidates: impl IntoIterator<Item = T>, measure: impl Fn(&T) -> f64, then: impl Fn(&T) -> f64) -> Option<T> {
    let all: Vec<T> = candidates.into_iter().collect();
    let least = all.iter().map(&measure).fold(f64::INFINITY, f64::min);
    first_least(all.into_iter().filter(|c| measure(c) <= least + TIE), then)
}

/// The first candidate, in the order given, whose measure is within `TIE` of
/// the greatest. `None` for no candidates.
pub fn first_greatest<T>(candidates: impl IntoIterator<Item = T>, measure: impl Fn(&T) -> f64) -> Option<T> {
    first_least(candidates, |c| -measure(c))
}

/// The candidates greatest first, each place going to the first of those tied
/// for it in the order given.
pub fn greatest_first<T>(mut candidates: Vec<T>, measure: impl Fn(&T) -> f64) -> Vec<T> {
    let mut out = Vec::with_capacity(candidates.len());
    while let Some(k) = first_greatest(0..candidates.len(), |k| measure(&candidates[*k])) {
        out.push(candidates.remove(k));
    }
    out
}

/// Whether `a` is within `limit`, a length on the limit counting as within.
pub fn within(a: f64, limit: f64) -> bool {
    a <= limit + TIE
}

/// Whether `a` beats `b` by more than a tie: the one in hand keeps its place
/// against anything that is not clearly less.
pub fn clearly_less(a: f64, b: f64) -> bool {
    a < b - TIE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_of_two_tied_candidates_wins_whichever_bit_is_smaller() {
        let noise = 1e-13;
        assert_eq!(first_least([("a", 5.0 + noise), ("b", 5.0)], |c| c.1).unwrap().0, "a");
        assert_eq!(first_least([("a", 5.0), ("b", 5.0 + noise)], |c| c.1).unwrap().0, "a");
        assert_eq!(first_greatest([("a", 5.0), ("b", 5.0 + noise)], |c| c.1).unwrap().0, "a");
    }

    #[test]
    fn a_clearly_better_candidate_wins_from_any_place() {
        assert_eq!(first_least([("a", 5.0), ("b", 4.9), ("c", 4.9)], |c| c.1).unwrap().0, "b");
        assert_eq!(first_greatest([("a", 5.0), ("b", 5.1)], |c| c.1).unwrap().0, "b");
        assert!(first_least(Vec::<(&str, f64)>::new(), |c| c.1).is_none());
    }

    #[test]
    fn a_tie_is_measured_from_the_best_not_from_the_neighbour() {
        // Each is within a tie of the one before it, but only the last two
        // are within a tie of the least.
        let chain = [("a", 3.0 * 0.6e-6), ("b", 2.0 * 0.6e-6), ("c", 0.6e-6), ("d", 0.0)];
        assert_eq!(first_least(chain, |c| c.1).unwrap().0, "c");
    }

    #[test]
    fn among_tied_candidates_the_second_measure_chooses() {
        let c = [("a", 5.0, 9.0), ("b", 5.0 + 1e-13, 2.0), ("c", 4.0 + 2.0, 1.0), ("d", 5.0, 2.0)];
        assert_eq!(first_least_then(c, |c| c.1, |c| c.2).unwrap().0, "b");
    }

    #[test]
    fn an_ordering_keeps_tied_candidates_in_the_order_given() {
        let pieces = vec![("a", 2.0), ("b", 9.0 + 1e-13), ("c", 9.0), ("d", 9.0 + 2e-13)];
        let names: Vec<&str> = greatest_first(pieces, |c| c.1).into_iter().map(|c| c.0).collect();
        assert_eq!(names, ["b", "c", "d", "a"]);
    }

    #[test]
    fn a_length_on_the_limit_is_within_it() {
        assert!(within(4.0 + 1e-13, 4.0));
        assert!(!within(4.001, 4.0));
        assert!(!clearly_less(5.0 - 1e-13, 5.0));
        assert!(clearly_less(4.9, 5.0));
    }
}
