use engine::segment::SegmentTable;
use types::ByteRange;

#[test]
fn test_segment_creation_covers_entire_file_without_gaps() {
    let file_size: u64 = 100_000_000; // 100 MB
    let num_segments = 8;
    let initial_range = vec![ByteRange::new(0, file_size - 1)];
    let table = SegmentTable::new(file_size, &initial_range, num_segments);

    let segments = table.snapshot_segments();
    assert_eq!(segments.len(), num_segments);

    // Verify first starts at 0
    assert_eq!(segments[0].0, 0);

    // Verify last ends at file_size - 1
    assert_eq!(segments.last().unwrap().2, file_size - 1);

    // Verify contiguous intervals with zero gaps and zero overlaps
    for i in 0..segments.len() - 1 {
        assert_eq!(
            segments[i].2 + 1,
            segments[i + 1].0,
            "Gap or overlap detected between segment {} and {}",
            i,
            i + 1
        );
    }

    // Verify total byte sum
    let total_bytes: u64 = segments
        .iter()
        .map(|(start, _pos, end)| end - start + 1)
        .sum();
    assert_eq!(total_bytes, file_size);
}

#[test]
fn test_segment_edge_cases() {
    // 1-byte file
    let table = SegmentTable::new(1, &[ByteRange::new(0, 0)], 4);
    let segments = table.snapshot_segments();
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].0, 0);
    assert_eq!(segments[0].2, 0);

    // File smaller than requested segments
    let table = SegmentTable::new(3, &[ByteRange::new(0, 2)], 8);
    let segments = table.snapshot_segments();
    assert_eq!(segments.len(), 1); // Below MIN_SPLIT_HALF * 2, preserved as 1 segment
    assert_eq!(segments[0].0, 0);
    assert_eq!(segments[0].2, 2);
}

#[test]
fn test_work_stealing_split() {
    // 50 MB segment
    let file_size: u64 = 50 * 1024 * 1024;
    let mut table = SegmentTable::new(file_size, &[ByteRange::new(0, file_size - 1)], 1);

    // Worker 1 claims the first segment
    let claim1 = table.claim();
    assert!(claim1.is_some());
    let (id1, range1) = claim1.unwrap();
    assert_eq!(id1, 0);
    assert_eq!(range1.start, 0);

    // Worker 2 attempts to claim - table steals half from Worker 1's remaining work
    let claim2 = table.claim();
    assert!(
        claim2.is_some(),
        "Work stealing should split the large active segment"
    );
    let (id2, range2) = claim2.unwrap();
    assert_eq!(id2, 1);

    // Verify segments are still contiguous and total size is preserved
    let segments = table.snapshot_segments();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].2 + 1, range2.start);
    assert_eq!(range2.end, file_size - 1);

    let total: u64 = segments
        .iter()
        .map(|(start, _pos, end)| end - start + 1)
        .sum();
    assert_eq!(total, file_size);
}
