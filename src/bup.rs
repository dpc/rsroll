use {RollingHash, CDC};
use std::default::Default;

const WINDOW_BITS: usize = 6;
const WINDOW_SIZE: usize = 1 << WINDOW_BITS;

const CHAR_OFFSET: usize = 31;

/// Default chunk size used by `bup`
pub const CHUNK_SIZE: u32 = 1 << CHUNK_BITS;

/// Default chunk size used by `bup` (log2)
pub const CHUNK_BITS: u32 = 13;


/// Rolling checksum method used by `bup`
///
/// Strongly based on
/// https://github.com/bup/bup/blob/706e8d273/lib/bup/bupsplit.c
/// https://github.com/bup/bup/blob/706e8d273/lib/bup/bupsplit.h
/// (a bit like https://godoc.org/camlistore.org/pkg/rollsum)
pub struct Bup {
    s1: usize,
    s2: usize,
    window: [u8; WINDOW_SIZE],
    wofs: usize,
    chunk_bits: u32,
}

impl Default for Bup {
    fn default() -> Self {
        Bup {
            s1: WINDOW_SIZE * CHAR_OFFSET,
            s2: WINDOW_SIZE * (WINDOW_SIZE-1) * CHAR_OFFSET,
            window: [0; WINDOW_SIZE],
            wofs: 0,
            chunk_bits: CHUNK_BITS,
        }
    }
}


impl RollingHash for Bup {
    type Digest = u32;

    fn roll_byte(&mut self, newch: u8) {
        // Since this crate is performance ciritical, and
        // we're in strict control of `wofs`, it is justified
        // to skip bound checking to increase the performance
        // https://github.com/rust-lang/rfcs/issues/811
        let prevch = unsafe { *self.window.get_unchecked(self.wofs) };
        self.add(prevch, newch);
        unsafe { *self.window.get_unchecked_mut(self.wofs)  = newch };
        self.wofs = (self.wofs + 1) % WINDOW_SIZE;
    }

    fn digest(&self) -> u32 {
        ((self.s1 as u32) << 16) | ((self.s2 as u32) & 0xffff)
    }

    fn reset(&mut self) {
        *self = Bup {
            chunk_bits: self.chunk_bits,
            .. Default::default()
        }
    }
}

impl CDC for Bup {
    fn find_chunk<'a>(&mut self, buf: &'a [u8]) -> Option<(&'a [u8], &'a [u8])> {
        let chunk_mask = (1 << self.chunk_bits) - 1;
        for (i, &b) in buf.iter().enumerate() {
            self.roll_byte(b);

            if self.digest() & chunk_mask == chunk_mask {
                self.reset();
                return Some((&buf[..i+1], &buf[i+1..]));
            }
        }
        None
    }
}


impl Bup {
    /// Create new Bup engine with default chunking settings
    pub fn new() -> Self {
        Default::default()
    }

    /// Create new Bup engine with custom chunking settings
    ///
    /// `chunk_bits` is number of bits that need to match in
    /// the edge condition. `CHUNK_BITS` constant is the default.
    pub fn new_with_chunk_bits(chunk_bits: u32) -> Self {
        assert!(chunk_bits < 32);
        Bup {
            chunk_bits: chunk_bits,
            .. Default::default()
        }
    }

    fn add(&mut self, drop: u8, add: u8) {
        self.s1 += add as usize;
        self.s1 -= drop as usize;
        self.s2 += self.s1;
        self.s2 -= WINDOW_SIZE * (drop as usize + CHAR_OFFSET);
    }

    /// Counts the number of low bits set in the rollsum, assuming
    /// the digest has the bottom `CHUNK_BITS` bits set to `1`
    /// (i.e. assuming a digest at a default bup chunk edge, as
    /// returned by `find_chunk_edge`).
    /// Be aware that there's a deliberate 'bug' in this function
    /// in order to match expected return values from other bupsplit
    /// implementations.
    pub fn count_bits(&self) -> u32 {
        let mut bits = self.chunk_bits;
        let mut rsum = self.digest() >> self.chunk_bits;
        // Yes, the ordering of this loop does mean that the
        // `CHUNK_BITS+1`th bit will be ignored. This isn't actually
        // a problem as the distribution of values will be the same,
        // but it is unexpected.
        loop {
            rsum >>= 1;
            if (rsum & 1) == 0 {
                break;
            }
            bits += 1;
        }
        bits
    }
}

#[cfg(feature = "bench")]
mod tests {
    use test::Bencher;
    use super::{Bup, CDC};
    use tests::test_data_1mb;

    #[bench]
    fn bup_perf_1mb(b: &mut Bencher) {
        let v = test_data_1mb();
        b.bytes = v.len() as u64;

        b.iter(|| {
            let mut cdc = Bup::new();
            let mut buf = v.as_slice();

            while let Some((_last, rest)) = cdc.find_chunk(buf) {
                buf = rest;
            }
        });
    }
}
