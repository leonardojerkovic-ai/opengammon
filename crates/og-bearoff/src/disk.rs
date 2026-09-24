//! On-disk format for the one-sided bearoff database, and reading it from a raw byte
//! slice — deliberately platform-agnostic: [`BearoffData`] only ever sees `&[u8]`, not
//! a file or a mmap, so it works identically whether those bytes came from `mmap` (see
//! the `native` submodule, native-only, behind the `mmap` feature) or were loaded into
//! memory some other way (e.g. fetched in a browser and handed in as a WASM-side
//! `Vec<u8>`/`&[u8]`). See `docs/rules-notes.md` for why the split matters:
//! `memmap2` cannot be an unconditional dependency without breaking the
//! `wasm32-unknown-unknown` build this crate is required to keep working.
//!
//! # Format (version 1), all multi-byte integers little-endian
//!
//! ```text
//! Header (20 bytes):
//!   0..4   magic: b"OGB1"
//!   4..6   version: u16          (1)
//!   6      points: u8            (og_bearoff::one_sided::POINTS)
//!   7      max_checkers: u8      (og_bearoff::one_sided::MAX_CHECKERS)
//!   8..10  max_finish_len: u16   (measured, not assumed, by the writer)
//!   10..12 max_first_off_len: u16 (measured, not assumed, by the writer)
//!   12..16 finish_count: u32     (= combinatorial::count(points, max_checkers))
//!   16..20 first_off_count: u32  (= combinatorial::count(points - 1, max_checkers))
//!
//! Finish section (finish_count * max_finish_len * 2 bytes), immediately after the
//! header: finish_count fixed-size records, each max_finish_len quantized u16
//! probabilities (see quantize.rs), zero-padded past a position's real support.
//! Record for `combinatorial::rank(max_checkers, checkers)` starts at byte
//! `20 + rank * max_finish_len * 2`.
//!
//! First-off section (first_off_count * max_first_off_len * 2 bytes), immediately
//! after the finish section: first_off_count fixed-size records, same shape, but
//! indexed by the *dense* off == 0 subset — `combinatorial::rank(max_checkers,
//! checkers[..points - 1])` (the last point's count is always the remainder, so the
//! first `points - 1` counts alone already uniquely and densely index this subset;
//! no second indexing scheme needed). A position with `off > 0` has no record here at
//! all: the value is a known constant ("already saved") that isn't stored — see
//! `one_sided::Entry::first_off`'s doc.
//! ```

use std::io::{self, Write};

use crate::combinatorial;
use crate::one_sided::{Entry, MAX_CHECKERS, POINTS};
use crate::quantize;

const MAGIC: [u8; 4] = *b"OGB1";
const FORMAT_VERSION: u16 = 1;
const HEADER_LEN: usize = 20;

/// A parsed, validated file header. Every field a reader needs to interpret the file
/// is read from the file itself, not assumed to match this crate's current constants
/// — see [`BearoffData::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub points: u8,
    pub max_checkers: u8,
    pub max_finish_len: u16,
    pub max_first_off_len: u16,
    pub finish_count: u32,
    pub first_off_count: u32,
}

/// Why a byte slice couldn't be parsed as a bearoff database file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatError {
    TooShortForHeader {
        len: usize,
    },
    BadMagic,
    UnsupportedVersion {
        found: u16,
        supported: u16,
    },
    ShapeMismatch {
        expected_points: u8,
        expected_max_checkers: u8,
        found_points: u8,
        found_max_checkers: u8,
    },
    Truncated {
        expected_len: usize,
        actual_len: usize,
    },
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FormatError::TooShortForHeader { len } => {
                write!(
                    f,
                    "file is only {len} bytes, shorter than the {HEADER_LEN}-byte header"
                )
            }
            FormatError::BadMagic => write!(f, "missing or wrong magic bytes (expected {MAGIC:?})"),
            FormatError::UnsupportedVersion { found, supported } => {
                write!(
                    f,
                    "format version {found} not supported (this build supports {supported})"
                )
            }
            FormatError::ShapeMismatch {
                expected_points,
                expected_max_checkers,
                found_points,
                found_max_checkers,
            } => write!(
                f,
                "file is for {found_points} points / {found_max_checkers} checkers, \
                 this build expects {expected_points} points / {expected_max_checkers} checkers"
            ),
            FormatError::Truncated {
                expected_len,
                actual_len,
            } => write!(
                f,
                "file is {actual_len} bytes, expected at least {expected_len} per its own header"
            ),
        }
    }
}

