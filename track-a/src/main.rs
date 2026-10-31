use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::sync::Mutex;
use std::thread;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct GroupKey {
    hour: [u8; 13],
    endpoint: Vec<u8>,
}

struct GroupStats {
    latencies: Vec<u32>,
    errors: u32,
}

fn parse_chunk(chunk: &[u8]) -> HashMap<GroupKey, GroupStats> {
    let mut map: HashMap<GroupKey, GroupStats> = HashMap::with_capacity(8192);
    let mut pos = 0;
    let len = chunk.len();

    while pos < len {
        // Line format: ts SP method SP endpoint SP status SP latency LF
        // ts is always 27 bytes: YYYY-MM-DDTHH:mm:ss.ffffffZ
        if pos + 28 > len {
            break;
        }

        let mut hour = [0u8; 13];
        hour.copy_from_slice(&chunk[pos..pos + 13]);

        // Skip to after ts and space (28 bytes)
        pos += 28;

        // Skip method until space
        while pos < len && chunk[pos] != b' ' {
            pos += 1;
        }
        pos += 1; // skip space

        // Read endpoint until space
        let ep_start = pos;
        while pos < len && chunk[pos] != b' ' {
            pos += 1;
        }
        let ep_bytes = &chunk[ep_start..pos];
        pos += 1; // skip space

        // Read status (always 3 digits)
        let is_error = pos + 3 <= len && chunk[pos] == b'5';
        pos += 3;
        pos += 1; // skip space

        // Read latency until newline
        let mut latency: u32 = 0;
        while pos < len && chunk[pos] != b'\n' {
            let b = chunk[pos];
            if b >= b'0' && b <= b'9' {
                latency = latency * 10 + (b - b'0') as u32;
            }
            pos += 1;
        }
        pos += 1; // skip newline

        let key = GroupKey {
            hour,
            endpoint: ep_bytes.to_vec(),
        };

        let entry = map.entry(key).or_insert_with(|| GroupStats {
            latencies: Vec::with_capacity(8),
            errors: 0,
        });
        entry.latencies.push(latency);
        if is_error {
            entry.errors += 1;
        }
    }

    map
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input_path = std::env::var("DATA_DIR")
        .map(|d| format!("{d}/requests.log"))
        .unwrap_or_else(|_| "/data/requests.log".to_string());

    let output_path = std::env::var("OUT_DIR")
        .map(|d| format!("{d}/summary.txt"))
        .unwrap_or_else(|_| "/out/summary.txt".to_string());

    let mut file = match File::open(&input_path) {
        Ok(f) => f,
        Err(_) => File::open("test-data/a/requests.log")?,
    };

    let mut data = Vec::new();
    file.read_to_end(&mut data)?;

    if data.is_empty() {
        let _ = File::create(&output_path)?;
        return Ok(());
    }

    // Determine number of worker threads (default to available cores up to 12)
    let num_threads = std::thread::available_parallelism()
        .map(|n| n.get().min(12))
        .unwrap_or(1);

    let chunk_size = data.len() / num_threads;
    let mut split_points = Vec::with_capacity(num_threads + 1);
    split_points.push(0);

    for i in 1..num_threads {
        let mut pt = i * chunk_size;
        while pt < data.len() && data[pt] != b'\n' {
            pt += 1;
        }
        if pt < data.len() {
            pt += 1; // line starts right after newline
        }
        split_points.push(pt.min(data.len()));
    }
    split_points.push(data.len());

    let merged_map: Mutex<HashMap<GroupKey, GroupStats>> = Mutex::new(HashMap::new());

    thread::scope(|s| {
        for w in split_points.windows(2) {
            let start = w[0];
            let end = w[1];
            if start >= end {
                continue;
            }
            let chunk = &data[start..end];
            let merged_ref = &merged_map;

            s.spawn(move || {
                let local_map = parse_chunk(chunk);
                let mut global = merged_ref.lock().unwrap();
                for (k, v) in local_map {
                    let entry = global.entry(k).or_insert_with(|| GroupStats {
                        latencies: Vec::new(),
                        errors: 0,
                    });
                    entry.latencies.extend(v.latencies);
                    entry.errors += v.errors;
                }
            });
        }
    });

    let mut global_map = merged_map.into_inner().unwrap();

    // Sort keys: hour ascending, then endpoint byte-order ascending
    let mut keys: Vec<GroupKey> = global_map.keys().cloned().collect();
    keys.sort_unstable_by(|a, b| {
        a.hour.cmp(&b.hour).then_with(|| a.endpoint.cmp(&b.endpoint))
    });

    let out_file = File::create(&output_path)?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, out_file);

    for key in keys {
        let stats = global_map.get_mut(&key).unwrap();
        let n = stats.latencies.len();
        if n == 0 {
            continue;
        }

        stats.latencies.sort_unstable();

        // 1-indexed ranks: ceil(p * n / 100) = (p * n + 99) / 100
        let p50_idx = ((50 * n + 99) / 100) - 1;
        let p95_idx = ((95 * n + 99) / 100) - 1;
        let p99_idx = ((99 * n + 99) / 100) - 1;

        let p50 = stats.latencies[p50_idx];
        let p95 = stats.latencies[p95_idx];
        let p99 = stats.latencies[p99_idx];

        let error_ppm = (stats.errors as u64 * 1_000_000) / (n as u64);

        // Format: hour endpoint count p50 p95 p99 errors error_ppm
        // hour is YYYY-MM-DDTHH:00:00Z (key.hour is YYYY-MM-DDTHH)
        writer.write_all(&key.hour)?;
        writer.write_all(b":00:00Z ")?;
        writer.write_all(&key.endpoint)?;
        write!(
            writer,
            " {} {} {} {} {} {}\n",
            n, p50, p95, p99, stats.errors, error_ppm
        )?;
    }

    writer.flush()?;
    Ok(())
}
