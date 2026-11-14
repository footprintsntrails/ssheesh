//! Checking a copied LevelDB store before rusty-leveldb opens it.
//!
//! rusty-leveldb reads a store the way LevelDB does when nobody asks it to be strict, and says
//! nothing about what it steps over: a table block that fails its checksum is skipped, a log
//! record that fails ends the log there (every record after it is dropped), a manifest record
//! that fails ends the manifest. What comes back can then be missing records, or hold an older
//! value (or a record since deleted) where the newest one was, with no sign of it. The library
//! also takes the sizes a file states on trust: a table that says a block is a terabyte long
//! gets a terabyte allocated, and that ends the process (an abort, which `catch_unwind` can't
//! stop).
//!
//! So the copy is checked here first, against LevelDB's file formats (`doc/table_format.md`,
//! `doc/log_format.md` and `db/version_edit.cc` in LevelDB's source), in everything the library
//! will go on to read:
//!
//! - `CURRENT` must be `MANIFEST-<number>` and a newline, naming a file in the folder. The
//!   library opens whatever path it names, outside the copy too.
//! - The manifest it names is read through. A record that fails its checksum, or isn't a
//!   well-formed version edit, means the list of tables can't be trusted: the store is damaged
//!   and isn't opened.
//! - Every table the manifest lists must be there and hold together: the magic number in its
//!   footer, index and meta-index blocks whole, every block they point at inside the file, in
//!   order and not overlapping, and none unpacking past [`MAX_BLOCK_BYTES`]. A table that
//!   doesn't was not written by LevelDB, or was made to trip a reader: damaged, not opened. A
//!   data block that fails its checksum is the damage a disk does. The library skips it, so the
//!   store is read without it and the damage is noted.
//! - Every log the library replays is read through as well, and a record that fails is noted.
//!
//! In logs and the manifest alike, a last record cut short by the end of the file is not
//! damage: it is a write the old app hadn't finished (it was still running, or was closed in
//! the middle of it), and LevelDB itself takes it that way. A record that is all there but fails
//! its checksum is damage, wherever it is, and so is one that only seems to run past the end
//! while whole records follow it (its length was what got damaged).
//!
//! What's noted ends up in [`LegacyError::ReadPastDamage`](super::LegacyError::ReadPastDamage),
//! and the import then counts as doubtful.

use super::MAX_VALUE_BYTES;
use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// A log (and a manifest, which is a log) is written in blocks of this size. A record never
/// crosses from one block into the next; it is split into fragments instead.
const LOG_BLOCK: usize = 32 * 1024;
/// A log record's header: checksum (4 bytes), length (2), type (1).
const LOG_HEADER: usize = 7;
/// A whole record, and the first, middle and last fragments of a split one.
const FULL: u8 = 1;
const FIRST: u8 = 2;
const MIDDLE: u8 = 3;
const LAST: u8 = 4;

/// A table ends in a footer: two block handles, padding, then the magic number.
const FOOTER: usize = 48;
const TABLE_MAGIC: [u8; 8] = 0xdb47_7524_8b80_fb57_u64.to_le_bytes();
/// Each block in a table is followed by its compression type (1 byte) and checksum (4).
const BLOCK_TRAILER: usize = 5;
/// The most one table block may unpack to. A block holds about 4 KB of records, or a single
/// bigger one, which can't be more than one origin's whole localStorage (10 MB in Chromium).
pub(crate) const MAX_BLOCK_BYTES: usize = 2 * MAX_VALUE_BYTES;
/// LevelDB's number of levels. The library keeps arrays of that length, indexed by the level a
/// manifest names.
const LEVELS: usize = 7;

/// Whether `name` is one of LevelDB's own database files: `CURRENT`, a manifest, a log or a
/// table. Not `LOCK` (held by a running web view; the library makes its own) and not the `LOG`
/// text files (they name folders on the user's disk).
pub(crate) fn is_database_file(name: &str) -> bool {
    if name == "CURRENT" {
        return true;
    }
    if let Some(number) = name.strip_prefix("MANIFEST-") {
        return is_file_number(number);
    }
    matches!(name.split_once('.'), Some((number, "log" | "ldb" | "sst")) if is_file_number(number))
}

fn is_file_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && text.parse::<u64>().is_ok()
}

