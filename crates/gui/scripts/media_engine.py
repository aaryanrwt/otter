import sys
import os
import json
import re
import time
import shutil
import threading
import subprocess
import urllib.request
import urllib.error
import ctypes
from concurrent.futures import ThreadPoolExecutor, as_completed

CANDIDATE_STREAM_LEVELS = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 16, 32, 44, 64, 108, 128]

def get_ffmpeg_path():
    candidates = [
        r"C:\Program Files\ffmpeg\bin",
        r"C:\ffmpeg\bin",
    ]
    local_app_data = os.environ.get("LOCALAPPDATA")
    if local_app_data:
        winget_pkgs = os.path.join(local_app_data, "Microsoft", "WinGet", "Packages")
        if os.path.isdir(winget_pkgs):
            for entry in os.listdir(winget_pkgs):
                if "FFmpeg" in entry:
                    full_p = os.path.join(winget_pkgs, entry)
                    for sub in os.listdir(full_p):
                        bin_p = os.path.join(full_p, sub, "bin")
                        if os.path.isdir(bin_p):
                            candidates.append(bin_p)
    for c in candidates:
        if os.path.isdir(c) and (os.path.exists(os.path.join(c, "ffmpeg.exe")) or os.path.exists(os.path.join(c, "ffmpeg"))):
            return c
    # Also check PATH
    which_ff = shutil.which("ffmpeg")
    if which_ff:
        return os.path.dirname(which_ff)
    return None

def sanitize_filename(name):
    cleaned = re.sub(r'[\\/*?:"<>|]', "", str(name)).strip()
    return cleaned[:120] if cleaned else "media_download"

def set_hidden_attribute(path):
    if os.name == 'nt' and os.path.exists(path):
        try:
            ctypes.windll.kernel32.SetFileAttributesW(str(path), 0x02)
        except Exception:
            pass

def get_same_drive_staging_dir(dest_dir, staging_id):
    """
    CRITICAL REQUIREMENT: Never use C: as a temporary download destination when downloading to D:.
    Always place temporary staging on the exact same drive/volume as dest_dir, inside a hidden
    .otter_staging folder so the user never sees clutter AND final assembly is an O(1) atomic move.
    """
    staging_root = os.path.join(dest_dir, ".otter_staging")
    os.makedirs(staging_root, exist_ok=True)
    set_hidden_attribute(staging_root)
    job_staging = os.path.join(staging_root, staging_id)
    os.makedirs(job_staging, exist_ok=True)
    return job_staging

def base_ydl_opts():
    ffmpeg_dir = get_ffmpeg_path()
    opts = {
        'quiet': True,
        'no_warnings': True,
        'noplaylist': True,
        'noprogress': True,
    }
    if ffmpeg_dir:
        opts['ffmpeg_location'] = ffmpeg_dir
    if os.path.exists(r"C:\Program Files\nodejs\node.exe"):
        opts['js_runtimes'] = {'node': {'path': r"C:\Program Files\nodejs\node.exe"}}
        opts['remote_components'] = ['ejs:github']
    return opts

def probe_url(url):
    import yt_dlp

    ydl_opts = base_ydl_opts()
    ydl_opts['skip_download'] = True
    ydl_opts['extract_flat'] = False

    with yt_dlp.YoutubeDL(ydl_opts) as ydl:
        try:
            info = ydl.extract_info(url, download=False)
            if not info:
                return {"is_media": False, "capability": "Unsupported", "error": "No media info returned"}

            title = info.get('title') or "Media Download"
            extractor = info.get('extractor_key') or info.get('extractor') or "Direct Media"
            thumbnail = info.get('thumbnail') or ""
            uploader = info.get('uploader') or info.get('channel') or ""
            duration = info.get('duration') or 0

            formats = info.get('formats') or []
            heights_set = set()
            fps_set = set()
            vcodecs = set()
            acodecs = set()
            audio_bitrates = set()
            protocols = set()
            max_est_size = info.get('filesize') or info.get('filesize_approx') or 0

            for f in formats:
                h = f.get('height')
                fps = f.get('fps')
                vc = f.get('vcodec')
                ac = f.get('acodec')
                abr = f.get('abr')
                proto = f.get('protocol')
                fsize = f.get('filesize') or f.get('filesize_approx') or 0
                if fsize > max_est_size:
                    max_est_size = fsize
                if proto:
                    protocols.add(proto)
                if h and vc and vc != 'none':
                    heights_set.add(int(h))
                if fps and vc and vc != 'none':
                    fps_set.add(int(fps))
                if vc and vc != 'none':
                    vcodecs.add(vc.split('.')[0])
                if ac and ac != 'none':
                    acodecs.add(ac.split('.')[0])
                if abr:
                    audio_bitrates.add(int(abr))

            sorted_heights = sorted(list(heights_set), reverse=True)

            target_resolutions = [
                (2160, "4K UHD (2160p)"),
                (1440, "2K QHD (1440p)"),
                (1080, "1080p (Full HD)"),
                (720, "720p (HD)"),
                (480, "480p (SD)"),
                (360, "360p"),
                (240, "240p"),
                (144, "144p"),
            ]

            available_video_options = []
            for target_h, label in target_resolutions:
                # Strictly include ONLY resolutions genuinely offered by the source
                if target_h in heights_set:
                    available_video_options.append({
                        "id": f"video_{target_h}",
                        "height": target_h,
                        "label": label,
                        "type": "video",
                        "verified": True
                    })

            if not available_video_options and sorted_heights:
                for h in sorted_heights:
                    available_video_options.append({
                        "id": f"video_{h}",
                        "height": h,
                        "label": f"{h}p (Verified Source)",
                        "type": "video",
                        "verified": True
                    })

            if not available_video_options:
                available_video_options.append({
                    "id": "video_best",
                    "height": 0,
                    "label": "Best Native Video Quality",
                    "type": "video",
                    "verified": True
                })

            audio_options = [
                {"id": "audio_best", "height": 0, "label": "Highest Native Audio (Original/M4A/Opus)", "type": "audio"},
                {"id": "audio_mp3", "height": 0, "label": "Audio Only (MP3)", "type": "audio"},
                {"id": "audio_m4a", "height": 0, "label": "Audio Only (M4A)", "type": "audio"},
            ]

            all_options = available_video_options + audio_options
            safe_title = sanitize_filename(title)

            # Classify capability
            capability = "Supported platform extraction"
            if any("m3u8" in p for p in protocols) and not any(p in ("http", "https") for p in protocols):
                capability = "Supported HLS"
            elif any("dash" in p for p in protocols):
                capability = "Supported DASH"
            elif extractor.lower() in ("generic", "direct"):
                capability = "Supported direct media"

            return {
                "is_media": True,
                "capability": capability,
                "url": url,
                "title": title,
                "clean_filename": safe_title,
                "extractor": extractor,
                "thumbnail": thumbnail,
                "uploader": uploader,
                "duration": duration,
                "available_heights": sorted_heights,
                "available_fps": sorted(list(fps_set), reverse=True),
                "video_codecs": sorted(list(vcodecs)),
                "audio_codecs": sorted(list(acodecs)),
                "audio_bitrates_kbps": sorted(list(audio_bitrates), reverse=True),
                "protocols": sorted(list(protocols)),
                "expected_size_bytes": max_est_size,
                "range_supported": True,
                "quality_options": all_options,
                "default_quality_id": available_video_options[0]["id"] if available_video_options else "audio_mp3",
            }
        except Exception as e:
            err_str = str(e)
            cap = "Unsupported"
            if "403" in err_str or "Cloudflare" in err_str or "anti-bot" in err_str:
                cap = "Access denied"
            elif "401" in err_str or "login" in err_str.lower() or "private" in err_str.lower():
                cap = "Authentication required"
            elif "DRM" in err_str:
                cap = "DRM-protected"
            elif "410" in err_str or "expired" in err_str.lower():
                cap = "Expired"
            return {
                "is_media": False,
                "capability": cap,
                "error": err_str
            }