impl std::error::Error for FormatError {}

/// Writes the full bearoff table to `writer` in the format this module documents.
/// `table` must be indexed by `combinatorial::rank(MAX_CHECKERS, checkers)`, as
/// returned by [`crate::one_sided::compute_table`].
pub fn write_table<W: Write>(table: &[Entry], writer: &mut W) -> io::Result<()> {
    let max_finish_len = table.iter().map(|e| e.finish.len()).max().unwrap_or(0);
    let max_first_off_len = table.iter().map(|e| e.first_off.len()).max().unwrap_or(0);
    let first_off_count = combinatorial::count(POINTS - 1, MAX_CHECKERS);

    writer.write_all(&MAGIC)?;
    writer.write_all(&FORMAT_VERSION.to_le_bytes())?;
    writer.write_all(&[POINTS as u8, MAX_CHECKERS])?;
    writer.write_all(&(max_finish_len as u16).to_le_bytes())?;
    writer.write_all(&(max_first_off_len as u16).to_le_bytes())?;
    writer.write_all(&(table.len() as u32).to_le_bytes())?;
    writer.write_all(&(first_off_count as u32).to_le_bytes())?;

    for entry in table {
        write_padded_record(writer, &quantize::quantize(&entry.finish), max_finish_len)?;
    }

    for dense_index in 0..first_off_count {
        let prefix: [u8; POINTS - 1] = combinatorial::unrank(MAX_CHECKERS, dense_index);
        let mut checkers = [0u8; POINTS];
        checkers[..POINTS - 1].copy_from_slice(&prefix);
        let prefix_sum: u32 = prefix.iter().map(|&c| c as u32).sum();
        checkers[POINTS - 1] = (MAX_CHECKERS as u32 - prefix_sum) as u8;

        let rank = combinatorial::rank(MAX_CHECKERS, checkers);
        let entry = &table[rank];
        write_padded_record(
            writer,
            &quantize::quantize(&entry.first_off),
            max_first_off_len,
        )?;
    }

    Ok(())
}

fn write_padded_record<W: Write>(
    writer: &mut W,
    quantized: &[u16],
    target_len: usize,
) -> io::Result<()> {
    for i in 0..target_len {
        writer.write_all(&quantized.get(i).copied().unwrap_or(0).to_le_bytes())?;
    }
    Ok(())
}

/// A parsed bearoff database, borrowed from a byte slice. Doesn't know or care where
/// the bytes came from — see the module doc.
#[derive(Debug)]
pub struct BearoffData<'a> {
    header: Header,
    bytes: &'a [u8],
}