/// Checks the LevelDB store copied into `dir` (see the module notes). `Err` says why it can't be
/// opened; `Ok` lists the damage the library will step over, and is empty for a whole store.
/// A table the manifest lists that is only there under LevelDB's older name, `.sst`, is renamed
/// to `.ldb`, the only name the library opens.
pub(crate) fn check(dir: &Path) -> Result<Vec<String>, String> {
    let manifest = current_manifest(dir)?;
    let version = read_manifest(&read(dir, &manifest)?).map_err(|e| format!("{manifest}: {e}"))?;
    let mut damage = Vec::new();
    for number in &version.tables {
        let name = format!("{number:06}.ldb");
        if !dir.join(&name).is_file() {
            let older = format!("{number:06}.sst");
            if !dir.join(&older).is_file() {
                return Err(format!("{name}, a table {manifest} lists, is missing"));
            }
            fs::rename(dir.join(&older), dir.join(&name)).map_err(|e| format!("{older}: {e}"))?;
        }
        let notes = check_table(&read(dir, &name)?).map_err(|e| format!("{name}: {e}"))?;
        damage.extend(notes.into_iter().map(|note| format!("{name}: {note}")));
    }
    // The library replays these and no others (`DB::recover`).
    for (number, name) in log_files(dir)? {
        if number >= version.log_number || number == version.prev_log_number {
            if let Some(fault) = read_log(&read(dir, &name)?).fault {
                damage.push(format!("{name}: {fault}"));
            }
        }
    }
    Ok(damage)
}

fn read(dir: &Path, name: &str) -> Result<Vec<u8>, String> {
    fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"))
}

/// The manifest `CURRENT` names, checked to be a plain file name of a file in `dir`.
fn current_manifest(dir: &Path) -> Result<String, String> {
    let current = fs::read(dir.join("CURRENT")).map_err(|_| "it has no CURRENT file".to_owned())?;
    current
        .strip_suffix(b"\n")
        .and_then(|name| std::str::from_utf8(name).ok())
        .filter(|name| name.strip_prefix("MANIFEST-").is_some_and(is_file_number))
        .filter(|name| dir.join(name).is_file())
        .map(str::to_owned)
        .ok_or_else(|| "CURRENT doesn't name a manifest in the folder".to_owned())
}

/// The numbered logs in `dir`, with their numbers.
fn log_files(dir: &Path) -> Result<Vec<(u64, String)>, String> {
    let mut logs = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let name = entry.map_err(|e| e.to_string())?.file_name().to_string_lossy().into_owned();
        let number = name.strip_suffix(".log").filter(|n| is_file_number(n)).and_then(|n| n.parse().ok());
        if let Some(number) = number {
            logs.push((number, name));
        }
    }
    logs.sort();
    Ok(logs)
}

/// What a manifest says the store is made of, as the library will take it.
#[derive(Debug, Default, PartialEq, Eq)]
struct Version {
    /// Logs numbered below this are already in tables, and aren't replayed.
    log_number: u64,
    /// An older log that is replayed all the same (0 for none).
    prev_log_number: u64,
    /// The tables of every level.
    tables: BTreeSet<u64>,
}

fn read_manifest(bytes: &[u8]) -> Result<Version, String> {
    let log = read_log(bytes);
    if let Some(fault) = log.fault {
        return Err(fault);
    }
    // As rusty-leveldb's `version_set::Builder` adds edits up: a file deleted from a level stays
    // deleted unless a later edit adds it to that level again.
    let mut added: [Vec<u64>; LEVELS] = Default::default();
    let mut deleted: [Vec<u64>; LEVELS] = Default::default();
    let mut version = Version::default();
    for record in &log.records {
        let edit = decode_edit(record).ok_or("a version edit in it doesn't hold together")?;
        if let Some(number) = edit.log_number {
            version.log_number = number;
        }
        if let Some(number) = edit.prev_log_number {
            version.prev_log_number = number;
        }
        for (level, number) in edit.deleted {
            deleted[level].push(number);
        }
        for (level, number) in edit.added {
            deleted[level].retain(|&d| d != number);
            added[level].push(number);
        }
    }
    for (added, deleted) in added.iter().zip(&deleted) {
        version.tables.extend(added.iter().filter(|n| !deleted.contains(n)));
    }
    Ok(version)
}

