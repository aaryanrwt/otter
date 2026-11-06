use std::sync::{Arc, Mutex};
use types::ByteRange;

pub const MIN_SPLIT_HALF: u64 = 512 * 1024; // 512 KiB

#[derive(Debug, Clone)]
pub struct Segment {
    pub id: usize,
    pub start: u64,
    pub pos: u64,
    pub end: u64, // inclusive
    pub active: bool,
}

impl Segment {
    pub fn remaining(&self) -> u64 {
        if self.pos > self.end {
            0
        } else {
            self.end - self.pos + 1
        }
    }
}

pub struct SegmentTable {
    total_size: u64,
    segments: Vec<Segment>,
    next_id: usize,
}

impl SegmentTable {
    pub fn new(total_size: u64, missing_ranges: &[ByteRange], initial_workers: usize) -> Self {
        let workers = initial_workers.max(1);
        let mut segments = Vec::new();
        let mut next_id = 0;

        for range in missing_ranges {
            let range_len = range.len();
            if range_len == 0 {
                continue;
            }

            // Split this missing range proportionally among available workers
            let split_count = if range_len < MIN_SPLIT_HALF * 2 {
                1
            } else {
                ((range_len / (MIN_SPLIT_HALF * 2)).min(workers as u64)).max(1) as usize
            };

            let chunk_size = range_len / split_count as u64;
            let mut cursor = range.start;

            for i in 0..split_count {
                let end = if i == split_count - 1 {
                    range.end
                } else {
                    cursor + chunk_size - 1
                };

                segments.push(Segment {
                    id: next_id,
                    start: cursor,
                    pos: cursor,
                    end,
                    active: false,
                });
                next_id += 1;
                cursor = end + 1;
            }
        }

        Self {
            total_size,
            segments,
            next_id,
        }
    }

    pub fn total_size(&self) -> u64 {
        self.total_size
    }

    pub fn current_end(&self, id: usize) -> Option<u64> {
        self.segments.iter().find(|s| s.id == id).map(|s| s.end)
    }

    /// Claim an inactive segment, or steal half of the largest remaining active segment.
    pub fn claim(&mut self) -> Option<(usize, ByteRange)> {
        // 1. Look for an inactive segment with remaining work
        if let Some(seg) = self
            .segments
            .iter_mut()
            .find(|s| !s.active && s.remaining() > 0)
        {
            seg.active = true;
            return Some((seg.id, ByteRange::new(seg.pos, seg.end)));
        }

        // 2. Work-Stealing: find the active segment with the largest remaining bytes
        let mut largest_idx = None;
        let mut max_remaining = 0;

        for (idx, seg) in self.segments.iter().enumerate() {
            let rem = seg.remaining();
            if rem > max_remaining {
                max_remaining = rem;
                largest_idx = Some(idx);
            }
        }

        // 3. Tail Starvation Rule: Only split if the stolen chunk is at least MIN_SPLIT_HALF (512 KiB)
        // This prevents connection storms on trivial tail fragments while ensuring large tails (>1 MiB)
        // are actively shared across all idle workers.
        if let Some(idx) = largest_idx
            && max_remaining >= MIN_SPLIT_HALF * 2
        {
            let stolen_len = max_remaining / 2;
            let original = &mut self.segments[idx];
            let new_end = original.end;
            original.end = new_end - stolen_len;

            let new_start = original.end + 1;
            let new_seg = Segment {
                id: self.next_id,
                start: new_start,
                pos: new_start,
                end: new_end,
                active: true,
            };
            let id = self.next_id;
            self.next_id += 1;
            self.segments.push(new_seg);

            return Some((id, ByteRange::new(new_start, new_end)));
        }

        None
    }

    /// Advance segment position by n bytes and return true if still has bytes to download.
    pub fn advance(&mut self, id: usize, n: u64) -> bool {
        if let Some(seg) = self.segments.iter_mut().find(|s| s.id == id) {
            seg.pos += n;
            seg.remaining() > 0
        } else {
            false
        }
    }

    /// Mark segment as completed or inactive.
    pub fn release(&mut self, id: usize) {
        if let Some(seg) = self.segments.iter_mut().find(|s| s.id == id) {
            seg.active = false;
        }
    }

    pub fn total_downloaded(&self) -> u64 {
        self.segments
            .iter()
            .map(|s| s.pos.saturating_sub(s.start))
            .sum()
    }

    pub fn is_all_complete(&self) -> bool {
        self.segments.iter().all(|s| s.remaining() == 0)
    }

    pub fn snapshot_segments(&self) -> Vec<(u64, u64, u64)> {
        self.segments
            .iter()
            .map(|s| (s.start, s.pos, s.end))
            .collect()
    }
}

pub type SharedSegmentTable = Arc<Mutex<SegmentTable>>;
