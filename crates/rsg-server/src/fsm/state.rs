//! The request lifecycle's explicit transition table (LIFE-01): exactly 12
//! of the 49 `(from, to)` pairs are allowed, every transition out of a
//! terminal state (`Finished`/`Cancelled`/`Failed`) is rejected, and
//! `first_token_at` is set exactly once, on the `Submitted -> Decoding`
//! transition.

use std::fmt;
use std::time::Instant;

/// The request lifecycle's states (LIFE-01).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LifecycleState {
    Received,
    Tokenizing,
    Submitted,
    Decoding,
    Finished,
    Cancelled,
    Failed,
}

impl LifecycleState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            LifecycleState::Finished | LifecycleState::Cancelled | LifecycleState::Failed
        )
    }
}

/// Whether `to` is a valid next state from `from`.
///
/// TODO(GREEN): this first draft hasn't implemented the table yet.
pub fn can_transition(_from: LifecycleState, _to: LifecycleState) -> bool {
    true
}

/// An attempted transition `can_transition` rejects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionError {
    pub uid: i64,
    pub from: LifecycleState,
    pub to: LifecycleState,
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "uid {}: invalid lifecycle transition {:?} -> {:?}",
            self.uid, self.from, self.to
        )
    }
}

impl std::error::Error for TransitionError {}

/// One request's lifecycle progress: its current state, when it was first
/// seen, and when it first reached `Decoding` (TTFT).
#[derive(Debug, Clone)]
pub struct ReqState {
    pub uid: i64,
    pub state: LifecycleState,
    pub received_at: Instant,
    pub first_token_at: Option<Instant>,
}

impl ReqState {
    pub fn new(uid: i64, at: Instant) -> ReqState {
        ReqState {
            uid,
            state: LifecycleState::Received,
            received_at: at,
            first_token_at: None,
        }
    }

    /// Attempts the transition to `to` at time `at`. Rejects (leaving
    /// `self.state` unchanged) any transition `can_transition` disallows,
    /// which includes every transition out of a terminal state. Records
    /// `first_token_at` on the `Submitted -> Decoding` transition.
    pub fn advance(&mut self, to: LifecycleState, at: Instant) -> Result<(), TransitionError> {
        if !can_transition(self.state, to) {
            return Err(TransitionError {
                uid: self.uid,
                from: self.state,
                to,
            });
        }
        if self.state == LifecycleState::Submitted && to == LifecycleState::Decoding {
            self.first_token_at = Some(at);
        }
        self.state = to;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_table_is_exact() {
        use LifecycleState::*;
        let all = [
            Received, Tokenizing, Submitted, Decoding, Finished, Cancelled, Failed,
        ];
        let allowed: std::collections::HashSet<(LifecycleState, LifecycleState)> = [
            (Received, Tokenizing),
            (Received, Cancelled),
            (Received, Failed),
            (Tokenizing, Submitted),
            (Tokenizing, Cancelled),
            (Tokenizing, Failed),
            (Submitted, Decoding),
            (Submitted, Cancelled),
            (Submitted, Failed),
            (Decoding, Finished),
            (Decoding, Cancelled),
            (Decoding, Failed),
        ]
        .into_iter()
        .collect();
        assert_eq!(allowed.len(), 12);

        let mut checked = 0;
        for &from in &all {
            for &to in &all {
                checked += 1;
                let expected = allowed.contains(&(from, to));
                assert_eq!(
                    can_transition(from, to),
                    expected,
                    "from {from:?} to {to:?}"
                );
            }
        }
        assert_eq!(checked, 49);
    }

    #[test]
    fn advance_records_first_token_and_rejects_terminal_exit() {
        let t0 = Instant::now();
        let mut rs = ReqState::new(7, t0);
        rs.advance(LifecycleState::Tokenizing, t0)
            .expect("received->tokenizing");
        rs.advance(LifecycleState::Submitted, t0)
            .expect("tokenizing->submitted");

        let t1 = Instant::now();
        rs.advance(LifecycleState::Decoding, t1)
            .expect("submitted->decoding");
        assert_eq!(rs.first_token_at, Some(t1));

        rs.advance(LifecycleState::Finished, t1)
            .expect("decoding->finished");

        let err = rs.advance(LifecycleState::Cancelled, t1).unwrap_err();
        assert_eq!(
            err,
            TransitionError {
                uid: 7,
                from: LifecycleState::Finished,
                to: LifecycleState::Cancelled,
            }
        );
        assert_eq!(rs.state, LifecycleState::Finished);
    }
}