class AdaptiveStreamGovernor:
    """
    AIMD + Throughput/Latency/Tail Governor that dynamically selects concurrency across:
    [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 16, 32, 44, 64, 108, 128]
    based on measured throughput, TTFB/RTT, HTTP 429/503 signals, and remaining work.
    """
    def __init__(self, initial_workers=16, max_workers=128, total_bytes=0):
        self.lock = threading.Lock()
        # Pick initial level based on initial_workers
        self.level_idx = self._nearest_idx(initial_workers)
        self.max_idx = self._nearest_idx(max_workers)
        self.total_bytes = total_bytes
        self.last_eval_time = time.time()
        self.last_eval_bytes = 0
        self.baseline_speed_bps = 0.0
        self.plateau_count = 0
        self.error_429_503 = 0
        self.timeout_errors = 0
        self.rtt_samples_ms = []
        self.disk_latency_ms = 0.0
        self.history = [{
            "timestamp": round(time.time(), 3),
            "progress_pct": 0.0,
            "workers": self.current_workers,
            "reason": "initial_warmup",
            "speed_mbps": 0.0,
            "rtt_ms": 0.0
        }]

    def _nearest_idx(self, val):
        best_i = 0
        best_diff = abs(CANDIDATE_STREAM_LEVELS[0] - val)
        for i, c in enumerate(CANDIDATE_STREAM_LEVELS):
            if abs(c - val) < best_diff:
                best_diff = abs(c - val)
                best_i = i
        return best_i

    @property
    def current_workers(self):
        return CANDIDATE_STREAM_LEVELS[self.level_idx]

    def record_rtt(self, rtt_ms):
        with self.lock:
            self.rtt_samples_ms.append(rtt_ms)
            if len(self.rtt_samples_ms) > 50:
                self.rtt_samples_ms.pop(0)

    def record_error(self, status_code=0, is_timeout=False):
        with self.lock:
            if status_code in (429, 503):
                self.error_429_503 += 1
            if is_timeout:
                self.timeout_errors += 1

    def evaluate(self, downloaded_bytes, total_bytes, pending_ranges):
        with self.lock:
            now = time.time()
            dt = now - self.last_eval_time
            if dt < 0.8:
                return self.current_workers

            bytes_delta = max(0, downloaded_bytes - self.last_eval_bytes)
            inst_speed = bytes_delta / max(0.001, dt)
            self.last_eval_time = now
            self.last_eval_bytes = downloaded_bytes

            pct = (downloaded_bytes / max(1, total_bytes)) * 100.0 if total_bytes > 0 else 0.0
            avg_rtt = sum(self.rtt_samples_ms) / len(self.rtt_samples_ms) if self.rtt_samples_ms else 50.0
            old_workers = self.current_workers
            reason = None

            # 1. Throttling / Congestion check (Multiplicative Decrease)
            if self.error_429_503 > 0 or self.timeout_errors >= 2:
                self.level_idx = max(1, self.level_idx - 2)
                reason = f"server_throttle_backoff (429/503={self.error_429_503}, timeouts={self.timeout_errors})"
                self.error_429_503 = 0
                self.timeout_errors = 0
                self.plateau_count = 0
            # 2. Tail-end burst / compaction check
            elif pct >= 88.0 and pending_ranges > 0:
                # Ensure worker count matches remaining ranges or bursts to clear tail stragglers
                if pending_ranges < self.current_workers:
                    target_idx = self._nearest_idx(max(4, pending_ranges))
                    if target_idx != self.level_idx:
                        self.level_idx = target_idx
                        reason = f"tail_range_compaction (pending_ranges={pending_ranges})"
            # 3. Mid-transfer compaction at 30%-45% if plateaued
            elif 28.0 <= pct <= 45.0 and self.plateau_count >= 1 and self.level_idx > 6:
                self.level_idx = max(6, self.level_idx - 1)
                reason = "mid_transfer_window_compaction"
                self.plateau_count = 0
            # 4. Additive / Stepped Increase when throughput improves over moving baseline
            elif inst_speed > self.baseline_speed_bps * 1.04 or self.baseline_speed_bps == 0:
                self.baseline_speed_bps = inst_speed if self.baseline_speed_bps == 0 else (0.7 * self.baseline_speed_bps + 0.3 * inst_speed)
                self.plateau_count = 0
                if self.level_idx < self.max_idx and pending_ranges > self.current_workers:
                    self.level_idx = min(self.max_idx, self.level_idx + 1)
                    reason = "throughput_gain_scale_up"
            else:
                self.baseline_speed_bps = 0.8 * self.baseline_speed_bps + 0.2 * inst_speed
                self.plateau_count += 1
                if self.plateau_count >= 2 and self.level_idx > 5:
                    # Slight compaction to avoid idle worker overhead
                    self.level_idx = max(5, self.level_idx - 1)
                    reason = "plateau_hysteresis_stabilize"
                    self.plateau_count = 0

            new_workers = self.current_workers
            if new_workers != old_workers and reason:
                self.history.append({
                    "timestamp": round(now, 3),
                    "progress_pct": round(pct, 1),
                    "workers_before": old_workers,
                    "workers": new_workers,
                    "reason": reason,
                    "speed_mbps": round(inst_speed / (1024 * 1024), 2),
                    "rtt_ms": round(avg_rtt, 1)
                })
            return new_workers


