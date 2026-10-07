//! Beam search: how the decoder's scores become one translation.
//!
//! The decoder gives a score to every vocabulary entry as the next token.
//! Taking the best one each time (a beam of 1) is fast but can paint itself
//! into a corner; keeping the few best unfinished translations side by side
//! lets a start that looked second best win in the end. The rules (length
//! normalisation, when to stop) are those of the `transformers` library, so
//! the outputs can be compared with the reference implementation.

/// A decoder that is stepped one token at a time.
pub(crate) trait Steps {
    /// The scores (logits) of every vocabulary entry as the next token of
    /// each beam, one beam after the other. `last` holds the newest token of
    /// each beam and `position` counts the tokens decoded before it.
    fn step(&mut self, last: &[u32], position: usize) -> candle_core::Result<Vec<f32>>;

    /// Tells the decoder that beam `i` now continues former beam
    /// `parents[i]`.
    fn reorder(&mut self, parents: &[u32]) -> candle_core::Result<()>;
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Search {
    /// How many unfinished translations are kept; 1 is greedy decoding.
    pub beams: usize,
    /// Decoding stops here even if no translation is finished.
    pub max_tokens: usize,
    /// The token the decoder is given first.
    pub start: u32,
    /// The token that ends a translation.
    pub end: u32,
    /// A token the model must never produce (Marian's padding).
    pub never: u32,
}

/// An unfinished translation.
struct Beam {
    tokens: Vec<u32>,
    /// Sum of the log-probabilities of its tokens.
    score: f32,
}

/// A way to continue a beam by one token.
struct Step {
    parent: usize,
    token: u32,
    score: f32,
}

impl Search {
    /// The best translation, without the start and end tokens. `wanted` is
    /// asked before each token; when it says no, the search stops there and
    /// gives `None`.
    pub fn run(&self, decoder: &mut impl Steps, wanted: &mut (dyn FnMut() -> bool + Send)) -> candle_core::Result<Option<Vec<u32>>> {
        let beams = self.beams.max(1);
        let mut live = vec![Beam { tokens: Vec::new(), score: 0.0 }];
        let mut finished: Vec<Beam> = Vec::new();

        for position in 0..self.max_tokens {
            if !wanted() {
                return Ok(None);
            }
            let last: Vec<u32> = live.iter().map(|beam| beam.tokens.last().copied().unwrap_or(self.start)).collect();
            let mut scores = decoder.step(&last, position)?;
            if scores.is_empty() || scores.len() % live.len() != 0 {
                candle_core::bail!("the decoder returned {} scores for {} beams", scores.len(), live.len())
            }
            let vocab = scores.len() / live.len();

            let mut steps: Vec<Step> = Vec::with_capacity(live.len() * beams);
            for (parent, (beam, scores)) in live.iter().zip(scores.chunks_exact_mut(vocab)).enumerate() {
                if let Some(never) = scores.get_mut(self.never as usize) {
                    *never = f32::NEG_INFINITY;
                }
                // With one beam only the order of the scores matters, and
                // the pass over the whole vocabulary can be saved.
                if beams > 1 {
                    log_softmax(scores);
                }
                steps.extend(best(scores, beams).map(|(token, score)| Step { parent, token, score: beam.score + score }));
            }
            steps.sort_by(|a, b| b.score.total_cmp(&a.score));
            steps.truncate(beams);

            let mut next = Vec::with_capacity(beams);
            let mut parents = Vec::with_capacity(beams);
            for step in steps {
                let mut tokens = live[step.parent].tokens.clone();
                if step.token == self.end {
                    // The end token counts in the length, like in the reference.
                    finished.push(Beam { score: step.score / (tokens.len() + 1) as f32, tokens });
                } else {
                    tokens.push(step.token);
                    next.push(Beam { tokens, score: step.score });
                    parents.push(step.parent as u32);
                }
            }
            live = next;
            if self.settled(&live, &mut finished, position + 1) {
                break;
            }
            // Most steps keep every beam where it was: nothing to move then.
            if !parents.iter().copied().eq(0..last.len() as u32) {
                decoder.reorder(&parents)?;
            }
        }

        // When nothing finished within the limit, the best unfinished
        // translation is all there is. The two kinds are not compared: only
        // the finished ones have their score divided by their length.
        let by_score = |a: &Beam, b: &Beam| a.score.total_cmp(&b.score);
        let best = finished.into_iter().max_by(by_score).or_else(|| live.into_iter().max_by(by_score));
        Ok(Some(best.map(|beam| beam.tokens).unwrap_or_default()))
    }

