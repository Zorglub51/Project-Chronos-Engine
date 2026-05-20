// Mersenne Twister MT19937 — reference implementation matching the seeding
// scheme used by M2's MArchiveBatchTool. Specifically, init_by_array() must
// produce the same sequence as the C reference and Python mzstool.py.

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

pub struct MT19937 {
    mt: [u32; N],
    mti: usize,
}

impl MT19937 {
    pub fn new() -> Self {
        Self { mt: [0; N], mti: N + 1 }
    }

    pub fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for i in 1..N {
            let prev = self.mt[i - 1];
            self.mt[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        self.mti = N;
    }

    pub fn init_by_array(&mut self, init_key: &[u32]) {
        self.init_genrand(19_650_218);
        let mut i: usize = 1;
        let mut j: usize = 0;
        let k = N.max(init_key.len());
        for _ in 0..k {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i] ^ ((prev ^ (prev >> 30)).wrapping_mul(1_664_525)))
                .wrapping_add(init_key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            if j >= init_key.len() {
                j = 0;
            }
        }
        for _ in 0..(N - 1) {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i] ^ ((prev ^ (prev >> 30)).wrapping_mul(1_566_083_941)))
                .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
        }
        self.mt[0] = 0x8000_0000;
    }

    pub fn genrand_int32(&mut self) -> u32 {
        let mag01 = [0u32, MATRIX_A];
        if self.mti >= N {
            for kk in 0..(N - M) {
                let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            }
            for kk in (N - M)..(N - 1) {
                let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M - N] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            }
            let y = (self.mt[N - 1] & UPPER_MASK) | (self.mt[0] & LOWER_MASK);
            self.mt[N - 1] = self.mt[M - 1] ^ (y >> 1) ^ mag01[(y & 1) as usize];
            self.mti = 0;
        }
        let mut y = self.mt[self.mti];
        self.mti += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_by_array_matches_c_reference() {
        // First 10 outputs from mt19937ar.c with init_key = {0x123, 0x234, 0x345, 0x456}.
        let mut mt = MT19937::new();
        mt.init_by_array(&[0x123, 0x234, 0x345, 0x456]);
        let expected: [u32; 10] = [
            1_067_595_299, 955_945_823, 477_289_528, 4_107_218_783, 4_228_976_476,
            3_344_332_714, 3_355_579_695, 227_628_506, 810_200_273, 2_591_290_167,
        ];
        for &want in &expected {
            assert_eq!(mt.genrand_int32(), want);
        }
    }
}
