/// bfile.rs — Btrieve .B flat-file reader.
///
/// Supports Btrieve 6.x (v6) and Btrieve 5.x (v5) file formats.
///
/// The file is page-based.  Page 0 is the FCR (File Control Record).
/// Data pages contain fixed-stride records; each record slot starts with a
/// 2-byte usage count (v6) or is tracked via a deleted-chain (v5).
/// A usage count of 0 means the slot is deleted/empty.
///
/// Reference: mbbsemu/wbtrv32, Btrieve Programmer's Reference Manual.
use std::path::Path;

/// Parsed File Control Record.
#[derive(Debug, Clone)]
pub struct FileHeader {
    pub version: u8, // 5 or 6
    pub page_size: u32,
    pub logical_rec_len: u32,
    pub physical_rec_len: u32,
    pub key_count: u16,
    pub record_count: u32,
    pub page_count: u32,
}

pub struct BtrieveFile {
    pub header: FileHeader,
    data: Vec<u8>,
}

// ── V6 FCR offsets ─────────────────────────────────────────────────────────────
const V6_MAGIC: [u8; 2] = [b'F', b'C'];
const V6_PAGE_SIZE: usize = 0x08; // u16
const V6_KEY_COUNT: usize = 0x14; // u16
const V6_LOGICAL_RECLEN: usize = 0x16; // u16
const V6_PHYS_RECLEN: usize = 0x18; // u16
const V6_REC_COUNT: usize = 0x1A; // u24 (3 bytes)
const V6_PAGE_COUNT: usize = 0x26; // u24 (3 bytes)

// ── V5 FCR offsets ─────────────────────────────────────────────────────────────
// v5 has no "FC" magic; version code is at 0x06 (values 3, 4, or 5).
const V5_VERSION: usize = 0x06; // u16; 3/4/5 = Btrieve 5.x
const V5_PAGE_SIZE: usize = 0x08; // u16
const V5_LOGICAL_RECLEN: usize = 0x0E; // u16
const V5_PHYS_RECLEN: usize = 0x10; // u16
const V5_KEY_COUNT: usize = 0x12; // u16
const V5_REC_COUNT: usize = 0x1C; // u32
const V5_PAGE_COUNT: usize = 0x14; // u16
                                   // v5 deleted-chain starts at: FCR offset 0x10 (u32 file offset of first deleted record)
const V5_DELETED_CHAIN: usize = 0x10; // u32

fn le16(data: &[u8], off: usize) -> u32 {
    if off + 2 > data.len() {
        return 0;
    }
    u16::from_le_bytes([data[off], data[off + 1]]) as u32
}

fn le32(data: &[u8], off: usize) -> u32 {
    if off + 4 > data.len() {
        return 0;
    }
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

fn u24(data: &[u8], off: usize) -> u32 {
    if off + 3 > data.len() {
        return 0;
    }
    data[off] as u32 | ((data[off + 1] as u32) << 8) | ((data[off + 2] as u32) << 16)
}

impl BtrieveFile {
    pub fn open(path: &Path) -> Result<Self, String> {
        let data =
            std::fs::read(path).map_err(|e| format!("cannot read {}: {}", path.display(), e))?;

        if data.len() < 512 {
            return Err(format!(
                "{}: file too small to be a valid .B file",
                path.display()
            ));
        }

        // Detect version from magic bytes
        let is_v6 = data[0] == V6_MAGIC[0] && data[1] == V6_MAGIC[1];

        let header = if is_v6 {
            let page_size = le16(&data, V6_PAGE_SIZE).max(512);
            FileHeader {
                version: 6,
                page_size,
                logical_rec_len: le16(&data, V6_LOGICAL_RECLEN),
                physical_rec_len: le16(&data, V6_PHYS_RECLEN),
                key_count: le16(&data, V6_KEY_COUNT) as u16,
                record_count: u24(&data, V6_REC_COUNT),
                page_count: u24(&data, V6_PAGE_COUNT),
            }
        } else {
            // v5: check version code at 0x06
            let ver = le16(&data, V5_VERSION) as u8;
            if ver == 0 || ver > 5 {
                return Err(format!(
                    "{}: unrecognized Btrieve file format (magic={:#04x}{:#04x}, ver_code={})",
                    path.display(),
                    data[0],
                    data[1],
                    ver
                ));
            }
            let page_size = le16(&data, V5_PAGE_SIZE).max(512);
            FileHeader {
                version: 5,
                page_size,
                logical_rec_len: le16(&data, V5_LOGICAL_RECLEN),
                physical_rec_len: le16(&data, V5_PHYS_RECLEN),
                key_count: le16(&data, V5_KEY_COUNT) as u16,
                record_count: le32(&data, V5_REC_COUNT),
                page_count: le16(&data, V5_PAGE_COUNT),
            }
        };

        if header.logical_rec_len == 0 {
            return Err(format!(
                "{}: logical record length is 0 in FCR",
                path.display()
            ));
        }
        if header.physical_rec_len < header.logical_rec_len {
            return Err(format!(
                "{}: physical_rec_len ({}) < logical_rec_len ({})",
                path.display(),
                header.physical_rec_len,
                header.logical_rec_len
            ));
        }

        Ok(Self { header, data })
    }

    /// Iterate all active (non-deleted) records in the file.
    /// Yields raw logical record bytes (exactly `header.logical_rec_len` bytes each).
    pub fn records(&self) -> RecordIter<'_> {
        if self.header.version == 5 {
            RecordIter::new_v5(self)
        } else {
            RecordIter::new_v6(self)
        }
    }
}