    /// True when no unfinished translation can still beat the finished ones
    /// that would be kept. `finished` is left sorted, best first.
    fn settled(&self, live: &[Beam], finished: &mut [Beam], length: usize) -> bool {
        let Some(best_live) = live.iter().map(|beam| beam.score).max_by(f32::total_cmp) else {
            return true;
        };
        finished.sort_by(|a, b| b.score.total_cmp(&a.score));
        let beams = self.beams.max(1);
        finished.len() >= beams && best_live / length as f32 <= finished[beams - 1].score
    }
}

/// Turns scores into log-probabilities in place.
fn log_softmax(scores: &mut [f32]) {
    let largest = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let sum: f32 = scores.iter().map(|score| (score - largest).exp()).sum();
    let log_sum = largest + sum.ln();
    scores.iter_mut().for_each(|score| *score -= log_sum);
}

/// The `count` best entries as `(index, value)`, best first. Entries that
/// are impossible (minus infinity) or not numbers are never returned.
fn best(values: &[f32], count: usize) -> impl Iterator<Item = (u32, f32)> {
    let mut kept: Vec<(u32, f32)> = Vec::with_capacity(count + 1);
    for (index, value) in values.iter().copied().enumerate().filter(|(_, value)| value.is_finite()) {
        if kept.len() < count || kept.last().is_some_and(|(_, worst)| value > *worst) {
            let place = kept.partition_point(|(_, other)| *other >= value);
            kept.insert(place, (index as u32, value));
            kept.truncate(count);
        }
    }
    kept.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A decoder over a four-token vocabulary (0 = end, 3 = never) whose
    /// scores depend only on the tokens so far.
    struct Scripted {
        beams: Vec<Vec<u32>>,
        probabilities: fn(&[u32]) -> [f32; 4],
        reorders: usize,
    }

    impl Scripted {
        fn new(probabilities: fn(&[u32]) -> [f32; 4]) -> Self {
            Self { beams: vec![Vec::new()], probabilities, reorders: 0 }
        }
    }

    impl Steps for Scripted {
        fn step(&mut self, last: &[u32], position: usize) -> candle_core::Result<Vec<f32>> {
            assert_eq!(last.len(), self.beams.len());
            let mut scores = Vec::new();
            for (beam, token) in self.beams.iter_mut().zip(last) {
                assert_eq!(beam.len(), position);
                beam.push(*token);
                // The first token is the start token, which is not history.
                scores.extend((self.probabilities)(&beam[1..]).map(f32::ln));
            }
            Ok(scores)
        }

        fn reorder(&mut self, parents: &[u32]) -> candle_core::Result<()> {
            self.beams = parents.iter().map(|parent| self.beams[*parent as usize].clone()).collect();
            self.reorders += 1;
            Ok(())
        }
    }

    const SEARCH: Search = Search { beams: 1, max_tokens: 8, start: 9, end: 0, never: 3 };

    /// Runs a search that nobody stops.
    fn run(search: Search, decoder: &mut impl Steps) -> Vec<u32> {
        search.run(decoder, &mut || true).expect("no decoder error").expect("not stopped")
    }

    /// Token 1 looks best first but leads nowhere good; token 2 then 1 is
    /// the likeliest translation as a whole.
    fn garden_path(history: &[u32]) -> [f32; 4] {
        match history {
            [] => [0.0, 0.6, 0.4, 0.0],
            [1] => [0.34, 0.33, 0.33, 0.0],
            [2] => [0.05, 0.9, 0.05, 0.0],
            _ => [0.9, 0.05, 0.05, 0.0],
        }
    }

    #[test]
    fn one_beam_takes_the_best_token_each_time() {
        let mut decoder = Scripted::new(garden_path);
        assert_eq!(run(SEARCH, &mut decoder), [1]);
        assert_eq!(decoder.reorders, 0);
    }

    #[test]
    fn more_beams_find_the_translation_that_is_best_as_a_whole() {
        let mut decoder = Scripted::new(garden_path);
        let search = Search { beams: 3, ..SEARCH };
        assert_eq!(run(search, &mut decoder), [2, 1]);
        assert!(decoder.reorders > 0, "the decoder state must follow the beams");
    }

    #[test]
    fn the_forbidden_token_is_never_produced_and_the_limit_is_kept() {
        // Padding scores highest, the end never comes.
        let mut decoder = Scripted::new(|_| [0.0, 0.2, 0.1, 0.7]);
        let search = Search { beams: 2, max_tokens: 5, ..SEARCH };
        assert_eq!(run(search, &mut decoder), [1, 1, 1, 1, 1]);
    }

    #[test]
    fn scores_of_the_wrong_size_are_an_error() {
        struct Broken;
        impl Steps for Broken {
            fn step(&mut self, _: &[u32], _: usize) -> candle_core::Result<Vec<f32>> {
                Ok(Vec::new())
            }
            fn reorder(&mut self, _: &[u32]) -> candle_core::Result<()> {
                Ok(())
            }
        }
        assert!(SEARCH.run(&mut Broken, &mut || true).is_err());
    }

    #[test]
    fn a_search_that_is_no_longer_wanted_stops_at_the_next_token() {
        let mut decoder = Scripted::new(|_| [0.0, 0.2, 0.1, 0.7]);
        let mut asked = 0;
        let mut wanted = || {
            asked += 1;
            asked <= 3
        };
        assert_eq!(SEARCH.run(&mut decoder, &mut wanted).expect("no decoder error"), None);
        assert_eq!(decoder.beams[0].len(), 3, "three tokens were decoded, not eight");
    }
}