impl<'a> BearoffData<'a> {
    /// Validates `bytes` as a bearoff database file for this build's `POINTS` /
    /// `MAX_CHECKERS`, and returns a view over it. Checks the header self-consistently
    /// (magic, version, shape, declared length against the slice's actual length)
    /// before any lookup trusts an offset into it.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, FormatError> {
        if bytes.len() < HEADER_LEN {
            return Err(FormatError::TooShortForHeader { len: bytes.len() });
        }
        if bytes[0..4] != MAGIC {
            return Err(FormatError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != FORMAT_VERSION {
            return Err(FormatError::UnsupportedVersion {
                found: version,
                supported: FORMAT_VERSION,
            });
        }
        let points = bytes[6];
        let max_checkers = bytes[7];
        if points as usize != POINTS || max_checkers != MAX_CHECKERS {
            return Err(FormatError::ShapeMismatch {
                expected_points: POINTS as u8,
                expected_max_checkers: MAX_CHECKERS,
                found_points: points,
                found_max_checkers: max_checkers,
            });
        }
        let max_finish_len = u16::from_le_bytes([bytes[8], bytes[9]]);
        let max_first_off_len = u16::from_le_bytes([bytes[10], bytes[11]]);
        let finish_count = u32::from_le_bytes(bytes[12..16].try_into().expect("4-byte slice"));
        let first_off_count = u32::from_le_bytes(bytes[16..20].try_into().expect("4-byte slice"));

        let expected_len = HEADER_LEN
            + finish_count as usize * max_finish_len as usize * 2
            + first_off_count as usize * max_first_off_len as usize * 2;
        if bytes.len() < expected_len {
            return Err(FormatError::Truncated {
                expected_len,
                actual_len: bytes.len(),
            });
        }

        Ok(BearoffData {
            header: Header {
                points,
                max_checkers,
                max_finish_len,
                max_first_off_len,
                finish_count,
                first_off_count,
            },
            bytes,
        })
    }

    pub fn header(&self) -> Header {
        self.header
    }

    /// The `finish` distribution for `checkers`, dequantized. Zero-padded past the
    /// position's real support (a trailing zero is a valid, correct probability, not
    /// missing data).
    pub fn finish(&self, checkers: [u8; POINTS]) -> Vec<f64> {
        let rank = combinatorial::rank(self.header.max_checkers, checkers);
        let record_len = self.header.max_finish_len as usize * 2;
        let start = HEADER_LEN + rank * record_len;
        decode_record(&self.bytes[start..start + record_len])
    }

    /// The `first_off` distribution for `checkers`, dequantized — or `None` if
    /// `off > 0` (checkers don't sum to `max_checkers`), where the value is the known
    /// constant "already saved" rather than a stored record. See the module doc.
    pub fn first_off(&self, checkers: [u8; POINTS]) -> Option<Vec<f64>> {
        let total: u32 = checkers.iter().map(|&c| c as u32).sum();
        if total != self.header.max_checkers as u32 {
            return None;
        }
        let prefix: [u8; POINTS - 1] = checkers[..POINTS - 1]
            .try_into()
            .expect("POINTS - 1 elements");
        let dense_index = combinatorial::rank(self.header.max_checkers, prefix);
        let finish_section_len =
            self.header.finish_count as usize * self.header.max_finish_len as usize * 2;
        let record_len = self.header.max_first_off_len as usize * 2;
        let start = HEADER_LEN + finish_section_len + dense_index * record_len;
        Some(decode_record(&self.bytes[start..start + record_len]))
    }
}

fn decode_record(bytes: &[u8]) -> Vec<f64> {
    let (pairs, _) = bytes.as_chunks::<2>();
    let values: Vec<u16> = pairs.iter().map(|&pair| u16::from_le_bytes(pair)).collect();
    quantize::dequantize(&values)
}

/// Native, memory-mapped file access. Not available on `wasm32-unknown-unknown` (no
/// filesystem in the browser) — gated behind the `mmap` Cargo feature, which is not a
/// default feature, so a plain `cargo build --target wasm32-unknown-unknown` never
/// needs to know this module exists.
#[cfg(feature = "mmap")]
pub mod native {
    use std::fs::File;
    use std::path::Path;
    use std::{fmt, io, ops::Deref};

    use memmap2::Mmap;

    use super::{BearoffData, FormatError};

    /// Why [`MappedBearoffFile::open`] failed: either the file couldn't be opened /
    /// mapped, or it opened fine but isn't a valid bearoff database file.
    #[derive(Debug)]
    pub enum OpenError {
        Io(io::Error),
        Format(FormatError),
    }

