#![cfg(test)]

// std::hash::Hasher designed to cause a lot of collisions for testing purposes.
// Computes single parity bit of all input bits. Hashes will always be 0 or 1.
pub struct TerribleHasher {
    state: u8,
}
impl Default for TerribleHasher {
    fn default() -> Self {
        Self { state: 0 }
    }
}
impl std::hash::Hasher for TerribleHasher {
    fn write(&mut self, msg: &[u8]) {
        let b: u8 = msg.iter().fold(0, |accum, data| accum ^ data);
        self.state ^= b;
    }
    #[inline]
    fn finish(&self) -> u64 {
        let r1 = (self.state & 0b00001111) ^ ((self.state & 0b11110000) >> 4);
        let r2 = (r1 & 0b0011) ^ ((r1 & 0b1100) >> 2);
        let r3 = (r2 & 0b01) ^ ((r2 & 0b10) >> 1);
        assert!(r3 == 0 || r3 == 1);
        r3 as u64
    }
}
