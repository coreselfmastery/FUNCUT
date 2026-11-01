// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Deterministic simplex noise: the seed is the whole contract.
//!
//! Organic motion is the one thing a keyframe cannot write by hand: a drift
//! that never repeats, a field of motes that never collides the same way
//! twice, a background that breathes instead of sitting. Noise is how it is
//! done, and determinism is the one rule it must obey - a comp that renders
//! differently on two machines is a broken cache, a broken CI and a broken
//! promise, so the noise here takes its randomness from a seed the document
//! carries, and nothing else:
//!
//! - `Noise::from_seed(7)` answers the same values on every machine, in the
//!   same order, forever - the permutation table is a pure function of the
//!   seed, hashed through SplitMix64 (no allocator RNG, no clock, no
//!   per-process state).
//! - A different seed is a different world; a missing seed is a refusal in
//!   the APIs that take one, never a silently-picked one. Unseeded
//!   randomness is exactly what the determinism contract forbids, and a
//!   default seed would make every wobble the same wobble, which is worse
//!   than no wobble at all.
//!
//! The engine plays keyframes, not functions, so everything this module
//! computes is *baked* by the callers - a wobble becomes position keys, a
//! particle field becomes per-element `@keyframes` - and once baked the
//! noise has done its job: the document carries only the numbers, and the
//! renderer needs no randomness of any kind.

/// A seeded simplex noise field: 2-D and 3-D samples in `[-1, 1]`, pure
/// functions of the seed and the coordinates.
///
/// The permutation table is Gustavson's classic simplex setup - 256 sources
/// gradient indices, doubled and shuffled - and the shuffle is a
/// Fisher-Yates walk driven by SplitMix64 over the seed, so two seeds give
/// two different tables and one seed gives one table everywhere.
#[derive(Clone, Debug, PartialEq)]
pub struct Noise {
    /// The shuffled gradient-index permutation, doubled to avoid a modulo.
    perm: [u8; 512],
}

