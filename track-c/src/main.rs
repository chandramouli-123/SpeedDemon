use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::AsRawFd;

extern "C" {
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, offset: i64) -> *mut u8;
}

#[inline(always)]
fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

struct NameSet {
    hashes: Vec<u64>,
    names: Vec<Box<[u8]>>,
    count: usize,
}

impl NameSet {
    fn new(cap: usize) -> Self {
        Self {
            hashes: vec![0; cap],
            names: vec![Vec::new().into_boxed_slice(); cap],
            count: 0,
        }
    }

    #[inline(always)]
    fn insert(&mut self, name: &[u8]) {
        let h = hash_bytes(name) | 1;
        let mask = self.hashes.len() - 1;
        let mut slot = (h as usize) & mask;
        loop {
            if self.hashes[slot] == 0 {
                self.hashes[slot] = h;
                self.names[slot] = name.into();
                self.count += 1;
                if self.count * 2 >= self.hashes.len() {
                    self.grow();
                }
                return;
            }
            if self.hashes[slot] == h && &*self.names[slot] == name {
                return;
            }
            slot = (slot + 1) & mask;
        }
    }

    fn grow(&mut self) {
        let new_cap = self.hashes.len() * 2;
        let mut new_hashes = vec![0u64; new_cap];
        let mut new_names = vec![Vec::new().into_boxed_slice(); new_cap];
        let mask = new_cap - 1;

        for i in 0..self.hashes.len() {
            let h = self.hashes[i];
            if h != 0 {
                let mut slot = (h as usize) & mask;
                while new_hashes[slot] != 0 {
                    slot = (slot + 1) & mask;
                }
                new_hashes[slot] = h;
                new_names[slot] = std::mem::replace(&mut self.names[i], Vec::new().into_boxed_slice());
            }
        }
        self.hashes = new_hashes;
        self.names = new_names;
    }
}

struct CountryStats {
    country_name: [u8; 11],
    name_len: u8,
    count: u64,
    id_sum: u64,
    amount_sum: i64,
    amount_min: i64,
    amount_max: i64,
    name_bytes: u64,
    note_bytes: u64,
    names: NameSet,
}