/// The parts of one version edit that say which files make up the store.
#[derive(Debug, Default)]
struct Edit {
    log_number: Option<u64>,
    prev_log_number: Option<u64>,
    deleted: Vec<(usize, u64)>,
    added: Vec<(usize, u64)>,
}

/// Decodes a version edit (`VersionEdit::EncodeTo`): tagged fields, each a number or a
/// length-prefixed string. `None` unless every field is known and every length fits inside the
/// record. The library would allocate whatever length it reads before looking.
fn decode_edit(record: &[u8]) -> Option<Edit> {
    let mut r = Reader(record);
    let mut edit = Edit::default();
    while !r.is_empty() {
        match r.varint()? {
            // The comparator's name.
            1 => {
                r.prefixed()?;
            }
            2 => edit.log_number = Some(r.varint()?),
            // The next file number, the last sequence number.
            3 | 4 => {
                r.varint()?;
            }
            // A compaction pointer: level, key.
            5 => {
                r.level()?;
                r.prefixed()?;
            }
            6 => edit.deleted.push((r.level()?, r.varint()?)),
            // A new file: level, number, size, smallest and largest key.
            7 => {
                let file = (r.level()?, r.varint()?);
                r.varint()?;
                r.prefixed()?;
                r.prefixed()?;
                edit.added.push(file);
            }
            9 => edit.prev_log_number = Some(r.varint()?),
            _ => return None,
        }
    }
    Some(edit)
}

/// A log read through as rusty-leveldb's `LogReader` reads it.
#[derive(Debug, Default)]
struct LogRead {
    /// The whole records, fragments joined, up to the first that fails.
    records: Vec<Vec<u8>>,
    /// What is wrong with the first record that fails, if one does. A last record cut short by
    /// the end of the file doesn't count (see the module notes).
    fault: Option<String>,
}

fn read_log(bytes: &[u8]) -> LogRead {
    let mut read = LogRead::default();
    let mut partial: Option<Vec<u8>> = None;
    let mut at = 0;
    read.fault = loop {
        let left = LOG_BLOCK - at % LOG_BLOCK;
        if left < LOG_HEADER {
            // The writer fills the end of a block too small for a header with zeros.
            at += left;
            continue;
        }
        let Some(header) = bytes.get(at..at + LOG_HEADER) else {
            // The end, or a header cut short by it.
            break None;
        };
        let length = usize::from(u16::from_le_bytes([header[4], header[5]]));
        let kind = header[6];
        if LOG_HEADER + length > left {
            break Some(format!("the record at byte {at} runs past the end of its block"));
        }
        let Some(payload) = bytes.get(at + LOG_HEADER..at + LOG_HEADER + length) else {
            // Cut short by the end of the file: a write that didn't finish, unless a whole
            // record follows, and the length was what got damaged.
            break whole_record_after(bytes, at).then(|| {
                format!("the record at byte {at} says it runs past the end of the file, but more follows")
            });
        };
        if unmask(le32(&header[..4])) != crc32c::crc32c_append(crc32c::crc32c(&[kind]), payload) {
            break Some(format!("the record at byte {at} fails its checksum"));
        }
        // The library joins whatever comes until a whole or last fragment, so a fragment out of
        // place would glue two records into one.
        match (kind, partial.take()) {
            (FULL, None) => read.records.push(payload.to_vec()),
            (FIRST, None) => partial = Some(payload.to_vec()),
            (MIDDLE, Some(mut record)) => {
                record.extend_from_slice(payload);
                partial = Some(record);
            }
            (LAST, Some(mut record)) => {
                record.extend_from_slice(payload);
                read.records.push(record);
            }
            _ => break Some(format!("the record at byte {at} doesn't follow on from the one before")),
        }
        at += LOG_HEADER + length;
    };
    read
}

/// Whether a record that passes its checksum starts anywhere in `bytes` after `at`. Only asked
/// of the stretch after a record that claims to run past the end of the file, which is less
/// than one block (a record never crosses one), so trying every byte is cheap.
fn whole_record_after(bytes: &[u8], at: usize) -> bool {
    (at + 1..bytes.len()).any(|p| {
        let left = LOG_BLOCK - p % LOG_BLOCK;
        let Some(header) = bytes.get(p..p + LOG_HEADER) else { return false };
        let length = usize::from(u16::from_le_bytes([header[4], header[5]]));
        let kind = header[6];
        if !(FULL..=LAST).contains(&kind) || left < LOG_HEADER + length {
            return false;
        }
        let Some(payload) = bytes.get(p + LOG_HEADER..p + LOG_HEADER + length) else { return false };
        unmask(le32(&header[..4])) == crc32c::crc32c_append(crc32c::crc32c(&[kind]), payload)
    })
}