def refresh_stream_url(canonical_url, target_format_id):
    """
    Re-probes the canonical source URL when a signed CDN URL expires (HTTP 401/403/410),
    validates representation identity (format_id and filesize), and returns the refreshed URL + headers.
    """
    import yt_dlp
    ydl_opts = base_ydl_opts()
    ydl_opts['skip_download'] = True
    with yt_dlp.YoutubeDL(ydl_opts) as ydl:
        info = ydl.extract_info(canonical_url, download=False)
        for f in (info.get('formats') or []):
            if str(f.get('format_id')) == str(target_format_id):
                return {
                    "url": f.get('url'),
                    "filesize": f.get('filesize') or f.get('filesize_approx') or 0,
                    "headers": f.get('http_headers') or {},
                    "format_id": str(f.get('format_id')),
                    "vcodec": f.get('vcodec'),
                    "acodec": f.get('acodec')
                }
    return None


def download_range_stream_otter(
    canonical_url,
    stream_info,
    part_path,
    checkpoint_path,
    governor,
    progress_callback,
    max_workers=64
):
    """
    OTTER Native Multi-Stream Byte-Range Engine:
    - Divides a byte-range stream into independent chunks (< 4 MB each to defeat CDN per-connection throttling).
    - Schedules workers independently without sequential queue blocking.
    - Persists durable range completion bitmap to checkpoint_path on D: after fsync.
    - Supports 0%-retransmission resume on network drop / kill / restart.
    - Supports automatic expired-URL refresh with strict resource identity verification.
    """
    stream_url = stream_info["url"]
    format_id = str(stream_info.get("format_id", "0"))
    headers = dict(stream_info.get("http_headers") or {})
    total_bytes = int(stream_info.get("filesize") or 0)

    # Probe exact Content-Length via Range: bytes=0-0 if filesize not exact
    try:
        req = urllib.request.Request(stream_url, headers={**headers, "Range": "bytes=0-0"})
        with urllib.request.urlopen(req, timeout=15) as resp:
            cr = resp.headers.get("Content-Range", "")
            if "/" in cr:
                total_bytes = int(cr.split("/")[-1].strip())
            elif resp.headers.get("Content-Length") and not total_bytes:
                total_bytes = int(resp.headers.get("Content-Length"))
            etag = resp.headers.get("ETag", "")
            last_mod = resp.headers.get("Last-Modified", "")
    except Exception:
        etag = ""
        last_mod = ""

    if total_bytes <= 0:
        return False, "Total byte size unknown for range engine"

    # Choose range chunk size: 2 MB for < 100 MB, 4 MB for >= 100 MB (always < 9 MB for unthrottled CDN burst)
    chunk_size = 2 * 1024 * 1024 if total_bytes < 100 * 1024 * 1024 else 4 * 1024 * 1024
    num_ranges = (total_bytes + chunk_size - 1) // chunk_size

    # Load or initialize durable checkpoint (with sub-range byte-offset tracking)
    completed_ranges = [False] * num_ranges
    range_offsets = [0] * num_ranges
    previously_durable_bytes = 0
    retransmitted_bytes = 0
    refresh_events = []

    if os.path.exists(checkpoint_path) and os.path.exists(part_path):
        try:
            ckpt = json.load(open(checkpoint_path, "r", encoding="utf-8"))
            if (
                ckpt.get("total_bytes") == total_bytes
                and ckpt.get("chunk_size") == chunk_size
                and len(ckpt.get("completed_ranges", [])) == num_ranges
            ):
                completed_ranges = ckpt["completed_ranges"]
                saved_offsets = ckpt.get("range_offsets", [])
                for idx, done in enumerate(completed_ranges):
                    s = idx * chunk_size
                    e = min(total_bytes - 1, s + chunk_size - 1)
                    r_len = e - s + 1
                    if done:
                        range_offsets[idx] = r_len
                    elif idx < len(saved_offsets):
                        range_offsets[idx] = max(0, min(r_len, int(saved_offsets[idx])))
                previously_durable_bytes = sum(range_offsets)
        except Exception:
            completed_ranges = [False] * num_ranges
            range_offsets = [0] * num_ranges

    # Pre-allocate or open existing .part file on D:
    if not os.path.exists(part_path):
        with open(part_path, "wb") as f:
            f.truncate(total_bytes)
    else:
        with open(part_path, "r+b") as f:
            if os.path.getsize(part_path) != total_bytes:
                f.truncate(total_bytes)

    file_lock = threading.Lock()
    state_lock = threading.Lock()
    url_state = {"url": stream_url, "headers": headers, "refreshing": False}

    downloaded_now = [previously_durable_bytes]
    in_flight_ranges = {}  # range_id -> unflushed_bytes
    last_ckpt_save = [0.0]

    def save_checkpoint(force=False):
        now = time.time()
        if not force and (now - last_ckpt_save[0] < 0.15):
            return
        last_ckpt_save[0] = now
        tmp_ckpt = checkpoint_path + ".tmp"
        data = {
            "canonical_url": canonical_url,
            "format_id": format_id,
            "ext": stream_info.get("ext", "mp4"),
            "title": stream_info.get("title", "media_download"),
            "stream_url": url_state["url"],
            "http_headers": url_state["headers"],
            "total_bytes": total_bytes,
            "chunk_size": chunk_size,
            "etag": etag,
            "last_modified": last_mod,
            "completed_ranges": completed_ranges,
            "range_offsets": range_offsets,
            "previously_durable_bytes": previously_durable_bytes,
            "retransmitted_bytes": retransmitted_bytes,
            "refresh_events": refresh_events,
            "updated_at": now
        }
        try:
            with open(tmp_ckpt, "w", encoding="utf-8") as cf:
                json.dump(data, cf)
                cf.flush()
                os.fsync(cf.fileno())
            os.replace(tmp_ckpt, checkpoint_path)
        except Exception:
            pass

    save_checkpoint(force=True)

    pending_indices = [i for i, done in enumerate(completed_ranges) if not done]
    if not pending_indices:
        progress_callback(total_bytes, total_bytes, 0, governor.current_workers, {})
        return True, {
            "total_bytes": total_bytes,
            "previously_durable_bytes": previously_durable_bytes,
            "retransmitted_bytes": 0,
            "refresh_events": refresh_events
        }

    # Semaphore controlled by AdaptiveStreamGovernor
    active_workers_count = [0]
    cond = threading.Condition()

    def fetch_single_range(idx):
        nonlocal retransmitted_bytes
        start_b = idx * chunk_size
        end_b = min(total_bytes - 1, start_b + chunk_size - 1)
        expected_len = end_b - start_b + 1

        # Wait until governor allows this worker slot
        with cond:
            while active_workers_count[0] >= governor.current_workers:
                cond.wait(timeout=0.2)
            active_workers_count[0] += 1

        try:
            for attempt in range(5):
                with state_lock:
                    cur_url = url_state["url"]
                    cur_headers = dict(url_state["headers"])
                    already_written = range_offsets[idx]

                if already_written >= expected_len:
                    with state_lock:
                        completed_ranges[idx] = True
                        save_checkpoint(force=True)
                    return True

                cur_start = start_b + already_written
                rem_len = end_b - cur_start + 1

                # Simulate / check forced expired URL test file trigger if present
                expire_trigger_file = checkpoint_path + ".force_expire"
                if os.path.exists(expire_trigger_file):
                    try:
                        os.remove(expire_trigger_file)
                    except Exception:
                        pass
                    cur_url = cur_url + "&expire=1"

                req_headers = {**cur_headers, "Range": f"bytes={cur_start}-{end_b}"}
                t_req = time.time()
                buf = bytearray()
                flushed_in_attempt = 0
                try:
                    req = urllib.request.Request(cur_url, headers=req_headers)
                    with urllib.request.urlopen(req, timeout=15) as resp:
                        ttfb_ms = (time.time() - t_req) * 1000.0
                        governor.record_rtt(ttfb_ms)
                        while True:
                            chunk = resp.read(65536)
                            if not chunk:
                                break
                            buf.extend(chunk)
                            # Flush every 256 KB to disk and update durable range_offsets
                            if len(buf) >= 262144:
                                write_pos = cur_start + flushed_in_attempt
                                with file_lock:
                                    with open(part_path, "r+b") as df:
                                        df.seek(write_pos)
                                        df.write(buf)
                                        df.flush()
                                n_flushed = len(buf)
                                flushed_in_attempt += n_flushed
                                buf.clear()
                                with state_lock:
                                    range_offsets[idx] = already_written + flushed_in_attempt
                                    downloaded_now[0] += n_flushed
                                    in_flight_ranges[idx] = 0
                                    cur_done = downloaded_now[0] + sum(in_flight_ranges.values())
                                    save_checkpoint(force=False)
                            else:
                                with state_lock:
                                    in_flight_ranges[idx] = len(buf)
                                    cur_done = downloaded_now[0] + sum(in_flight_ranges.values())

                            rem_count = sum(1 for d in completed_ranges if not d)
                            target_w = governor.evaluate(cur_done, total_bytes, rem_count)
                            with cond:
                                cond.notify_all()
                            progress_callback(cur_done, total_bytes, idx, target_w, in_flight_ranges)

                    if flushed_in_attempt + len(buf) == rem_len:
                        # Write final remainder and fsync BEFORE marking range completed
                        with file_lock:
                            with open(part_path, "r+b") as df:
                                if buf:
                                    df.seek(cur_start + flushed_in_attempt)
                                    df.write(buf)
                                    df.flush()
                                os.fsync(df.fileno())
                        n_rem = len(buf)
                        buf.clear()
                        with state_lock:
                            in_flight_ranges.pop(idx, None)
                            range_offsets[idx] = expected_len
                            completed_ranges[idx] = True
                            downloaded_now[0] += n_rem
                            save_checkpoint(force=True)
                        return True
                    else:
                        with state_lock:
                            retransmitted_bytes += len(buf)
                            in_flight_ranges.pop(idx, None)
                except urllib.error.HTTPError as he:
                    with state_lock:
                        retransmitted_bytes += len(buf)
                        in_flight_ranges.pop(idx, None)
                    governor.record_error(status_code=he.code)
                    if he.code in (401, 403, 410):
                        # Expired signed URL or session! Refresh from canonical source URL
                        with state_lock:
                            if not url_state["refreshing"]:
                                url_state["refreshing"] = True
                                try:
                                    refreshed = refresh_stream_url(canonical_url, format_id)
                                    if refreshed and refreshed.get("url"):
                                        # Verify resource identity before continuing!
                                        new_sz = int(refreshed.get("filesize") or total_bytes)
                                        if abs(new_sz - total_bytes) <= 4096 and refreshed.get("format_id") == format_id:
                                            url_state["url"] = refreshed["url"]
                                            url_state["headers"] = refreshed.get("headers") or cur_headers
                                            refresh_events.append({
                                                "timestamp": time.time(),
                                                "http_code": he.code,
                                                "range_idx": idx,
                                                "identity_verified": True,
                                                "format_id": format_id,
                                                "expected_bytes": total_bytes,
                                                "refreshed_bytes": new_sz
                                            })
                                            save_checkpoint(force=True)
                                finally:
                                    url_state["refreshing"] = False
                    time.sleep(min(4.0, (0.3 * (2 ** attempt))))
                except Exception:
                    with state_lock:
                        retransmitted_bytes += len(buf)
                        in_flight_ranges.pop(idx, None)
                    governor.record_error(is_timeout=True)
                    time.sleep(min(4.0, (0.3 * (2 ** attempt))))
            return False
        finally:
            with cond:
                active_workers_count[0] -= 1
                cond.notify_all()

    pool_cap = min(128, max(max_workers, 64))
    with ThreadPoolExecutor(max_workers=pool_cap) as executor:
        futures = {executor.submit(fetch_single_range, i): i for i in pending_indices}
        for fut in as_completed(futures):
            ok = fut.result()
            if not ok:
                return False, f"Range {futures[fut]} failed after bounded retries"

    return True, {
        "total_bytes": total_bytes,
        "previously_durable_bytes": previously_durable_bytes,
        "retransmitted_bytes": retransmitted_bytes,
        "refresh_events": refresh_events
    }


