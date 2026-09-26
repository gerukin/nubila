//! Offline cache-hit measurement, including parsing; no network or user files.
use nubila::service::Client;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{hint::black_box, time::Instant};
fn main() {
    let dir = tempfile::tempdir().unwrap();
    let url = "https://example.invalid/forecast";
    let data = json!({"at": chrono::Utc::now().timestamp(), "data": {
        "hourly": {"time": (0..2208).map(|i| 1_800_000_000_i64 + i * 3600).collect::<Vec<_>>(),
        "values": (0..2208).map(|i| vec![i as f64 / 10.0; 9]).collect::<Vec<_>>()}
    }});
    let bytes = serde_json::to_vec(&data).unwrap();
    std::fs::write(
        dir.path()
            .join(format!("{:x}.json", Sha256::digest(url.as_bytes()))),
        &bytes,
    )
    .unwrap();
    let client = Client::new(dir.path().into(), true);
    let _ = client.get(url, &[], 1800, false).unwrap();
    println!("Cache response: {} bytes; 200 reads per case", bytes.len());
    for extra_clone in [true, false] {
        let start = Instant::now();
        for _ in 0..200 {
            let (value, _) = client.get(url, &[], 1800, false).unwrap();
            if extra_clone {
                black_box(value.clone());
            }
            black_box(value);
        }
        println!(
            "{}: {:.1} us/read",
            if extra_clone {
                "with former extra JSON clone"
            } else {
                "current ownership transfer"
            },
            start.elapsed().as_secs_f64() * 1e6 / 200.0
        );
    }
}
