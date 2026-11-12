//! Rule outputs shared by the build-time compiler and runtime matcher.

#[cfg(test)]
use super::punto_compile::Mode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rule(pub(super) u32);

impl Rule {
    pub(super) const fn anchored(self) -> bool {
        self.0 & 3 != 0
    }
    pub(super) const fn exact(self) -> bool {
        self.0 & 3 == 1
    }
    // Every source row has a unique rank, including across the two case trees.
    // Rank occupies the high bits, so integer order preserves native priority.
    pub(super) const fn order_key(self) -> u32 {
        self.0
    }

    #[cfg(test)]
    pub(super) const fn rank(self) -> u32 {
        self.0 >> 5
    }
    #[cfg(test)]
    pub(super) const fn mode(self) -> Mode {
        match self.0 & 3 {
            0 => Mode::Anywhere,
            1 => Mode::Exact,
            2 => Mode::Beginning,
            _ => unreachable!(),
        }
    }
    pub(super) const fn exception(self) -> bool {
        self.0 & 4 != 0
    }
    pub(super) const fn priority(self) -> bool {
        self.0 & 8 != 0
    }
    pub(super) const fn after_dash(self) -> bool {
        self.0 & 16 != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_modes_flags_and_rank_extremes_round_trip() {
        assert_eq!(std::mem::size_of::<Rule>(), 4);
        for rank in [0, 112_835, u32::MAX >> 5] {
            for mode in [Mode::Anywhere, Mode::Exact, Mode::Beginning] {
                for flags in 0..8 {
                    let e = flags & 1 != 0;
                    let a = flags & 2 != 0;
                    let d = flags & 4 != 0;
                    let rule = Rule::new(rank, mode, e, a, d);
                    assert_eq!(
                        (
                            rule.rank(),
                            rule.mode(),
                            rule.exception(),
                            rule.priority(),
                            rule.after_dash()
                        ),
                        (rank, mode, e, a, d)
                    );
                }
            }
        }
    }
}