/// SplitMix64: the fastest way to turn a seed into a well-spread stream of
/// u64s, which is what a Fisher-Yates shuffle wants.
fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Noise {
    /// The noise field a seed names. The same seed is the same field on
    /// every machine and every run - the guarantee the cache keys and the
    /// golden frames are built on.
    pub fn from_seed(seed: u64) -> Noise {
        // Fisher-Yates over 0..=255, drawing from the seed's own stream:
        // a shuffle, not a hash of an index, so no two seeds share a table
        // by the accident of a poor bit mix.
        let mut sources: [u8; 256] = [0; 256];
        for (index, slot) in sources.iter_mut().enumerate() {
            *slot = index as u8;
        }
        let mut state = seed;
        for index in (1..256).rev() {
            let pick = (splitmix(&mut state) % (index as u64 + 1)) as usize;
            sources.swap(index, pick);
        }
        let mut perm = [0u8; 512];
        for (index, slot) in perm.iter_mut().enumerate() {
            *slot = sources[index & 255];
        }
        Noise { perm }
    }

    /// One 2-D sample, in about `[-1, 1]`. Continuous, smooth, never
    /// periodic - the shape a keyframe list samples for organic drift.
    ///
    /// The gradient at each of the three corners is picked by the corner's
    /// own cell coordinates, hashed through the table on both axes: a
    /// corner shared by two cells must answer one gradient or the field
    /// tears at every cell boundary, and a field that tears is a wobble
    /// that strobes.
    pub fn noise2(&self, x: f64, y: f64) -> f64 {
        // The skewing constants of simplex noise: 0.5*(sqrt(3)-1) to skew
        // into triangle corners, (3-sqrt(3))/6 to unskew back.
        const F2: f64 = 0.366_025_403_784_438_6;
        const G2: f64 = 0.211_324_865_405_187_13;
        let skew = (x + y) * F2;
        let cell_x = (x + skew).floor();
        let cell_y = (y + skew).floor();
        let origin_x = x - cell_x + (cell_x + cell_y) * G2;
        let origin_y = y - cell_y + (cell_x + cell_y) * G2;
        let (cell_x, cell_y) = (cell_x as i64, cell_y as i64);
        // The middle corner sits one step along the triangle's long edge:
        // toward x when x is further from the cell's own origin.
        let (i1, j1) = if origin_x > origin_y { (1, 0) } else { (0, 1) };
        let corner = |dx: f64, dy: f64, ii: i64, jj: i64| -> f64 {
            // The radius cut-off of a simplex corner: 0.5 keeps the kernel
            // compact, and the 70x scale is Gustavson's own, chosen so the
            // outputs fill the range.
            const RADIUS: f64 = 0.5;
            let distance = dx * dx + dy * dy;
            if distance >= RADIUS {
                return 0.0;
            }
            let falloff = (RADIUS - distance).powi(4);
            // The gradient directions, indexed by the corner's own hashed
            // coordinates through the shuffled table.
            const GRADIENTS: [(f64, f64); 8] = [
                (1.0, 1.0),
                (-1.0, 1.0),
                (1.0, -1.0),
                (-1.0, -1.0),
                (1.0, 0.0),
                (-1.0, 0.0),
                (0.0, 1.0),
                (0.0, -1.0),
            ];
            let hash = self.perm[(ii as usize & 255) + self.perm[jj as usize & 255] as usize] & 7;
            let (gx, gy) = GRADIENTS[hash as usize];
            70.0 * falloff * (gx * dx + gy * dy)
        };
        let total = corner(origin_x, origin_y, cell_x, cell_y)
            + corner(
                origin_x - i1 as f64 + G2,
                origin_y - j1 as f64 + G2,
                cell_x + i1,
                cell_y + j1,
            )
            + corner(
                origin_x - 1.0 + 2.0 * G2,
                origin_y - 1.0 + 2.0 * G2,
                cell_x + 1,
                cell_y + 1,
            );
        // Three corners of a triangle, each cut off at the same radius: the
        // theoretical range is ±~1.0, held there rather than clamped,
        // because a clamp would crease the field.
        total.clamp(-1.0, 1.0)
    }

    /// One 3-D sample, in about `[-1, 1]` - the shape a time-driven wobble
    /// wants: two coordinates for the space, one for the clock, so several
    /// layers can share one field without ever reading the same place in
    /// it. The four corners of the tetrahedron are hashed on all three of
    /// their own coordinates, for the same reason `noise2` hashes on two.
    pub fn noise3(&self, x: f64, y: f64, z: f64) -> f64 {
        const F3: f64 = 1.0 / 3.0;
        const G3: f64 = 1.0 / 6.0;
        let skew = (x + y + z) * F3;
        let cell_x = (x + skew).floor();
        let cell_y = (y + skew).floor();
        let cell_z = (z + skew).floor();
        let unskew = (cell_x + cell_y + cell_z) * G3;
        let origin_x = x - cell_x + unskew;
        let origin_y = y - cell_y + unskew;
        let origin_z = z - cell_z + unskew;
        let (cell_x, cell_y, cell_z) = (cell_x as i64, cell_y as i64, cell_z as i64);
        // Which tetrahedron the point sits in, as the offsets of its two
        // middle corners: Gustavson's own six-way branch, kept exactly -
        // the ordering is what makes the field continuous, and a mangled
        // branch here is a crease there.
        let (i1, j1, k1, i2, j2, k2) = if origin_x >= origin_y {
            if origin_y >= origin_z {
                (1, 0, 0, 1, 1, 1)
            } else if origin_x >= origin_z {
                (1, 0, 0, 1, 0, 1)
            } else {
                (0, 0, 1, 1, 0, 1)
            }
        } else if origin_y < origin_z {
            (0, 0, 1, 0, 1, 1)
        } else if origin_x < origin_z {
            (0, 1, 0, 0, 1, 1)
        } else {
            (0, 1, 0, 1, 1, 0)
        };
        let corner = |dx: f64, dy: f64, dz: f64, ii: i64, jj: i64, kk: i64| -> f64 {
            const RADIUS: f64 = 0.6;
            let distance = dx * dx + dy * dy + dz * dz;
            if distance >= RADIUS {
                return 0.0;
            }
            let falloff = (RADIUS - distance).powi(4);
            // The twelve gradient directions of Gustavson's 3-D set.
            const GRADIENTS: [(f64, f64, f64); 12] = [
                (1.0, 1.0, 0.0),
                (-1.0, 1.0, 0.0),
                (1.0, -1.0, 0.0),
                (-1.0, -1.0, 0.0),
                (1.0, 0.0, 1.0),
                (-1.0, 0.0, 1.0),
                (1.0, 0.0, -1.0),
                (-1.0, 0.0, -1.0),
                (0.0, 1.0, 1.0),
                (0.0, -1.0, 1.0),
                (0.0, 1.0, -1.0),
                (0.0, -1.0, -1.0),
            ];
            let hash = self.perm[(ii as usize & 255)
                + self.perm[(jj as usize & 255) + self.perm[kk as usize & 255] as usize] as usize]
                % 12;
            let (gx, gy, gz) = GRADIENTS[hash as usize];
            32.0 * falloff * (gx * dx + gy * dy + gz * dz)
        };
        let total = corner(origin_x, origin_y, origin_z, cell_x, cell_y, cell_z)
            + corner(
                origin_x - i1 as f64 + G3,
                origin_y - j1 as f64 + G3,
                origin_z - k1 as f64 + G3,
                cell_x + i1,
                cell_y + j1,
                cell_z + k1,
            )
            + corner(
                origin_x - i2 as f64 + 2.0 * G3,
                origin_y - j2 as f64 + 2.0 * G3,
                origin_z - k2 as f64 + 2.0 * G3,
                cell_x + i2,
                cell_y + j2,
                cell_z + k2,
            )
            + corner(
                origin_x - 1.0 + 3.0 * G3,
                origin_y - 1.0 + 3.0 * G3,
                origin_z - 1.0 + 3.0 * G3,
                cell_x + 1,
                cell_y + 1,
                cell_z + 1,
            );
        total.clamp(-1.0, 1.0)
    }

    /// Fractal (multi-octave) 2-D noise: `octaves` samples, each twice the
    /// frequency and half the amplitude of the one before, summed and
    /// normalized. One octave is a smooth hill; three is a landscape. The
    /// amplitude of every octave is a power of the same gain, so the total
    /// stays inside `[-1, 1]` whatever the octave count.
    pub fn fbm2(&self, x: f64, y: f64, octaves: u32) -> f64 {
        let octaves = octaves.clamp(1, 6);
        let mut total = 0.0;
        let mut amplitude = 1.0;
        let mut norm = 0.0;
        let mut frequency = 1.0;
        for _ in 0..octaves {
            total += self.noise2(x * frequency, y * frequency) * amplitude;
            norm += amplitude;
            amplitude *= 0.5;
            frequency *= 2.0;
        }
        total / norm.max(1e-9)
    }

    /// One decorrelated (x, y) pair at a point on the clock: the shape a
    /// drift wants - one field, two channels, never the same number twice.
    /// `channel` decorrelates the callers that share one seed, so a stack
    /// of layers wobbling on the field never wobble in step.
    pub fn drift(&self, at: f64, channel: u32) -> (f64, f64) {
        let spread = f64::from(channel) * 137.831;
        (
            self.noise3(at, spread, 0.0),
            self.noise3(at, spread + 71.443, 0.0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every sample of a freshly made field, at points a grid would visit:
    /// inside the documented range, or the bake writes lies.
    #[test]
    fn samples_stay_inside_the_range_they_promise() {
        let noise = Noise::from_seed(7);
        for step in 0..200 {
            let x = f64::from(step) * 0.173 - 17.0;
            let y = f64::from(step) * 0.311 - 4.0;
            let two = noise.noise2(x, y);
            let three = noise.noise3(x, y, x * 0.5);
            assert!((-1.0..=1.0).contains(&two), "noise2 out of range: {two}");
            assert!(
                (-1.0..=1.0).contains(&three),
                "noise3 out of range: {three}"
            );
        }
    }

    /// The contract the cache keys and the golden frames stand on: the same
    /// seed answers the same numbers, every machine, every run.
    #[test]
    fn the_same_seed_answers_the_same_field_everywhere() {
        let left = Noise::from_seed(0x5EED_D00D);
        let right = Noise::from_seed(0x5EED_D00D);
        assert_eq!(left, right);
        for step in 0..50 {
            let at = f64::from(step) * 0.37;
            assert_eq!(left.drift(at, 0), right.drift(at, 0));
            assert_eq!(left.noise2(at, at * 2.0), right.noise2(at, at * 2.0));
        }
    }

    /// A different seed is a different world - and not merely a shifted one,
    /// which is what a weak seed mix would produce.
    #[test]
    fn a_different_seed_is_a_different_field() {
        let left = Noise::from_seed(1);
        let right = Noise::from_seed(2);
        let mut differ = 0;
        for step in 0..32 {
            let at = f64::from(step) * 0.29;
            if left.drift(at, 3) != right.drift(at, 3) {
                differ += 1;
            }
        }
        assert!(differ >= 30, "two seeds agreed {differ}/32 times");
    }

    /// Noise is continuous: a small step is a small change. A field that
    /// jumped would bake keyframes that strobe.
    #[test]
    fn the_field_is_smooth_under_a_small_step() {
        let noise = Noise::from_seed(99);
        for step in 0..40 {
            let at = f64::from(step) * 0.41;
            let (x0, y0) = noise.drift(at, 0);
            let (x1, y1) = noise.drift(at + 0.02, 0);
            assert!(
                (x1 - x0).abs() < 0.2 && (y1 - y0).abs() < 0.2,
                "a 20 ms step moved the field from ({x0}, {y0}) to ({x1}, {y1})"
            );
        }
    }

    /// Octaves add detail without leaving the range, and more octaves are
    /// visibly more rugged: the curvature - the sum of second differences,
    /// which grows with the frequency content however the trend moves -
    /// of a three-octave field beats a one-octave one at the same scale.
    #[test]
    fn octaves_add_ruggedness_without_leaving_the_range() {
        let noise = Noise::from_seed(41);
        for step in 0..100 {
            let x = f64::from(step) * 0.23;
            for octaves in 1..=4 {
                let value = noise.fbm2(x, x * 0.7, octaves);
                assert!((-1.0..=1.0).contains(&value), "fbm out of range: {value}");
            }
        }
        let curvature = |octaves: u32| -> f64 {
            let mut total = 0.0;
            for step in 1..59 {
                let at = f64::from(step) * 0.1;
                let left = noise.fbm2(at - 0.1, (at - 0.1) * 0.7, octaves);
                let mid = noise.fbm2(at, at * 0.7, octaves);
                let right = noise.fbm2(at + 0.1, (at + 0.1) * 0.7, octaves);
                total += (right - 2.0 * mid + left).abs();
            }
            total
        };
        assert!(
            curvature(3) > curvature(1),
            "three octaves should be more rugged than one: {} vs {}",
            curvature(3),
            curvature(1)
        );
    }

    /// Channels sharing one seed never read the same place in the field, so
    /// a stack of layers on one seed wobbles out of step - the point of a
    /// channel number rather than a second seed.
    #[test]
    fn channels_of_one_field_are_decorrelated() {
        let noise = Noise::from_seed(2026);
        let mut agree = 0;
        for step in 0..40 {
            let at = f64::from(step) * 0.31;
            if noise.drift(at, 0) == noise.drift(at, 1) {
                agree += 1;
            }
        }
        assert!(agree <= 2, "two channels agreed {agree}/40 times");
    }

    /// The permutation is a permutation: 256 distinct indices, doubled - a
    /// shuffle that lost or duplicated a source would bias the field, and a
    /// biased field is a visible one.
    #[test]
    fn the_permutation_is_a_permutation() {
        let noise = Noise::from_seed(123_456);
        let mut seen = [false; 256];
        for index in 0..256 {
            seen[noise.perm[index] as usize] = true;
        }
        assert!(
            seen.iter().all(|slot| *slot),
            "a gradient index went missing"
        );
    }
}
