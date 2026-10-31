use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::thread;

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

#[derive(Clone, Copy)]
struct Record {
    hour: u8,
    local_ep: u16,
    is_err: bool,
    latency: u32,
}

struct ThreadParseResult<'a> {
    records: Vec<Record>,
    ep_slices: Vec<&'a [u8]>,
}

#[inline(always)]
unsafe fn find_char(mut p: *const u8, c: u8) -> *const u8 {
    let broadcast = 0x0101010101010101u64 * (c as u64);
    while p as usize & 7 != 0 {
        if *p == c { return p; }
        p = p.add(1);
    }
    loop {
        let v = *(p as *const u64);
        let x = v ^ broadcast;
        let has = (x.wrapping_sub(0x0101010101010101)) & !x & 0x8080808080808080;
        if has != 0 {
            let idx = (has.trailing_zeros() / 8) as usize;
            return p.add(idx);
        }
        p = p.add(8);
    }
}

fn parse_chunk<'a>(chunk: &'a [u8]) -> ThreadParseResult<'a> {
    let mut records = Vec::with_capacity(chunk.len() / 70);
    let mut table_hashes = vec![0u64; 4096];
    let mut table_ids = vec![0u16; 4096];
    let mut ep_slices: Vec<&'a [u8]> = Vec::with_capacity(2048);

    unsafe {
        let mut p = chunk.as_ptr();
        let end = p.add(chunk.len());

        while p < end {
            if p.add(28) >= end { break; }
            let m = (*p.add(5) - b'0') * 10 + (*p.add(6) - b'0');
            let d = (*p.add(8) - b'0') * 10 + (*p.add(9) - b'0');
            let h = (*p.add(11) - b'0') * 10 + (*p.add(12) - b'0');
            let day_off = if m == 10 { d - 30 } else { d + 1 };
            let hour_id = day_off * 24 + h - 18;

            p = p.add(28); // skip ts and space

            // method: skip until space
            p = find_char(p, b' ');
            p = p.add(1);

            // endpoint
            let ep_start = p;
            p = find_char(p, b' ');
            let ep_len = p as usize - ep_start as usize;
            let ep_slice = std::slice::from_raw_parts(ep_start, ep_len);
            p = p.add(1);

            // intern endpoint
            let h_val = hash_bytes(ep_slice);
            let mut slot = (h_val as usize) & 4095;
            let local_ep = loop {
                if table_hashes[slot] == 0 {
                    let id = ep_slices.len() as u16;
                    table_hashes[slot] = h_val;
                    table_ids[slot] = id;
                    ep_slices.push(ep_slice);
                    break id;
                }
                if table_hashes[slot] == h_val && ep_slices[table_ids[slot] as usize] == ep_slice {
                    break table_ids[slot];
                }
                slot = (slot + 1) & 4095;
            };

            // status: 3 digits + space
            let is_err = *p == b'5';
            p = p.add(4);

            // latency
            let mut lat: u32 = 0;
            while *p != b'\n' {
                lat = lat * 10 + (*p - b'0') as u32;
                p = p.add(1);
            }
            p = p.add(1);

            records.push(Record {
                hour: hour_id,
                local_ep,
                is_err,
                latency: lat,
            });
        }
    }

    ThreadParseResult { records, ep_slices }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input_path = std::env::var("DATA_DIR")
        .map(|d| format!("{d}/requests.log"))
        .unwrap_or_else(|_| "/data/requests.log".to_string());

    let output_path = std::env::var("OUT_DIR")
        .map(|d| format!("{d}/summary.txt"))
        .unwrap_or_else(|_| "/out/summary.txt".to_string());

    let file = match File::open(&input_path) {
        Ok(f) => f,
        Err(_) => File::open("test-data/a/requests.log")?,
    };

    let len = file.metadata()?.len() as usize;
    if len == 0 {
        let _ = File::create(&output_path)?;
        return Ok(());
    }

    let ptr = unsafe { mmap(std::ptr::null_mut(), len, 1, 1, file.as_raw_fd(), 0) };
    let data = unsafe { std::slice::from_raw_parts(ptr, len) };

    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get().min(12))
        .unwrap_or(1);

    let chunk_size = len / num_threads;
    let mut split_points = Vec::with_capacity(num_threads + 1);
    split_points.push(0);

    for i in 1..num_threads {
        let mut pt = i * chunk_size;
        while pt < len && data[pt] != b'\n' {
            pt += 1;
        }
        if pt < len {
            pt += 1;
        }
        split_points.push(pt.min(len));
    }
    split_points.push(len);

    let mut thread_results = Vec::with_capacity(num_threads);

    thread::scope(|s| {
        let mut handles = Vec::with_capacity(num_threads);
        for w in split_points.windows(2) {
            let chunk = &data[w[0]..w[1]];
            handles.push(s.spawn(move || parse_chunk(chunk)));
        }
        for h in handles {
            thread_results.push(h.join().unwrap());
        }
    });

    // 1. Unify endpoints across threads
    let mut all_unique_eps: Vec<&[u8]> = Vec::new();
    for res in &thread_results {
        for &ep in &res.ep_slices {
            if !all_unique_eps.contains(&ep) {
                all_unique_eps.push(ep);
            }
        }
    }
    all_unique_eps.sort_unstable();

    let num_eps = all_unique_eps.len();
    let mut remaps = Vec::with_capacity(thread_results.len());
    for res in &thread_results {
        let mut remap = Vec::with_capacity(res.ep_slices.len());
        for &ep in &res.ep_slices {
            let canon_id = all_unique_eps.binary_search(&ep).unwrap() as u16;
            remap.push(canon_id);
        }
        remaps.push(remap);
    }

    let num_groups = 72 * num_eps;
    let mut group_counts = vec![0u32; num_groups];
    let mut group_errors = vec![0u32; num_groups];

    for (t_idx, res) in thread_results.iter().enumerate() {
        let remap = &remaps[t_idx];
        for r in &res.records {
            let canon_ep = remap[r.local_ep as usize];
            let g = (r.hour as usize) * num_eps + (canon_ep as usize);
            group_counts[g] += 1;
            if r.is_err {
                group_errors[g] += 1;
            }
        }
    }

    let mut group_offsets = vec![0usize; num_groups + 1];
    let mut total_records = 0;
    for g in 0..num_groups {
        group_offsets[g] = total_records;
        total_records += group_counts[g] as usize;
    }
    group_offsets[num_groups] = total_records;

    let mut latencies = vec![0u32; total_records];
    let mut current_pos = group_offsets.clone();

    for (t_idx, res) in thread_results.iter().enumerate() {
        let remap = &remaps[t_idx];
        for r in &res.records {
            let canon_ep = remap[r.local_ep as usize];
            let g = (r.hour as usize) * num_eps + (canon_ep as usize);
            let pos = current_pos[g];
            latencies[pos] = r.latency;
            current_pos[g] += 1;
        }
    }

    // Pre-generate 72 hour strings
    let mut hour_strings = Vec::with_capacity(72);
    for h_id in 0..72 {
        let total_h = h_id + 18;
        let day_offset = total_h / 24;
        let h = total_h % 24;
        let (month, day) = if day_offset <= 1 {
            (10, 30 + day_offset)
        } else {
            (11, day_offset - 1)
        };
        hour_strings.push(format!("2026-{:02}-{:02}T{:02}:00:00Z", month, day, h).into_bytes());
    }

    // Parallel formatting: split num_groups across available threads
    let mut thread_buffers = vec![Vec::new(); num_threads];
    let groups_per_thread = (num_groups + num_threads - 1) / num_threads;

    let group_counts_ref = &group_counts;
    let group_offsets_ref = &group_offsets;
    let group_errors_ref = &group_errors;
    let hour_strings_ref = &hour_strings;
    let all_unique_eps_ref = &all_unique_eps;
    let lat_addr = latencies.as_mut_ptr() as usize;

    thread::scope(|s| {
        for (t_idx, buf) in thread_buffers.iter_mut().enumerate() {
            let g_start = t_idx * groups_per_thread;
            let g_end = (g_start + groups_per_thread).min(num_groups);
            if g_start >= g_end { continue; }

            s.spawn(move || {
                let mut local_buf = Vec::with_capacity((g_end - g_start) * 80);
                for g in g_start..g_end {
                    let count = group_counts_ref[g] as usize;
                    if count == 0 { continue; }
                    let h_id = g / num_eps;
                    let ep_id = g % num_eps;

                    let start = group_offsets_ref[g];
                    let slice = unsafe { std::slice::from_raw_parts_mut((lat_addr as *mut u32).add(start), count) };
                    slice.sort_unstable();

                    let p50_idx = ((50 * count + 99) / 100) - 1;
                    let p95_idx = ((95 * count + 99) / 100) - 1;
                    let p99_idx = ((99 * count + 99) / 100) - 1;

                    let p50 = slice[p50_idx];
                    let p95 = slice[p95_idx];
                    let p99 = slice[p99_idx];

                    let errs = group_errors_ref[g];
                    let error_ppm = (errs as u64 * 1_000_000) / (count as u64);

                    local_buf.extend_from_slice(&hour_strings_ref[h_id]);
                    local_buf.push(b' ');
                    local_buf.extend_from_slice(all_unique_eps_ref[ep_id]);
                    write!(&mut local_buf, " {} {} {} {} {} {}\n", count, p50, p95, p99, errs, error_ppm).unwrap();
                }
                *buf = local_buf;
            });
        }
    });

    let mut out_file = std::io::BufWriter::with_capacity(1024 * 1024, File::create(&output_path)?);
    for buf in thread_buffers {
        out_file.write_all(&buf)?;
    }
    out_file.flush()?;

    Ok(())
}
