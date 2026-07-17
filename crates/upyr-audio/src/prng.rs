//! The deterministic four-word xorshift generator used by pfxr.

/// pfxr-compatible xorshift128-style pseudo-random number generator.
pub(crate) struct Prng {
    a: u32,
    b: u32,
    c: u32,
    d: u32,
}

impl Prng {
    /// Seed as pfxr does, including its 32-draw warm-up.
    pub(crate) fn new(seed: u32) -> Self {
        let mut generator = Self {
            a: seed,
            b: 362_436_069,
            c: 521_288_629,
            d: 88_675_123,
        };
        for _ in 0..32 {
            generator.raw();
        }
        generator
    }

    fn raw(&mut self) -> u32 {
        let mixed = self.a ^ (self.a << 11);
        self.a = self.b;
        self.b = self.c;
        self.c = self.d;
        self.d = self.d ^ (self.d >> 19) ^ (mixed ^ (mixed >> 8));
        self.d ^ 0x8000_0000
    }

    pub(crate) fn range(&mut self, min: f64, max: f64) -> f64 {
        min + (max - min) * f64::from(self.raw()) / f64::from(u32::MAX)
    }

    pub(crate) fn signed_unit(&mut self) -> f64 {
        self.range(-1.0, 1.0)
    }

    pub(crate) fn index(&mut self, len: usize) -> usize {
        debug_assert!(len > 0);
        let index = self.range(0.0, len as f64).floor() as usize;
        index.min(len - 1)
    }
}
