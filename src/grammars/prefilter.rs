use fancy_regex::ByteSet;

fn bits_of(mut bits: u64) -> impl Iterator<Item = usize> {
    core::iter::from_fn(move || {
        (bits != 0).then(|| {
            let bit = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            bit
        })
    })
}

pub(crate) fn set_bits(words: &[u64]) -> impl Iterator<Item = usize> + '_ {
    words
        .iter()
        .enumerate()
        .flat_map(|(i, &word)| bits_of(word).map(move |bit| i * 64 + bit))
}

#[derive(PartialEq, Debug, Default)]
pub struct Prefilter {
    /// Matrix of 256 rows x number of u64 needed to handle every bit that needs to be set
    table: Vec<u64>,
    /// Number of u64 per row of `table`
    stride: usize,
    /// Union of every pattern byte set: does byte i has a pattern matching?
    quick_lookup: ByteSet,
}

impl Prefilter {
    /// Builds a prefilter from the given list of [ByteSet]
    /// If there are no actual ByteSet, returns None.
    /// Otherwise returns the built prefilter along with the indices that are NOT contained
    /// in the prefilter.
    pub(crate) fn from_byte_sets(sets: &[Option<ByteSet>]) -> Option<(Self, Vec<usize>)> {
        if sets.is_empty() {
            return None;
        }

        let stride = sets.len().div_ceil(64);
        let mut table = vec![0u64; 256 * stride];
        let mut quick_lookup = ByteSet::default();
        let mut unqualified = Vec::new();
        let mut something_qualified = false;

        for (idx, set) in sets.iter().enumerate() {
            let set = match set.as_ref() {
                Some(set) => set,
                None => {
                    unqualified.push(idx);
                    continue;
                }
            };
            something_qualified = true;
            quick_lookup.union(set);

            let (word_idx, bit) = (idx / 64, 1u64 << (idx % 64));
            for b in set.iter() {
                table[b as usize * stride + word_idx] |= bit;
            }
        }

        if !something_qualified {
            return None;
        }

        Some((
            Self {
                table,
                stride,
                quick_lookup,
            },
            unqualified,
        ))
    }

    /// Whether any pattern in the prefilter can start with that byte
    #[inline]
    pub(crate) fn may_match_at(&self, byte: u8) -> bool {
        self.quick_lookup.contains(byte)
    }

    /// The pattern indices this byte can match, in ascending order
    #[inline]
    pub(crate) fn candidates(&self, byte: u8) -> impl Iterator<Item = usize> + '_ {
        let start = byte as usize * self.stride;
        set_bits(&self.table[start..start + self.stride])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammars::engine::fancy_options;

    fn byte_sets(patterns: &[&str]) -> Vec<Option<ByteSet>> {
        patterns
            .iter()
            .map(|p| fancy_options().start_bytes(p).unwrap())
            .collect()
    }

    #[test]
    fn can_build_prefilter() {
        // `.` has no start bytes so it stays out of the table
        let sets = byte_sets(&["fn", "let|impl", ".", r"\{"]);
        let (prefilter, unqualified) = Prefilter::from_byte_sets(&sets).unwrap();
        assert_eq!(unqualified, vec![2]);

        assert!(prefilter.may_match_at(b'f'));
        assert!(prefilter.may_match_at(b'l'));
        assert!(!prefilter.may_match_at(b'x'));

        assert_eq!(prefilter.candidates(b'f').collect::<Vec<_>>(), vec![0]);
        assert_eq!(prefilter.candidates(b'i').collect::<Vec<_>>(), vec![1]);
        assert_eq!(prefilter.candidates(b'{').collect::<Vec<_>>(), vec![3]);
        assert!(prefilter.candidates(b'x').next().is_none());
    }

    #[test]
    fn prefilter_handles_more_than_one_word_per_row() {
        // 70 patterns means 2 u64 per row, so the last ones live in the second word
        let mut patterns: Vec<String> = (0..70).map(|i| format!("a{i}")).collect();
        patterns[69] = "z".to_string();
        let sets: Vec<_> = patterns
            .iter()
            .map(|p| fancy_options().start_bytes(p).unwrap())
            .collect();
        let (prefilter, unqualified) = Prefilter::from_byte_sets(&sets).unwrap();

        assert!(unqualified.is_empty());
        assert_eq!(prefilter.candidates(b'z').collect::<Vec<_>>(), vec![69]);
        assert_eq!(prefilter.candidates(b'a').count(), 69);
    }

    #[test]
    fn no_prefilter_when_nothing_qualifies() {
        assert!(Prefilter::from_byte_sets(&[]).is_none());
        assert!(Prefilter::from_byte_sets(&byte_sets(&[".", "a?"])).is_none());
    }
}