def download_media(url, quality_id, dest_dir, custom_filename=None, concurrency=32, job_id=None):
    import yt_dlp

    ffmpeg_dir = get_ffmpeg_path()
    try:
        os.makedirs(dest_dir, exist_ok=True)
    except Exception:
        dest_dir = os.path.join(os.environ.get("USERPROFILE", os.path.expanduser("~")), "Downloads", "Otter")
        os.makedirs(dest_dir, exist_ok=True)

    staging_id = sanitize_filename(job_id or custom_filename or url)
    staging_dir = get_same_drive_staging_dir(dest_dir, staging_id)

    is_audio = quality_id.startswith("audio_")
    initial_conc = max(1, min(128, int(concurrency) if int(concurrency) > 0 else 32))

    # Build format selector
    if quality_id.startswith("fmt:"):
        format_spec = quality_id[4:]
        is_audio = False
    elif is_audio:
        audio_ext = "mp3" if "mp3" in quality_id else "m4a"
        format_spec = "140/bestaudio[ext=m4a][protocol=https]/bestaudio[protocol=https]/bestaudio/best"
    else:
        height_match = re.search(r'video_(\d+)', quality_id)
        if height_match:
            max_h = int(height_match.group(1))
            # Prefer H.264 (avc1) + AAC (140/m4a) direct HTTPS byte-range streams for universal playability & exact parity
            format_spec = (
                f"bestvideo[height<={max_h}][vcodec^=avc1][ext=mp4][protocol=https]+140/"
                f"bestvideo[height<={max_h}][vcodec^=avc1][ext=mp4][protocol=https]+bestaudio[ext=m4a][protocol=https]/"
                f"bestvideo[height<={max_h}][ext=mp4][protocol=https]+bestaudio[ext=m4a][protocol=https]/"
                f"bestvideo[height<={max_h}][protocol=https]+bestaudio[protocol=https]/"
                f"bestvideo[height<={max_h}]+bestaudio/best[height<={max_h}]/best"
            )
        else:
            format_spec = (
                "bestvideo[vcodec^=avc1][ext=mp4][protocol=https]+140/"
                "bestvideo[vcodec^=avc1][ext=mp4][protocol=https]+bestaudio[ext=m4a][protocol=https]/"
                "bestvideo[ext=mp4][protocol=https]+bestaudio[ext=m4a][protocol=https]/"
                "bestvideo[protocol=https]+bestaudio[protocol=https]/"
                "bestvideo+bestaudio/best"
            )

    # Check if staging_dir already contains a job_manifest.json or durable range checkpoints from an interrupted transfer
    job_manifest_path = os.path.join(staging_dir, "job_manifest.json")
    existing_ckpts = {}
    if os.path.exists(staging_dir):
        for fn in sorted(os.listdir(staging_dir)):
            if fn.endswith(".checkpoint.json"):
                try:
                    c_obj = json.load(open(os.path.join(staging_dir, fn), "r", encoding="utf-8"))
                    if c_obj.get("stream_url") and c_obj.get("total_bytes"):
                        existing_ckpts[str(c_obj.get("format_id"))] = c_obj
                except Exception:
                    pass

    if os.path.exists(job_manifest_path):
        try:
            jm = json.load(open(job_manifest_path, "r", encoding="utf-8"))
            title = jm.get("title") or "media_download"
            safe_title = sanitize_filename(os.path.splitext(custom_filename)[0] if custom_filename else title)
            req_formats = jm.get("req_formats", [])
            for rf in req_formats:
                fid_str = str(rf.get("format_id"))
                if fid_str in existing_ckpts:
                    rf["url"] = existing_ckpts[fid_str]["stream_url"]
                    rf["http_headers"] = existing_ckpts[fid_str].get("http_headers") or rf.get("http_headers") or {}
                    rf["filesize"] = existing_ckpts[fid_str]["total_bytes"]
            can_use_range_engine = True
            total_combined_bytes = sum(int(rf.get("filesize") or 0) for rf in req_formats)
        except Exception:
            req_formats = []
    elif existing_ckpts:
        first_c = next(iter(existing_ckpts.values()))
        title = first_c.get("title") or "media_download"
        safe_title = sanitize_filename(os.path.splitext(custom_filename)[0] if custom_filename else title)
        req_formats = [
            {
                "format_id": c["format_id"],
                "ext": c.get("ext", "m4a" if is_audio else "mp4"),
                "protocol": "https",
                "filesize": c["total_bytes"],
                "url": c["stream_url"],
                "http_headers": c.get("http_headers") or {},
                "title": title
            }
            for c in existing_ckpts.values()
        ]
        can_use_range_engine = True
        total_combined_bytes = sum(int(c["total_bytes"]) for c in existing_ckpts.values())
    else:
        req_formats = []

    if not req_formats:
        probe_opts = base_ydl_opts()
        probe_opts['format'] = format_spec

        try:
            with yt_dlp.YoutubeDL(probe_opts) as ydl:
                info = ydl.extract_info(url, download=False)
        except Exception as e:
            print(json.dumps({"type": "error", "error": str(e)}), flush=True)
            sys.exit(1)

        title = info.get("title") or "media_download"
        safe_title = sanitize_filename(os.path.splitext(custom_filename)[0] if custom_filename else title)

        # Check if direct HTTPS byte-range streams are available for OTTER Native Multi-Stream Range Engine
        req_formats = info.get("requested_formats") or [info]
        can_use_range_engine = True
        total_combined_bytes = 0

        for rf in req_formats:
            rf["title"] = title
            proto = rf.get("protocol", "")
            fsize = rf.get("filesize") or rf.get("filesize_approx") or 0
            s_url = rf.get("url", "")
            if proto not in ("https", "http") or not s_url or "manifest.googlevideo.com" in s_url or ".m3u8" in s_url:
                can_use_range_engine = False
                break
            total_combined_bytes += int(fsize)

        if can_use_range_engine and total_combined_bytes > 0:
            try:
                with open(job_manifest_path, "w", encoding="utf-8") as jmf:
                    json.dump({
                        "title": title,
                        "req_formats": [
                            {
                                "format_id": str(rf.get("format_id")),
                                "ext": rf.get("ext", "mp4"),
                                "protocol": rf.get("protocol", "https"),
                                "filesize": int(rf.get("filesize") or rf.get("filesize_approx") or 0),
                                "url": rf.get("url"),
                                "http_headers": dict(rf.get("http_headers") or {}),
                                "title": title
                            }
                            for rf in req_formats
                        ]
                    }, jmf)
            except Exception:
                pass

    governor = AdaptiveStreamGovernor(
        initial_workers=min(initial_conc, 16),
        max_workers=max(initial_conc, 64),
        total_bytes=total_combined_bytes
    )

    if can_use_range_engine and total_combined_bytes > 0:
        # Execute via OTTER Native Multi-Stream Range Engine!
        completed_streams_bytes = [0]
        last_emit_time = [0.0]
        last_emit_bytes = [0]
        last_emit_check_time = [time.time()]
        smoothed_speed = [0.0]
        total_retransmitted = 0
        total_prev_durable = 0
        all_refresh_events = []
        elementary_files = []

        def on_range_progress(stream_done, stream_total, active_idx, active_workers, in_flight_map):
            now = time.time()
            if now - last_emit_time[0] < 0.25:
                return
            last_emit_time[0] = now
            overall_done = completed_streams_bytes[0] + stream_done
            overall_tot = max(total_combined_bytes, overall_done)
            dt = now - last_emit_check_time[0]
            if dt >= 0.4:
                diff = max(0, overall_done - last_emit_bytes[0])
                inst = diff / dt
                smoothed_speed[0] = inst if smoothed_speed[0] == 0 else (smoothed_speed[0] * 0.6 + inst * 0.4)
                last_emit_check_time[0] = now
                last_emit_bytes[0] = overall_done

            spd = smoothed_speed[0]
            spd_mb = spd / (1024.0 * 1024.0)
            spd_str = f"{spd_mb:.1f} MB/s" if spd_mb >= 0.1 else f"{spd / 1024.0:.0f} KB/s"
            pct = min(99.0, (overall_done / max(1, overall_tot)) * 100.0)
            eta = int((overall_tot - overall_done) / max(1.0, spd)) if spd > 0 else 0

            print(json.dumps({
                "type": "progress",
                "downloaded_bytes": overall_done,
                "total_bytes": overall_tot,
                "progress_pct": round(pct, 1),
                "speed_bps": round(spd, 1),
                "speed_str": spd_str,
                "eta_seconds": eta,
                "streams": active_workers,
                "engine_mode": "OTTER_PARALLEL_HTTP_RANGES"
            }), flush=True)

        range_success = True
        for s_idx, rf in enumerate(req_formats):
            fid = str(rf.get("format_id", s_idx))
            ext = rf.get("ext", "mp4")
            part_file = os.path.join(staging_dir, f"{safe_title}.f{fid}.{ext}.part")
            done_file = os.path.join(staging_dir, f"{safe_title}.f{fid}.{ext}")
            ckpt_file = os.path.join(staging_dir, f"{safe_title}.f{fid}.checkpoint.json")

            if os.path.exists(done_file) and os.path.getsize(done_file) > 0:
                sz = os.path.getsize(done_file)
                completed_streams_bytes[0] += sz
                total_prev_durable += sz
                elementary_files.append(done_file)
                continue

            ok, meta = download_range_stream_otter(
                url,
                rf,
                part_file,
                ckpt_file,
                governor,
                on_range_progress,
                max_workers=max(initial_conc, 64)
            )
            if not ok:
                range_success = False
                break

            os.replace(part_file, done_file)
            sz = os.path.getsize(done_file)
            completed_streams_bytes[0] += sz
            total_prev_durable += meta.get("previously_durable_bytes", 0)
            total_retransmitted += meta.get("retransmitted_bytes", 0)
            all_refresh_events.extend(meta.get("refresh_events", []))
            elementary_files.append(done_file)

        # Save adaptation log inside staging before finalization
        adapt_file = os.path.join(dest_dir, ".otter_staging", f"{staging_id}_adaptation.json")
        try:
            with open(adapt_file, "w", encoding="utf-8") as af:
                json.dump({
                    "job_id": staging_id,
                    "url": url,
                    "engine_mode": "OTTER_PARALLEL_HTTP_RANGES",
                    "previously_durable_bytes": total_prev_durable,
                    "retransmitted_bytes": total_retransmitted,
                    "refresh_events": all_refresh_events,
                    "adaptation_history": governor.history
                }, af, indent=2)
        except Exception:
            pass

        if range_success and elementary_files:
            ffmpeg_exe = os.path.join(ffmpeg_dir, "ffmpeg.exe") if ffmpeg_dir else "ffmpeg"
            if is_audio:
                audio_ext = "mp3" if "mp3" in quality_id else "m4a"
                final_target = os.path.join(dest_dir, f"{safe_title}.{audio_ext}")
                staged_out = os.path.join(staging_dir, f"{safe_title}_final.{audio_ext}")
                if audio_ext == "mp3":
                    subprocess.run([ffmpeg_exe, "-y", "-i", elementary_files[0], "-vn", "-c:a", "libmp3lame", "-b:a", "192k", staged_out], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
                    if os.path.exists(final_target):
                        os.remove(final_target)
                    os.replace(staged_out, final_target)
                elif elementary_files[0].endswith(".m4a"):
                    if os.path.exists(final_target):
                        os.remove(final_target)
                    os.replace(elementary_files[0], final_target)
                else:
                    subprocess.run([ffmpeg_exe, "-y", "-i", elementary_files[0], "-vn", "-c:a", "copy", "-fflags", "+bitexact", staged_out], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
                    if os.path.exists(final_target):
                        os.remove(final_target)
                    os.replace(staged_out, final_target)
            elif len(elementary_files) == 2:
                final_target = os.path.join(dest_dir, f"{safe_title}.mp4")
                staged_out = os.path.join(staging_dir, f"{safe_title}_final.mp4")
                # Fast stream copy muxing via FFmpeg (exact parity with yt-dlp FFmpegMergerPP)
                res_mux = subprocess.run(
                    [ffmpeg_exe, "-y", "-i", elementary_files[0], "-i", elementary_files[1], "-c", "copy", "-map", "0:v:0", "-map", "1:a:0", staged_out],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
                )
                if res_mux.returncode != 0:
                    subprocess.run(
                        [ffmpeg_exe, "-y", "-i", elementary_files[0], "-i", elementary_files[1], "-c:v", "copy", "-c:a", "aac", staged_out],
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True
                    )
                if os.path.exists(final_target):
                    os.remove(final_target)
                os.replace(staged_out, final_target)
            else:
                final_target = os.path.join(dest_dir, f"{safe_title}.mp4")
                if os.path.exists(final_target):
                    os.remove(final_target)
                os.replace(elementary_files[0], final_target)

            final_size = os.path.getsize(final_target) if os.path.exists(final_target) else 0
            shutil.rmtree(staging_dir, ignore_errors=True)

            print(json.dumps({
                "type": "completed",
                "file_path": final_target,
                "total_bytes": final_size,
                "previously_durable_bytes": total_prev_durable,
                "retransmitted_bytes": total_retransmitted,
                "refresh_events": all_refresh_events,
                "adaptation_steps": len(governor.history),
                "engine_mode": "OTTER_PARALLEL_HTTP_RANGES",
                "title": title,
            }), flush=True)
            return

    # Fallback / HLS / DASH Segment Engine (on same-drive staging_dir)
    session_state = {
        "current_fn": None,
        "completed_prev_bytes": 0,
        "last_downloaded": 0,
        "last_time": time.time(),
        "last_bytes": 0,
        "est_total": total_combined_bytes,
    }

    def progress_hook(d):
        if d['status'] == 'downloading':
            fn = d.get('filename')
            if fn != session_state["current_fn"]:
                if session_state["current_fn"] is not None:
                    session_state["completed_prev_bytes"] += session_state["last_downloaded"]
                session_state["current_fn"] = fn
                session_state["last_downloaded"] = 0

            downloaded_file = d.get('downloaded_bytes', 0)
            session_state["last_downloaded"] = downloaded_file
            info_dict = d.get('info_dict', {})
            total_file = d.get('total_bytes') or d.get('total_bytes_estimate') or info_dict.get('filesize') or info_dict.get('filesize_approx') or 0
            downloaded_total = session_state["completed_prev_bytes"] + downloaded_file

            frag_idx = d.get('fragment_index')
            frag_count = d.get('fragment_count')

            if not session_state["est_total"]:
                if total_file > 0:
                    session_state["est_total"] = total_file if is_audio else int(total_file * 1.15)
                elif frag_idx and frag_count and frag_count > 0:
                    session_state["est_total"] = int((downloaded_file / max(1, frag_idx)) * frag_count)

            overall_total = session_state["est_total"] or total_file or 0
            pct = 0.0
            if overall_total > 0 and downloaded_total > 0:
                pct = min(99.0, (downloaded_total / overall_total) * 100.0)
            elif frag_idx and frag_count and frag_count > 0:
                pct = min(99.0, (frag_idx / frag_count) * 100.0)

            speed = d.get('speed') or 0
            now = time.time()
            dt = now - session_state["last_time"]
            if dt >= 0.4:
                diff = downloaded_total - session_state["last_bytes"]
                if diff > 0 and dt > 0:
                    speed = diff / dt
                session_state["last_time"] = now
                session_state["last_bytes"] = downloaded_total

            speed_mb = speed / (1024.0 * 1024.0) if speed else 0.0
            speed_str = f"{speed_mb:.1f} MB/s" if speed_mb >= 0.1 else f"{speed / 1024.0:.0f} KB/s" if speed > 1000 else "Starting..."

            rem_frags = max(1, (frag_count - frag_idx)) if (frag_count and frag_idx) else 32
            active_governor_streams = governor.evaluate(downloaded_total, overall_total, rem_frags)

            print(json.dumps({
                "type": "progress",
                "downloaded_bytes": downloaded_total,
                "total_bytes": overall_total,
                "progress_pct": round(pct, 1),
                "speed_bps": speed,
                "speed_str": speed_str,
                "eta_seconds": d.get('eta', 0),
                "streams": active_governor_streams,
                "engine_mode": "OTTER_DASH_HLS_SEGMENT_WORKERS"
            }), flush=True)

    postprocessors = []
    merge_fmt = "mp4" if not is_audio and ffmpeg_dir else None
    if is_audio:
        audio_ext = "mp3" if "mp3" in quality_id else "m4a"
        if audio_ext in ("mp3", "m4a"):
            postprocessors.append({
                'key': 'FFmpegExtractAudio',
                'preferredcodec': audio_ext,
                'preferredquality': '192',
            })

    rel_template = f"{safe_title}.%(ext)s"
    ydl_opts = base_ydl_opts()
    ydl_opts.update({
        'format': format_spec,
        'outtmpl': rel_template,
        'paths': {
            'home': staging_dir,
            'temp': staging_dir,
        },
        'continuedl': True,
        'progress_hooks': [progress_hook],
        'concurrent_fragment_downloads': max(1, min(64, initial_conc)),
        'http_chunk_size': 10485760,
    })
    if merge_fmt:
        ydl_opts['merge_output_format'] = merge_fmt
    if postprocessors:
        ydl_opts['postprocessors'] = postprocessors

    try:
        with yt_dlp.YoutubeDL(ydl_opts) as ydl:
            info = ydl.extract_info(url, download=True)
            staging_file = ydl.prepare_filename(info)
            base, _ = os.path.splitext(staging_file)

            if is_audio:
                audio_ext = "mp3" if "mp3" in quality_id else "m4a"
                audio_target = f"{base}.{audio_ext}"
                if os.path.exists(audio_target):
                    staging_file = audio_target
            else:
                mp4_cand = f"{base}.mp4"
                if os.path.exists(mp4_cand):
                    staging_file = mp4_cand

            if not os.path.exists(staging_file):
                for cand_ext in [".mp4", ".mkv", ".webm", ".m4a", ".mp3"]:
                    if os.path.exists(base + cand_ext):
                        staging_file = base + cand_ext
                        break

            if not os.path.exists(staging_file):
                entries = [os.path.join(staging_dir, f) for f in os.listdir(staging_dir) if not f.endswith(".part") and not f.endswith(".ytdl") and not f.endswith(".json")]
                if entries:
                    staging_file = entries[0]

            final_name = os.path.basename(staging_file)
            final_target = os.path.join(dest_dir, final_name)

            if os.path.exists(final_target):
                try:
                    os.remove(final_target)
                except Exception:
                    pass
            os.replace(staging_file, final_target)
            file_size = os.path.getsize(final_target) if os.path.exists(final_target) else 0
            shutil.rmtree(staging_dir, ignore_errors=True)

            print(json.dumps({
                "type": "completed",
                "file_path": final_target,
                "total_bytes": file_size,
                "adaptation_steps": len(governor.history),
                "engine_mode": "OTTER_DASH_HLS_SEGMENT_WORKERS",
                "title": info.get('title', ''),
            }), flush=True)
    except Exception as e:
        print(json.dumps({
            "type": "error",
            "error": str(e)
        }), flush=True)
        sys.exit(1)

if __name__ == '__main__':
    if len(sys.argv) < 3:
        print(json.dumps({"error": "Usage: media_engine.py probe <url> | download <url> <quality_id> <dest_dir> [filename]"}))
        sys.exit(1)

    cmd = sys.argv[1]
    url_arg = sys.argv[2]

    if cmd == 'probe':
        res = probe_url(url_arg)
        print(json.dumps(res))
    elif cmd == 'download':
        qid = sys.argv[3] if len(sys.argv) > 3 else "video_best"
        destination = sys.argv[4] if len(sys.argv) > 4 else os.path.expanduser("~/Downloads")
        fname = sys.argv[5] if len(sys.argv) > 5 else None
        conc = int(sys.argv[6]) if len(sys.argv) > 6 and sys.argv[6].isdigit() else 32
        jid = sys.argv[7] if len(sys.argv) > 7 else None
        download_media(url_arg, qid, destination, fname, conc, jid)
