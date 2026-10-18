//! Allowance arithmetic for Familiar.
//!
//! A leash lets its agent pay at most `limit` (in µT) per window of `window_epochs` epochs. Windows are aligned to the
//! epoch the leash was created in: window `k` covers epochs `[origin + k * window_epochs, origin + (k + 1) *
//! window_epochs)`. Everything here is pure and total: overflow, a zero payment, a clock that went backwards and a
//! broken policy are refusals, never panics, so the template decides how to fail.

use core::fmt;

/// The leash's rule: at most `limit` per `window_epochs` epochs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub limit: u64,
    pub window_epochs: u64,
}

/// The current window: the epoch it started in and what has been spent in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start: u64,
    pub spent: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    InvalidPolicy,
    ZeroPayment,
    ClockWentBack { now: u64, start: u64 },
    WindowLimit { spent: u64, amount: u64, limit: u64 },
    Overflow,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy => write!(f, "familiar: invalid policy (limit and window must be positive)"),
            Self::ZeroPayment => write!(f, "familiar: zero payment"),
            Self::ClockWentBack { now, start } => {
                write!(f, "familiar: epoch {now} is before the window start {start}")
            }
            Self::WindowLimit { spent, amount, limit } => write!(
                f,
                "familiar: window limit exceeded: {spent} of {limit} spent, {} left, payment of {amount} refused",
                limit.saturating_sub(*spent)
            ),
            Self::Overflow => write!(f, "familiar: arithmetic overflow"),
        }
    }
}

impl Policy {
    pub fn validate(&self) -> Result<(), Refusal> {
        if self.limit == 0 || self.window_epochs == 0 {
            return Err(Refusal::InvalidPolicy);
        }
        Ok(())
    }
}

/// The window as seen at epoch `now`: rolled forward (with `spent` reset) if `now` is past its end.
pub fn current(policy: &Policy, window: Window, now: u64) -> Result<Window, Refusal> {
    policy.validate()?;
    if now < window.start {
        return Err(Refusal::ClockWentBack {
            now,
            start: window.start,
        });
    }
    let elapsed = now - window.start;
    if elapsed < policy.window_epochs {
        return Ok(window);
    }
    // Aligned roll: the new start is the last grid point at or before `now`.
    Ok(Window {
        start: now - elapsed % policy.window_epochs,
        spent: 0,
    })
}

/// Charges `amount` at epoch `now`. Returns the window to store, or why the payment is refused.
pub fn charge(policy: &Policy, window: Window, now: u64, amount: u64) -> Result<Window, Refusal> {
    if amount == 0 {
        return Err(Refusal::ZeroPayment);
    }
    let window = current(policy, window, now)?;
    let spent = window.spent.checked_add(amount).ok_or(Refusal::Overflow)?;
    if spent > policy.limit {
        return Err(Refusal::WindowLimit {
            spent: window.spent,
            amount,
            limit: policy.limit,
        });
    }
    Ok(Window {
        start: window.start,
        spent,
    })
}

/// What the agent may still pay in the window seen at `now`.
pub fn remaining(policy: &Policy, window: Window, now: u64) -> Result<u64, Refusal> {
    let window = current(policy, window, now)?;
    Ok(policy.limit.saturating_sub(window.spent))
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: Policy = Policy {
        limit: 100,
        window_epochs: 10,
    };
    const W0: Window = Window { start: 50, spent: 0 };

    #[test]
    fn charges_within_the_limit() {
        let w = charge(&P, W0, 50, 40).unwrap();
        let w = charge(&P, w, 59, 60).unwrap();
        assert_eq!(w, Window { start: 50, spent: 100 });
        assert_eq!(remaining(&P, w, 59).unwrap(), 0);
    }

    #[test]
    fn refuses_past_the_limit_with_numbers() {
        let w = charge(&P, W0, 51, 90).unwrap();
        let err = charge(&P, w, 52, 11).unwrap_err();
        assert_eq!(
            err,
            Refusal::WindowLimit {
                spent: 90,
                amount: 11,
                limit: 100
            }
        );
        assert_eq!(
            err.to_string(),
            "familiar: window limit exceeded: 90 of 100 spent, 10 left, payment of 11 refused"
        );
    }

    #[test]
    fn rolls_to_an_aligned_window() {
        let w = charge(&P, W0, 50, 100).unwrap();
        // epoch 60 starts window 1, epoch 87 is in window 3 (start 80)
        assert_eq!(charge(&P, w, 60, 100).unwrap(), Window { start: 60, spent: 100 });
        assert_eq!(charge(&P, w, 87, 1).unwrap(), Window { start: 80, spent: 1 });
    }

    #[test]
    fn refuses_zero_backwards_clock_overflow_and_bad_policy() {
        assert_eq!(charge(&P, W0, 50, 0).unwrap_err(), Refusal::ZeroPayment);
        assert_eq!(
            charge(&P, W0, 49, 1).unwrap_err(),
            Refusal::ClockWentBack { now: 49, start: 50 }
        );
        let big = Policy {
            limit: u64::MAX,
            window_epochs: 10,
        };
        let w = charge(&big, W0, 50, u64::MAX).unwrap();
        assert_eq!(charge(&big, w, 50, 1).unwrap_err(), Refusal::Overflow);
        let bad = Policy {
            limit: 0,
            window_epochs: 10,
        };
        assert_eq!(charge(&bad, W0, 50, 1).unwrap_err(), Refusal::InvalidPolicy);
        let bad = Policy {
            limit: 10,
            window_epochs: 0,
        };
        assert_eq!(charge(&bad, W0, 50, 1).unwrap_err(), Refusal::InvalidPolicy);
    }

    #[test]
    fn huge_epochs_do_not_overflow() {
        let w = Window { start: 0, spent: 5 };
        assert_eq!(current(&P, w, u64::MAX).unwrap().spent, 0);
    }

    /// Exhaustive-ish property: whatever sequence of payments is attempted, the accepted ones never sum to more than
    /// `limit` inside any aligned window, and a refused payment never changes the stored window.
    #[test]
    fn accepted_payments_never_exceed_the_limit_in_any_window() {
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move |m: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % m
        };
        for limit in 1..=6u64 {
            for window_epochs in 1..=5u64 {
                let policy = Policy { limit, window_epochs };
                for _run in 0..200 {
                    let origin = next(20);
                    let mut w = Window {
                        start: origin,
                        spent: 0,
                    };
                    let mut now = origin;
                    let mut per_window = std::collections::BTreeMap::<u64, u64>::new();
                    for _ in 0..30 {
                        now += next(3);
                        let amount = next(limit + 2);
                        match charge(&policy, w, now, amount) {
                            Ok(nw) => {
                                let k = (now - origin) / window_epochs;
                                *per_window.entry(k).or_default() += amount;
                                assert_eq!(nw.start, origin + k * window_epochs);
                                w = nw;
                            }
                            Err(_) => {
                                // stored window untouched by a refusal (the caller keeps `w`)
                            }
                        }
                    }
                    for (k, total) in per_window {
                        assert!(total <= limit, "window {k}: {total} > {limit}");
                    }
                }
            }
        }
    }
}
