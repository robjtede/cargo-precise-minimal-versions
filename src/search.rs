use clap::ValueEnum;
use serde::Serialize;

use crate::probe::Outcome;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Bisect,
    Linear,
    Descending,
}

pub struct Search {
    outcomes: Vec<Option<Outcome>>,
    start: usize,
    end: usize,
    bisect: bool,
    threshold: usize,
    descending: bool,
}

impl Search {
    pub fn new(count: usize, strategy: Strategy, threshold: usize) -> Self {
        Self {
            outcomes: vec![None; count],
            start: 0,
            end: count,
            bisect: strategy == Strategy::Bisect && count > threshold,
            threshold,
            descending: strategy == Strategy::Descending,
        }
    }

    pub fn next(&mut self) -> Option<usize> {
        if self.descending {
            return self.outcomes.iter().rposition(Option::is_none);
        }

        // The first eligible release can satisfy the requirement without a bisection.
        if self.outcomes.first().is_some_and(Option::is_none) {
            return Some(0);
        }

        if self.bisect && self.end - self.start > self.threshold {
            return Some(self.start + (self.end - self.start) / 2);
        }

        if let Some(offset) = self.outcomes[self.start..self.end]
            .iter()
            .position(Option::is_none)
        {
            return Some(self.start + offset);
        }

        // Without a passing bound, skipped releases can still contain a pass.
        if self.bisect && self.lowest_passing().is_none() {
            self.bisect = false;
            self.start = 0;
            self.end = self.outcomes.len();
            return self.next();
        }

        None
    }

    pub fn record(&mut self, index: usize, outcome: Outcome) {
        self.outcomes[index] = Some(outcome);

        match outcome {
            Outcome::Passed => self.end = index,
            Outcome::ApiFailure if self.bisect => self.start = index + 1,
            _ if self.bisect => {
                // An infrastructure or type error cannot discard older releases.
                self.bisect = false;
                self.start = 0;
                self.end = self.lowest_passing().unwrap_or(self.outcomes.len());
            }
            _ => {}
        }
    }

    pub fn lowest_passing(&self) -> Option<usize> {
        self.outcomes
            .iter()
            .position(|outcome| *outcome == Some(Outcome::Passed))
    }

    pub fn minimum_verified(&self) -> bool {
        self.lowest_passing().is_some_and(|index| {
            self.outcomes[..index]
                .iter()
                .all(|outcome| *outcome == Some(Outcome::ApiFailure))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(outcomes: &[Outcome], strategy: Strategy, threshold: usize) -> (Search, Vec<usize>) {
        let mut search = Search::new(outcomes.len(), strategy, threshold);
        let mut visited = Vec::new();

        while let Some(index) = search.next() {
            assert!(
                !visited.contains(&index),
                "candidate {index} was probed twice"
            );
            visited.push(index);
            search.record(index, outcomes[index]);
        }

        (search, visited)
    }

    #[test]
    fn large_monotonic_ranges_need_fewer_probes() {
        let mut outcomes = vec![Outcome::ApiFailure; 229];
        outcomes[59..].fill(Outcome::Passed);

        let (search, visited) = run(&outcomes, Strategy::Bisect, 10);

        assert_eq!(search.lowest_passing(), Some(59));
        assert!(visited.len() <= 16, "{} probes", visited.len());
        assert!(!search.minimum_verified());

        let (search, visited) = run(&outcomes, Strategy::Linear, 10);

        assert_eq!(visited.len(), 60);
        assert_eq!(search.lowest_passing(), Some(59));
        assert!(search.minimum_verified());
    }

    #[test]
    fn descending_search_checks_older_releases_after_a_failure() {
        let (search, visited) = run(
            &[
                Outcome::ApiFailure,
                Outcome::Passed,
                Outcome::ApiFailure,
                Outcome::Passed,
            ],
            Strategy::Descending,
            10,
        );

        assert_eq!(visited, [3, 2, 1, 0]);
        assert_eq!(search.lowest_passing(), Some(1));
        assert!(search.minimum_verified());
    }

    #[test]
    fn every_monotonic_boundary_is_found() {
        for threshold in [1, 10, 128] {
            for first_pass in 0..=128 {
                let mut outcomes = vec![Outcome::ApiFailure; 128];
                outcomes[first_pass..].fill(Outcome::Passed);

                let (search, _) = run(&outcomes, Strategy::Bisect, threshold);

                assert_eq!(
                    search.lowest_passing(),
                    (first_pass < 128).then_some(first_pass),
                    "first pass {first_pass}, threshold {threshold}"
                );
            }
        }
    }

    #[test]
    fn inconclusive_bisection_falls_back_and_reuses_probes() {
        for outcome in [
            Outcome::Inconclusive,
            Outcome::CandidateBuildFailed,
            Outcome::RequiresNewerRust,
        ] {
            let mut outcomes = vec![Outcome::ApiFailure; 32];
            outcomes[1] = Outcome::Passed;
            outcomes[16] = outcome;

            let (search, visited) = run(&outcomes, Strategy::Bisect, 10);

            assert_eq!(search.lowest_passing(), Some(1));
            assert_eq!(visited.len(), 3);
            assert!(search.minimum_verified());
        }
    }

    #[test]
    fn a_failed_bisection_checks_skipped_releases_before_giving_up() {
        let mut outcomes = vec![Outcome::ApiFailure; 32];
        outcomes[1] = Outcome::Passed;

        let (search, _) = run(&outcomes, Strategy::Bisect, 10);

        assert_eq!(search.lowest_passing(), Some(1));
        assert!(search.minimum_verified());
    }

    #[test]
    fn skipped_passing_releases_do_not_produce_a_verified_minimum() {
        let mut outcomes = vec![Outcome::ApiFailure; 32];
        outcomes[1] = Outcome::Passed;
        outcomes[23..].fill(Outcome::Passed);

        let (search, _) = run(&outcomes, Strategy::Bisect, 10);

        assert_eq!(search.lowest_passing(), Some(23));
        assert!(!search.minimum_verified());
    }

    #[test]
    fn first_release_passing_and_small_ranges_are_fully_checked() {
        let (search, visited) = run(&[Outcome::Passed; 229], Strategy::Bisect, 10);

        assert_eq!(visited, [0]);
        assert!(search.minimum_verified());

        let mut outcomes = vec![Outcome::ApiFailure; 10];
        outcomes[5..].fill(Outcome::Passed);
        let (search, visited) = run(&outcomes, Strategy::Bisect, 10);

        assert_eq!(visited, [0, 1, 2, 3, 4, 5]);
        assert!(search.minimum_verified());
    }

    #[test]
    fn earlier_inconclusive_results_prevent_minimum_verification() {
        let (search, _) = run(
            &[Outcome::Inconclusive, Outcome::Passed],
            Strategy::Bisect,
            1,
        );

        assert_eq!(search.lowest_passing(), Some(1));
        assert!(!search.minimum_verified());
    }
}