/// Where a block is in a table file (`BlockHandle`): its offset and size, the trailer not
/// counted.
#[derive(Debug, Clone, Copy)]
struct Handle {
    offset: u64,
    size: u64,
}

/// What can be wrong with a table block.
#[derive(Debug)]
enum BlockFault {
    /// Its checksum doesn't match: a damaged block, which the library skips.
    Checksum,
    /// Anything else (outside the file, say): the table isn't one LevelDB wrote.
    Malformed(String),
}

/// Checks one table (see the module notes). `Err` for a table that doesn't hold together; `Ok`
/// lists its data blocks that fail their checksum.
fn check_table(table: &[u8]) -> Result<Vec<String>, String> {
    let Some(end) = table.len().checked_sub(FOOTER) else {
        return Err("too short to be a table".into());
    };
    let footer = &table[end..];
    if footer[FOOTER - TABLE_MAGIC.len()..] != TABLE_MAGIC {
        return Err("no table magic number at its end".into());
    }
    let mut r = Reader(&footer[..FOOTER - TABLE_MAGIC.len()]);
    let (Some(meta), Some(index)) = (r.handle(), r.handle()) else {
        return Err("its footer doesn't hold together".into());
    };

    // The meta-index names the filter block, which the library reads (unchecked) as it opens
    // the table. Its handle only has to point inside the file.
    let meta = whole_block(table, meta, end, "meta-index")?;
    for value in block_values(&meta)? {
        let handle = Reader(value).handle().ok_or("a meta-index entry isn't a block handle")?;
        bounds(handle, end)?;
    }

    let index = whole_block(table, index, end, "index")?;
    let mut damage = Vec::new();
    // LevelDB writes data blocks one after another, in the index's order. Holding the index to
    // that also bounds the work: no block can be listed twice.
    let mut next = 0;
    for value in block_values(&index)? {
        let handle = Reader(value).handle().ok_or("an index entry isn't a block handle")?;
        if handle.offset < next {
            return Err("its index lists blocks out of order".into());
        }
        match block_at(table, handle, end) {
            Ok((_, 0)) => {}
            // The library would allocate whatever size the block says it unpacks to.
            Ok((data, 1)) => {
                snappy_size(data)?;
            }
            Ok((_, kind)) => return Err(unknown_compression(kind)),
            Err(BlockFault::Checksum) => {
                damage.push(format!("the data block at byte {} fails its checksum", handle.offset));
            }
            Err(BlockFault::Malformed(e)) => return Err(e),
        }
        // `block_at` checked this fits before the footer.
        next = handle.offset + handle.size + BLOCK_TRAILER as u64;
    }
    Ok(damage)
}

/// A block the table can't be read without (the index or the meta-index), checked and unpacked.
fn whole_block<'a>(table: &'a [u8], handle: Handle, end: usize, what: &str) -> Result<Cow<'a, [u8]>, String> {
    match block_at(table, handle, end) {
        Ok((data, kind)) => unpack(data, kind),
        Err(BlockFault::Checksum) => Err(format!("its {what} block fails its checksum")),
        Err(BlockFault::Malformed(e)) => Err(e),
    }
}

/// Where `handle`'s block and trailer are in a table whose blocks end at `end`, if inside.
fn bounds(handle: Handle, end: usize) -> Result<(usize, usize), String> {
    let start = usize::try_from(handle.offset).ok();
    let size = usize::try_from(handle.size).ok();
    start
        .zip(size)
        .filter(|&(start, size)| {
            start.checked_add(size).and_then(|s| s.checked_add(BLOCK_TRAILER)).is_some_and(|s| s <= end)
        })
        .ok_or_else(|| {
            format!(
                "a block handle points outside the file ({} bytes at byte {})",
                handle.size, handle.offset
            )
        })
}