// ── Record iterator ────────────────────────────────────────────────────────────

pub struct RecordIter<'a> {
    file: &'a BtrieveFile,
    page_num: usize,
    slot_idx: usize,
    total_pages: usize,
    slots_pp: usize,
    // v5: set of deleted record file-offsets (built once)
    v5_deleted: std::collections::HashSet<u64>,
    yielded: u32,
}

impl<'a> RecordIter<'a> {
    fn new_v6(file: &'a BtrieveFile) -> Self {
        let h = &file.header;
        let slots_pp = if h.physical_rec_len > 0 && h.page_size > 6 {
            ((h.page_size - 6) / h.physical_rec_len) as usize
        } else {
            0
        };
        let total_pages = file.data.len() / h.page_size as usize;
        Self {
            file,
            page_num: 0,
            slot_idx: 0,
            total_pages,
            slots_pp,
            v5_deleted: std::collections::HashSet::new(),
            yielded: 0,
        }
    }

    fn new_v5(file: &'a BtrieveFile) -> Self {
        let h = &file.header;
        let slots_pp = if h.physical_rec_len > 0 && h.page_size > 4 {
            ((h.page_size - 4) / h.physical_rec_len) as usize
        } else {
            0
        };
        let total_pages = file.data.len() / h.page_size as usize;

        // Build deleted-chain set from FCR
        let mut deleted = std::collections::HashSet::new();
        let mut ptr = le32(&file.data, V5_DELETED_CHAIN);
        let mut guard = 0u32;
        while ptr != 0xFFFF_FFFF && ptr != 0 && guard < 1_000_000 {
            deleted.insert(ptr as u64);
            // The deleted record's first 4 bytes point to the next in chain
            let off = ptr as usize;
            if off + 4 <= file.data.len() {
                ptr = le32(&file.data, off);
            } else {
                break;
            }
            guard += 1;
        }

        Self {
            file,
            page_num: 0,
            slot_idx: 0,
            total_pages,
            slots_pp,
            v5_deleted: deleted,
            yielded: 0,
        }
    }
}

impl<'a> Iterator for RecordIter<'a> {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Vec<u8>> {
        if self.slots_pp == 0 {
            return None;
        }
        // Stop once we've yielded the declared record count
        if self.yielded >= self.file.header.record_count && self.file.header.record_count > 0 {
            return None;
        }

        let ps = self.file.header.page_size as usize;
        let prl = self.file.header.physical_rec_len as usize;
        let lrl = self.file.header.logical_rec_len as usize;

        loop {
            if self.page_num >= self.total_pages {
                return None;
            }

            let page_off = self.page_num * ps;
            let page = &self.file.data[page_off..page_off + ps];

            // Check data-page flag: byte 5 must have bit 0x80 set.
            if page.len() > 5 && (page[5] & 0x80) != 0 {
                // This page has records.
                while self.slot_idx < self.slots_pp {
                    let si = self.slot_idx;
                    self.slot_idx += 1;

                    // v6: records start at page offset 6, stride = physical_rec_len
                    // v5: records start at page offset 4
                    let rec_start = if self.file.header.version == 6 { 6 } else { 4 };
                    let slot_off = rec_start + si * prl;
                    if slot_off + prl > ps {
                        break;
                    }

                    let slot = &page[slot_off..slot_off + prl];
                    let file_off = (page_off + slot_off) as u64;

                    if self.file.header.version == 6 {
                        // v6: usage_count (u16) at slot[0..2]; 0 = deleted
                        if slot.len() < 2 {
                            continue;
                        }
                        let usage = u16::from_le_bytes([slot[0], slot[1]]);
                        if usage == 0 {
                            continue;
                        }
                        // Logical record starts at slot[2]
                        let rec_end = 2 + lrl;
                        if rec_end > slot.len() {
                            continue;
                        }
                        let record = slot[2..rec_end].to_vec();
                        self.yielded += 1;
                        return Some(record);
                    } else {
                        // v5: check deleted-chain set
                        if self.v5_deleted.contains(&file_off) {
                            continue;
                        }
                        // v5 records have no usage prefix; data starts at slot[0]
                        let rec_end = lrl.min(slot.len());
                        let record = slot[..rec_end].to_vec();
                        self.yielded += 1;
                        return Some(record);
                    }
                }
            }

            // Advance to next page
            self.page_num += 1;
            self.slot_idx = 0;
        }
    }
}