impl CountryStats {
    fn new(name: &[u8]) -> Self {
        let mut country_name = [0u8; 11];
        country_name[..name.len()].copy_from_slice(name);
        Self {
            country_name,
            name_len: name.len() as u8,
            count: 0,
            id_sum: 0,
            amount_sum: 0,
            amount_min: i64::MAX,
            amount_max: i64::MIN,
            name_bytes: 0,
            note_bytes: 0,
            names: NameSet::new(1024),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "/data".to_string());
    let out_dir = std::env::var("OUT_DIR").unwrap_or_else(|_| "/out".to_string());

    let regions_path = format!("{data_dir}/regions.csv");
    let records_path = format!("{data_dir}/records.csv");
    let output_path = format!("{out_dir}/aggregates.txt");

    let regions_file = match File::open(&regions_path) {
        Ok(f) => f,
        Err(_) => File::open("test-data/c/regions.csv")?,
    };

    let reader = BufReader::new(regions_file);
    let mut country_map: HashMap<Vec<u8>, usize> = HashMap::new();
    let mut stats_list: Vec<CountryStats> = Vec::new();

    // Compact 10MB table for region_id -> country_idx (255 = unmatched)
    let mut region_table: Vec<u8> = vec![255u8; 10_000_000];

    for (line_idx, line) in reader.lines().enumerate() {
        let line = line?;
        if line_idx == 0 || line.is_empty() { continue; }
        let mut parts = line.split(',');
        let rid_str = parts.next().unwrap();
        let c_str = parts.next().unwrap();
        let rid: usize = rid_str.parse().unwrap();
        let c_bytes = c_str.as_bytes();

        let c_idx = *country_map.entry(c_bytes.to_vec()).or_insert_with(|| {
            let idx = stats_list.len();
            stats_list.push(CountryStats::new(c_bytes));
            idx
        });

        if rid < region_table.len() {
            region_table[rid] = c_idx as u8;
        }
    }

    let unmatched_idx = stats_list.len();
    stats_list.push(CountryStats::new(b"_UNMATCHED"));

    let records_file = match File::open(&records_path) {
        Ok(f) => f,
        Err(_) => File::open("test-data/c/records.csv")?,
    };

    let len = records_file.metadata()?.len() as usize;
    if len == 0 {
        let _ = File::create(&output_path)?;
        return Ok(());
    }

    let ptr = unsafe { mmap(std::ptr::null_mut(), len, 1, 1, records_file.as_raw_fd(), 0) };
    let end = unsafe { ptr.add(len) };

    let mut unescape_buf = Vec::with_capacity(256);

    unsafe {
        let mut p = ptr as *const u8;
        // Check for BOM at file start
        if p.add(3) <= end && *p == 0xEF && *p.add(1) == 0xBB && *p.add(2) == 0xBF {
            p = p.add(3);
        }

        // Skip header
        while p < end && *p != b'\n' { p = p.add(1); }
        if p < end { p = p.add(1); }

        // Check for BOM after header
        if p.add(3) <= end && *p == 0xEF && *p.add(1) == 0xBB && *p.add(2) == 0xBF {
            p = p.add(3);
        }

        while p < end {
            if p >= end { break; }

            // 1. id
            let mut id: u64 = 0;
            while *p != b',' {
                id = id * 10 + (*p - b'0') as u64;
                p = p.add(1);
            }
            p = p.add(1);

            // 2. name
            let name_slice: &[u8];
            let name_len: usize;
            if *p == b'"' {
                p = p.add(1);
                let start = p;
                let mut has_quotes = false;
                loop {
                    if *p == b'"' {
                        if *p.add(1) == b'"' {
                            has_quotes = true;
                            p = p.add(2);
                        } else {
                            break;
                        }
                    } else {
                        p = p.add(1);
                    }
                }
                let end_name = p;
                p = p.add(1);

                if !has_quotes {
                    let raw_len = end_name as usize - start as usize;
                    name_slice = std::slice::from_raw_parts(start, raw_len);
                    name_len = raw_len;
                } else {
                    unescape_buf.clear();
                    let mut q = start;
                    while q < end_name {
                        if *q == b'"' && *q.add(1) == b'"' {
                            unescape_buf.push(b'"');
                            q = q.add(2);
                        } else {
                            unescape_buf.push(*q);
                            q = q.add(1);
                        }
                    }
                    name_slice = &unescape_buf;
                    name_len = unescape_buf.len();
                }
            } else {
                let start = p;
                while *p != b',' { p = p.add(1); }
                let raw_len = p as usize - start as usize;
                name_slice = std::slice::from_raw_parts(start, raw_len);
                name_len = raw_len;
            }
            p = p.add(1);

            // 3. note
            let mut note_len: usize = 0;
            if *p == b'"' {
                p = p.add(1);
                loop {
                    if *p == b'"' {
                        if *p.add(1) == b'"' {
                            note_len += 1;
                            p = p.add(2);
                        } else {
                            p = p.add(1);
                            break;
                        }
                    } else {
                        note_len += 1;
                        p = p.add(1);
                    }
                }
            } else {
                while *p != b',' {
                    note_len += 1;
                    p = p.add(1);
                }
            }
            p = p.add(1);

            // 4. region_id
            let mut rid: usize = 0;
            while *p != b',' {
                rid = rid * 10 + (*p - b'0') as usize;
                p = p.add(1);
            }
            p = p.add(1);

            // 5. amount
            let neg = *p == b'-';
            if neg { p = p.add(1); }
            let mut amt_val: i64 = 0;
            while *p != b'.' {
                amt_val = amt_val * 10 + (*p - b'0') as i64;
                p = p.add(1);
            }
            p = p.add(1);
            amt_val = amt_val * 100 + (*p - b'0') as i64 * 10 + (*p.add(1) - b'0') as i64;
            p = p.add(2);
            let amount = if neg { -amt_val } else { amt_val };

            if p < end && *p == b'\r' { p = p.add(1); }
            if p < end && *p == b'\n' { p = p.add(1); }

            if p.add(3) <= end && *p == 0xEF && *p.add(1) == 0xBB && *p.add(2) == 0xBF {
                p = p.add(3);
            }

            let c_idx = if rid < region_table.len() && region_table[rid] != 255 {
                region_table[rid] as usize
            } else {
                unmatched_idx
            };

            let stats = &mut stats_list[c_idx];
            stats.count += 1;
            stats.id_sum += id;
            stats.amount_sum += amount;
            if amount < stats.amount_min { stats.amount_min = amount; }
            if amount > stats.amount_max { stats.amount_max = amount; }
            stats.name_bytes += name_len as u64;
            stats.note_bytes += note_len as u64;
            stats.names.insert(name_slice);
        }
    }

    let mut active: Vec<&CountryStats> = stats_list.iter().filter(|s| s.count > 0).collect();
    active.sort_unstable_by(|a, b| {
        let name_a = &a.country_name[..a.name_len as usize];
        let name_b = &b.country_name[..b.name_len as usize];
        name_a.cmp(name_b)
    });

    let mut out_file = std::io::BufWriter::new(File::create(&output_path)?);
    for s in active {
        let country = std::str::from_utf8(&s.country_name[..s.name_len as usize])?;
        write!(
            out_file,
            "{} {} {} {} {} {} {} {} {}\n",
            country,
            s.count,
            s.id_sum,
            format_money(s.amount_sum),
            format_money(s.amount_min),
            format_money(s.amount_max),
            s.names.count,
            s.name_bytes,
            s.note_bytes,
        )?;
    }
    out_file.flush()?;

    Ok(())
}

fn format_money(cents: i64) -> String {
    let (neg, abs_val) = if cents < 0 {
        (true, (-cents) as u64)
    } else {
        (false, cents as u64)
    };
    let dollars = abs_val / 100;
    let fraction = abs_val % 100;
    if neg {
        format!("-{}.{:02}", dollars, fraction)
    } else {
        format!("{}.{:02}", dollars, fraction)
    }
}