/// A block's stored bytes and compression type, checked against its checksum.
fn block_at(table: &[u8], handle: Handle, end: usize) -> Result<(&[u8], u8), BlockFault> {
    let (start, size) = bounds(handle, end).map_err(BlockFault::Malformed)?;
    let data = &table[start..start + size];
    let kind = table[start + size];
    let stored = le32(&table[start + size + 1..start + size + BLOCK_TRAILER]);
    if unmask(stored) != crc32c::crc32c_append(crc32c::crc32c(data), &[kind]) {
        return Err(BlockFault::Checksum);
    }
    Ok((data, kind))
}

/// A block's contents, unpacked. LevelDB writes, and the library reads, two kinds: none (0) and
/// Snappy (1).
fn unpack(data: &[u8], kind: u8) -> Result<Cow<'_, [u8]>, String> {
    match kind {
        0 => Ok(Cow::Borrowed(data)),
        1 => {
            snappy_size(data)?;
            let unpacked = snap::raw::Decoder::new().decompress_vec(data);
            unpacked.map(Cow::Owned).map_err(|e| format!("a block doesn't unpack: {e}"))
        }
        kind => Err(unknown_compression(kind)),
    }
}

fn unknown_compression(kind: u8) -> String {
    format!("a block is packed with compression {kind}, which this reader doesn't know")
}

/// What a Snappy block says it unpacks to, if that is no more than [`MAX_BLOCK_BYTES`].
fn snappy_size(data: &[u8]) -> Result<usize, String> {
    match snap::raw::decompress_len(data) {
        Ok(size) if size <= MAX_BLOCK_BYTES => Ok(size),
        Ok(size) => Err(format!("a block says it unpacks to {size} bytes")),
        Err(e) => Err(format!("a block doesn't unpack: {e}")),
    }
}

/// The values of a block's entries (`block_builder.cc`). Each entry is the length of the key it
/// shares with the entry before, the length of the rest of its key, the length of its value,
/// the rest of the key and the value. After the entries come the restart points, then their
/// count.
fn block_values(block: &[u8]) -> Result<Vec<&[u8]>, String> {
    let bad = || "a block's entries don't hold together".to_owned();
    let count_at = block.len().checked_sub(4).ok_or_else(bad)?;
    let restarts = le32(&block[count_at..]) as usize;
    let entries_end = restarts
        .checked_mul(4)
        .and_then(|n| count_at.checked_sub(n))
        .filter(|_| restarts > 0)
        .ok_or_else(bad)?;
    let mut r = Reader(&block[..entries_end]);
    let mut key_len = 0;
    let mut values = Vec::new();
    while !r.is_empty() {
        let (Some(shared), Some(rest), Some(value)) = (r.varint32(), r.varint32(), r.varint32()) else {
            return Err(bad());
        };
        if shared > key_len {
            return Err(bad());
        }
        r.take(rest).ok_or_else(bad)?;
        values.push(r.take(value).ok_or_else(bad)?);
        key_len = shared + rest;
    }
    Ok(values)
}

/// LevelDB stores checksums masked (`crc32c::Mask`), so that the checksum of data that holds
/// checksums doesn't come out the same as theirs.
fn unmask(masked: u32) -> u32 {
    masked.wrapping_sub(0xa282_ead8).rotate_left(15)
}