    impl fmt::Display for OpenError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                OpenError::Io(e) => write!(f, "{e}"),
                OpenError::Format(e) => write!(f, "{e}"),
            }
        }
    }

    impl std::error::Error for OpenError {}

    impl From<io::Error> for OpenError {
        fn from(e: io::Error) -> Self {
            OpenError::Io(e)
        }
    }

    /// A bearoff database file, memory-mapped read-only.
    pub struct MappedBearoffFile {
        mmap: Mmap,
    }

    impl MappedBearoffFile {
        /// Opens and maps `path`, validating the header immediately (see
        /// [`BearoffData::parse`]) rather than deferring a format error to first
        /// lookup — a caller that successfully opens one of these has a definite
        /// guarantee `data()` will too.
        ///
        /// # Safety-adjacent note
        /// `memmap2::Mmap::map` is `unsafe` because the OS gives no guarantee the
        /// backing file isn't mutated (by another process, or truncated) while mapped,
        /// which would be undefined behavior to read through the mapping. This is
        /// treated as an accepted risk here, not proven away: the file this crate
        /// writes is a build artifact meant to be written once and then only read, by
        /// possibly many readers, never mutated in place while a reader might hold it
        /// mapped.
        pub fn open(path: impl AsRef<Path>) -> Result<Self, OpenError> {
            let file = File::open(path)?;
            let mmap = unsafe { Mmap::map(&file)? };
            BearoffData::parse(&mmap).map_err(OpenError::Format)?;
            Ok(MappedBearoffFile { mmap })
        }

        /// Never fails: [`open`](Self::open) already proved the mapped bytes parse.
        pub fn data(&self) -> BearoffData<'_> {
            BearoffData::parse(&self.mmap).expect("open() already validated this file's header")
        }
    }

    impl Deref for MappedBearoffFile {
        type Target = [u8];

        fn deref(&self) -> &[u8] {
            &self.mmap
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::one_sided;

    fn small_table() -> Vec<Entry> {
        one_sided::compute_table()
    }

    #[test]
    fn round_trips_a_few_hand_picked_positions() {
        let table = small_table();
        let mut bytes = Vec::new();
        write_table(&table, &mut bytes).unwrap();
        let data = BearoffData::parse(&bytes).unwrap();

        let cases: [[u8; POINTS]; 3] =
            [[0, 0, 0, 0, 0, 15], [1, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0]];
        for checkers in cases {
            let rank = combinatorial::rank(MAX_CHECKERS, checkers);
            let expected = &table[rank];
            let expected_finish = quantize::dequantize(&quantize::quantize(&expected.finish));
            let actual_finish = data.finish(checkers);
            assert_eq!(
                actual_finish.len(),
                expected_finish.len().max(actual_finish.len())
            );
            for (i, &e) in expected_finish.iter().enumerate() {
                assert_eq!(actual_finish[i], e, "finish[{i}] for {checkers:?}");
            }
            for &v in &actual_finish[expected_finish.len()..] {
                assert_eq!(v, 0.0, "padding past real support should dequantize to 0");
            }
        }
    }

    #[test]
    fn first_off_is_none_for_off_greater_than_zero() {
        let table = small_table();
        let mut bytes = Vec::new();
        write_table(&table, &mut bytes).unwrap();
        let data = BearoffData::parse(&bytes).unwrap();

        assert!(data.first_off([1, 0, 0, 0, 0, 0]).is_none());
        assert!(data.first_off([0; POINTS]).is_none());
    }

    #[test]
    fn first_off_round_trips_for_off_zero_positions() {
        let table = small_table();
        let mut bytes = Vec::new();
        write_table(&table, &mut bytes).unwrap();
        let data = BearoffData::parse(&bytes).unwrap();

        let cases: [[u8; POINTS]; 2] = [[0, 0, 0, 0, 0, 15], [2, 2, 2, 2, 2, 5]];
        for checkers in cases {
            let rank = combinatorial::rank(MAX_CHECKERS, checkers);
            let expected = quantize::dequantize(&quantize::quantize(&table[rank].first_off));
            let actual = data.first_off(checkers).unwrap();
            for (i, &e) in expected.iter().enumerate() {
                assert_eq!(actual[i], e, "first_off[{i}] for {checkers:?}");
            }
        }
    }

    #[test]
    fn parse_rejects_bad_magic() {
        let mut bytes = vec![0u8; HEADER_LEN];
        bytes[0..4].copy_from_slice(b"NOPE");
        assert_eq!(
            BearoffData::parse(&bytes).unwrap_err(),
            FormatError::BadMagic
        );
    }

    #[test]
    fn parse_rejects_too_short_a_slice() {
        let bytes = vec![0u8; HEADER_LEN - 1];
        assert_eq!(
            BearoffData::parse(&bytes).unwrap_err(),
            FormatError::TooShortForHeader {
                len: HEADER_LEN - 1
            }
        );
    }

    #[test]
    fn parse_rejects_unsupported_version() {
        let mut bytes = vec![0u8; HEADER_LEN];
        bytes[0..4].copy_from_slice(&MAGIC);
        bytes[4..6].copy_from_slice(&99u16.to_le_bytes());
        assert_eq!(
            BearoffData::parse(&bytes).unwrap_err(),
            FormatError::UnsupportedVersion {
                found: 99,
                supported: FORMAT_VERSION
            }
        );
    }

    #[test]
    fn parse_rejects_truncated_data() {
        let table = small_table();
        let mut bytes = Vec::new();
        write_table(&table, &mut bytes).unwrap();
        bytes.truncate(bytes.len() - 1);
        assert!(matches!(
            BearoffData::parse(&bytes),
            Err(FormatError::Truncated { .. })
        ));
    }

    #[test]
    fn round_trips_exhaustively_against_the_full_table() {
        // Exhaustive, not sampled: 54,264 positions is small enough to check all of
        // them, same reasoning as combinatorial.rs's and quantize.rs's own exhaustive
        // tests.
        let table = small_table();
        let mut bytes = Vec::new();
        write_table(&table, &mut bytes).unwrap();
        let data = BearoffData::parse(&bytes).unwrap();

        for (rank, entry) in table.iter().enumerate() {
            let checkers: [u8; POINTS] = combinatorial::unrank(MAX_CHECKERS, rank);
            let expected_finish = quantize::dequantize(&quantize::quantize(&entry.finish));
            let actual_finish = data.finish(checkers);
            for (i, &e) in expected_finish.iter().enumerate() {
                assert_eq!(
                    actual_finish[i], e,
                    "finish[{i}] at rank {rank} {checkers:?}"
                );
            }

            let total: u32 = checkers.iter().map(|&c| c as u32).sum();
            if total == MAX_CHECKERS as u32 {
                let expected_first_off =
                    quantize::dequantize(&quantize::quantize(&entry.first_off));
                let actual_first_off = data.first_off(checkers).unwrap();
                for (i, &e) in expected_first_off.iter().enumerate() {
                    assert_eq!(
                        actual_first_off[i], e,
                        "first_off[{i}] at rank {rank} {checkers:?}"
                    );
                }
            } else {
                assert!(data.first_off(checkers).is_none());
            }
        }
    }

    #[cfg(feature = "mmap")]
    #[test]
    fn mapped_file_round_trips_through_a_real_file() {
        use super::native::MappedBearoffFile;

        let table = small_table();
        let mut bytes = Vec::new();
        write_table(&table, &mut bytes).unwrap();

        let path =
            std::env::temp_dir().join(format!("og_bearoff_disk_test_{}.bin", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();

        let mapped = MappedBearoffFile::open(&path).unwrap();
        let data = mapped.data();

        let checkers = [0u8, 0, 0, 0, 0, 15];
        let rank = combinatorial::rank(MAX_CHECKERS, checkers);
        let expected = quantize::dequantize(&quantize::quantize(&table[rank].finish));
        let actual = data.finish(checkers);
        assert_eq!(actual, expected);

        drop(mapped);
        std::fs::remove_file(&path).unwrap();
    }

    #[cfg(feature = "mmap")]
    #[test]
    fn mapped_file_open_rejects_a_bad_file() {
        use super::native::{MappedBearoffFile, OpenError};

        let path = std::env::temp_dir().join(format!(
            "og_bearoff_disk_test_bad_{}.bin",
            std::process::id()
        ));
        std::fs::write(&path, b"not a bearoff file, too short").unwrap();

        let result = MappedBearoffFile::open(&path);
        assert!(matches!(result, Err(OpenError::Format(_))));

        std::fs::remove_file(&path).unwrap();
    }
}