/// A little-endian `u32` from four bytes.
fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// Reads LevelDB's encodings off the front of a slice. Every read checks that it fits.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if n > self.0.len() {
            return None;
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Some(head)
    }

    /// A base-128 varint of up to 64 bits, low bits first.
    fn varint(&mut self) -> Option<u64> {
        let mut value = 0;
        for (i, &byte) in self.0.iter().enumerate().take(10) {
            let bits = u64::from(byte & 0x7f);
            if i == 9 && bits > 1 {
                return None;
            }
            value |= bits << (7 * i);
            if byte & 0x80 == 0 {
                self.0 = &self.0[i + 1..];
                return Some(value);
            }
        }
        None
    }

    /// A varint of up to 32 bits, as a length.
    fn varint32(&mut self) -> Option<usize> {
        u32::try_from(self.varint()?).ok().map(|n| n as usize)
    }

    /// A length-prefixed string.
    fn prefixed(&mut self) -> Option<&'a [u8]> {
        let len = usize::try_from(self.varint()?).ok()?;
        self.take(len)
    }

    fn level(&mut self) -> Option<usize> {
        usize::try_from(self.varint()?).ok().filter(|&level| level < LEVELS)
    }

    fn handle(&mut self) -> Option<Handle> {
        Some(Handle { offset: self.varint()?, size: self.varint()? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "tests/fx/windows-leveldb/Local/io.github.footprintsntrails.ssheesh/EBWebView/Default/Local Storage/leveldb",
        )
    }

    fn mask(crc: u32) -> u32 {
        crc.rotate_right(15).wrapping_add(0xa282_ead8)
    }

    fn varint_bytes(mut value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        while value >= 0x80 {
            out.push((value as u8) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
        out
    }

    /// A log as LevelDB's `log::Writer` writes one: records split into fragments at block ends,
    /// a block's last few bytes zeroed when no header fits.
    fn log_of(records: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for record in records {
            let mut rest = *record;
            let mut first = true;
            loop {
                let left = LOG_BLOCK - out.len() % LOG_BLOCK;
                if left < LOG_HEADER {
                    out.resize(out.len() + left, 0);
                    continue;
                }
                let n = rest.len().min(left - LOG_HEADER);
                let last = n == rest.len();
                let kind = match (first, last) {
                    (true, true) => FULL,
                    (true, false) => FIRST,
                    (false, false) => MIDDLE,
                    (false, true) => LAST,
                };
                let crc = crc32c::crc32c_append(crc32c::crc32c(&[kind]), &rest[..n]);
                out.extend_from_slice(&mask(crc).to_le_bytes());
                out.extend_from_slice(&(n as u16).to_le_bytes());
                out.push(kind);
                out.extend_from_slice(&rest[..n]);
                rest = &rest[n..];
                first = false;
                if last {
                    break;
                }
            }
        }
        out
    }

    #[test]
    fn only_leveldbs_own_files_are_database_files() {
        for name in ["CURRENT", "MANIFEST-000001", "000004.log", "000003.ldb", "000005.sst"] {
            assert!(is_database_file(name), "{name}");
        }
        for name in ["LOCK", "LOG", "LOG.old", "MANIFEST-", "MANIFEST-1a", "x.log", ".log", "000004.log.bak"]
        {
            assert!(!is_database_file(name), "{name}");
        }
        assert!(!is_database_file("+4.log"));
        assert!(!is_database_file("99999999999999999999999.ldb"), "past 64 bits");
    }

    #[test]
    fn logs_read_back_whole_and_split_records() {
        let big = vec![0x5a; 3 * LOG_BLOCK];
        let records: [&[u8]; 4] = [b"first", &big, &[], b"after the big one"];
        let log = log_of(&records);
        let read = read_log(&log);
        assert!(read.fault.is_none(), "{:?}", read.fault);
        assert_eq!(read.records, records.iter().map(|r| r.to_vec()).collect::<Vec<_>>());
        // A block that ends with fewer than seven bytes left gets zeros, and they're skipped.
        let filler = vec![1; LOG_BLOCK - LOG_HEADER - LOG_HEADER - 3];
        let read = read_log(&log_of(&[&filler, b"x", b"next block"]));
        assert!(read.fault.is_none());
        assert_eq!(read.records.len(), 3);
    }

    #[test]
    fn a_log_cut_short_is_unfinished_but_a_failed_record_is_damage() {
        let log = log_of(&[b"one", b"two", b"three"]);
        for cut in 0..log.len() {
            let read = read_log(&log[..cut]);
            assert!(read.fault.is_none(), "cut at {cut}: {:?}", read.fault);
        }
        // A flipped bit anywhere is damage, even in the last record: in its checksum, type or
        // data, or a length that comes out shorter. One that makes the last record seem to run
        // past the end looks just like a write cut short, and loses only what that cut would.
        let last = log.len() - (LOG_HEADER + b"three".len());
        for at in 0..log.len() {
            for bit in 0..8 {
                let mut bad = log.clone();
                bad[at] ^= 1 << bit;
                let read = read_log(&bad);
                if read.fault.is_none() {
                    assert!(at >= last, "flip of bit {bit} at {at} went unseen");
                    assert_eq!(read.records, [b"one".to_vec(), b"two".to_vec()], "flip of bit {bit} at {at}");
                }
            }
        }
    }

    #[test]
    fn fragments_out_of_place_are_damage() {
        // A first fragment, then a whole record: the library would glue them together.
        let split = log_of(&[&vec![7; LOG_BLOCK]]);
        let mut log = split[..LOG_BLOCK].to_vec();
        log.extend_from_slice(&log_of(&[b"whole"]));
        assert!(read_log(&log).fault.unwrap().contains("doesn't follow on"));
        // A last fragment with nothing before it.
        let last = split[LOG_BLOCK..].to_vec();
        assert!(read_log(&last).fault.is_some());
        // A first fragment and then the end: an unfinished write.
        assert!(read_log(&split[..LOG_BLOCK]).fault.is_none());
    }

    #[test]
    fn version_edits_must_fit_their_record() {
        // Log number 4, then a new file at level 0: number 3, size 9, keys "a" and "b".
        let edit = [2, 4, 7, 0, 3, 9, 1, b'a', 1, b'b'];
        let decoded = decode_edit(&edit).unwrap();
        assert_eq!(decoded.log_number, Some(4));
        assert_eq!(decoded.added, [(0, 3)]);
        assert!(decode_edit(&[7, 7, 3, 9, 1, b'a', 1, b'b']).is_none(), "level 7 doesn't exist");
        // A comparator name said to be 2^40 bytes long.
        assert!(decode_edit(&[1, 0x80, 0x80, 0x80, 0x80, 0x80, 0x20]).is_none());
        assert!(decode_edit(&[8, 0]).is_none(), "unknown tag");
        assert!(decode_edit(&[2]).is_none(), "cut short");
        assert!(decode_edit(&[2, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]).is_none());
    }

    #[test]
    fn a_manifest_adds_and_deletes_tables_level_by_level() {
        let new_file = |level: u8, number: u8| vec![7, level, number, 9, 1, b'a', 1, b'b'];
        let delete = |level: u8, number: u8| vec![6, level, number];
        let records = [
            [vec![2, 6], new_file(0, 3), new_file(1, 5)].concat(),
            [delete(0, 3), new_file(1, 8), vec![9, 2]].concat(),
        ];
        let records: Vec<&[u8]> = records.iter().map(Vec::as_slice).collect();
        let version = read_manifest(&log_of(&records)).unwrap();
        assert_eq!(version.log_number, 6);
        assert_eq!(version.prev_log_number, 2);
        assert_eq!(version.tables.into_iter().collect::<Vec<_>>(), [5, 8]);
    }

    #[test]
    fn the_fixture_store_is_whole() {
        let dir = fixture_dir();
        let manifest = read_manifest(&fs::read(dir.join("MANIFEST-000001")).unwrap()).unwrap();
        assert_eq!(manifest.tables.iter().copied().collect::<Vec<_>>(), [3]);
        assert_eq!(manifest.log_number, 4);
        assert_eq!(check_table(&fs::read(dir.join("000003.ldb")).unwrap()).unwrap(), Vec::<String>::new());
        let log = read_log(&fs::read(dir.join("000004.log")).unwrap());
        assert!(log.fault.is_none());
        assert!(!log.records.is_empty());
    }

    #[test]
    fn a_table_lying_about_its_blocks_is_refused() {
        let table = fs::read(fixture_dir().join("000003.ldb")).unwrap();
        let footer = table.len() - FOOTER;
        let mut r = Reader(&table[footer..]);
        let (meta, index) = (r.handle().unwrap(), r.handle().unwrap());
        // The index block said to be a terabyte long: the library would try to allocate it.
        let mut handles = [meta.offset, meta.size, index.offset, 1 << 40].map(varint_bytes).concat();
        handles.resize(FOOTER - TABLE_MAGIC.len(), 0);
        let mut lying = table.clone();
        lying[footer..footer + handles.len()].copy_from_slice(&handles);
        let error = check_table(&lying).unwrap_err();
        assert!(error.contains("outside the file"), "{error}");

        let mut no_magic = table.clone();
        let last = no_magic.len() - 1;
        no_magic[last] ^= 1;
        assert!(check_table(&no_magic).unwrap_err().contains("magic"));
        assert!(check_table(&table[..40]).is_err());
    }

    #[test]
    fn a_damaged_data_block_is_noted_and_the_rest_read() {
        let table = fs::read(fixture_dir().join("000003.ldb")).unwrap();
        let mut damaged = table.clone();
        damaged[100] ^= 0x10;
        let notes = check_table(&damaged).unwrap();
        assert_eq!(notes, ["the data block at byte 0 fails its checksum"]);
    }
}
