import concurrent.futures
import hashlib
import json
import os
import re
import shutil
import struct
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path


class AdaptiveWorkerGovernor:
    """
    Dynamic stream/worker controller across [1..128].
    Adapts concurrency based on measured goodput delta, RTT, HTTP 429/503 signals, and disk write latency.
    """

    def __init__(self, initial_workers: int = 16, min_workers: int = 4, max_workers: int = 48):
        self.current_workers = max(min_workers, min(max_workers, initial_workers))
        self.min_workers = min_workers
        self.max_workers = max_workers
        self.last_eval_time = time.time()
        self.last_bytes = 0
        self.prev_goodput_bps = 0.0
        self.rate_limits = 0
        self.transient_errors = 0
        self.avg_rtt_ms = 50.0
        self.avg_disk_write_ms = 1.0
        self.history = [
            {
                "t_sec": 0.0,
                "workers": self.current_workers,
                "goodput_mbps": 0.0,
                "rtt_ms": 50.0,
                "reason": "initial_allocation",
            }
        ]
        self.start_time = time.time()
        self._lock = threading.Lock()

    def record_http_event(self, rtt_ms: float = None, rate_limited: bool = False, error: bool = False):
        with self._lock:
            if rtt_ms is not None and rtt_ms > 0:
                self.avg_rtt_ms = 0.7 * self.avg_rtt_ms + 0.3 * rtt_ms
            if rate_limited:
                self.rate_limits += 1
            if error:
                self.transient_errors += 1

    def record_disk_write(self, write_ms: float):
        with self._lock:
            self.avg_disk_write_ms = 0.85 * self.avg_disk_write_ms + 0.15 * write_ms

    def evaluate(self, current_bytes: int, remaining_bytes: int, eval_interval: float = 2.0) -> int:
        with self._lock:
            now = time.time()
            dt = now - self.last_eval_time
            if dt < eval_interval:
                return self.current_workers

            delta_bytes = max(0, current_bytes - self.last_bytes)
            goodput_bps = delta_bytes / dt if dt > 0 else 0.0
            goodput_mbps = goodput_bps / (1024 * 1024)

            self.last_eval_time = now
            self.last_bytes = current_bytes

            old_w = self.current_workers
            reason = "steady"

            if self.rate_limits > 0:
                self.current_workers = max(self.min_workers, self.current_workers // 2)
                self.rate_limits = 0
                self.transient_errors = 0
                self.prev_goodput_bps = 0.0
                reason = "rate_limit_backoff_429"
            elif self.transient_errors >= 4:
                self.current_workers = max(self.min_workers, int(self.current_workers * 0.75))
                self.transient_errors = 0
                reason = "transient_error_backoff"
            elif self.avg_disk_write_ms > 250.0:
                self.current_workers = max(self.min_workers, int(self.current_workers * 0.85))
                self.avg_disk_write_ms = 20.0
                reason = "disk_write_pressure"
            elif remaining_bytes > 0 and remaining_bytes < 16 * 1024 * 1024 and self.current_workers > 8:
                self.current_workers = max(4, self.current_workers // 2)
                reason = "tail_completion_taper"
            elif self.avg_rtt_ms > 450.0 and self.current_workers > 16:
                # Bufferbloat / link saturation guard: trim workers when RTT exceeds 450ms
                self.current_workers = max(12, self.current_workers - 8)
                reason = f"rtt_bufferbloat_guard_{self.avg_rtt_ms:.0f}ms"
            else:
                self.transient_errors = 0
                if self.prev_goodput_bps == 0.0:
                    step = 8 if self.current_workers < 24 else 4
                    self.current_workers = min(self.max_workers, self.current_workers + step)
                    reason = "initial_ramp_probe"
                else:
                    gain = (goodput_bps - self.prev_goodput_bps) / max(1.0, self.prev_goodput_bps)
                    if gain >= 0.08 and self.current_workers < self.max_workers and self.avg_rtt_ms < 350.0:
                        step = 8 if gain >= 0.20 else 4
                        self.current_workers = min(self.max_workers, self.current_workers + step)
                        reason = f"goodput_gain_+{gain*100:.1f}pct"
                    elif gain < -0.15 and self.current_workers > 8:
                        self.current_workers = max(8, self.current_workers - 4)
                        reason = f"congestion_trim_{gain*100:.1f}pct"

                self.prev_goodput_bps = goodput_bps

            if self.current_workers != old_w or len(self.history) < 3:
                self.history.append(
                    {
                        "t_sec": round(now - self.start_time, 2),
                        "old_workers": old_w,
                        "workers": self.current_workers,
                        "goodput_mbps": round(goodput_mbps, 2),
                        "rtt_ms": round(self.avg_rtt_ms, 1),
                        "disk_write_ms": round(self.avg_disk_write_ms, 2),
                        "reason": reason,
                    }
                )
            return self.current_workers


def classify_ai_input(raw_input: str) -> dict:
    s = raw_input.strip()
    lower = s.lower()

    # 1. Ollama
    if (
        lower.startswith("ollama pull ")
        or lower.startswith("ollama run ")
        or lower.startswith("ollama:")
        or "ollama.com/" in lower
        or "registry.ollama.ai" in lower
    ):
        clean = s
        if lower.startswith("ollama pull "):
            clean = s[12:].strip()
        elif lower.startswith("ollama run "):
            clean = s[11:].strip()
        elif lower.startswith("ollama:"):
            clean = s[7:].strip()
        elif "ollama.com/" in lower:
            idx = lower.find("ollama.com/")
            clean = s[idx + 11 :].strip().strip("/")
            if clean.startswith("library/"):
                clean = clean[8:]

        if ":" in clean:
            model_part, tag = clean.split(":", 1)
        else:
            model_part, tag = clean, "latest"

        if "/" in model_part:
            ns, name = model_part.split("/", 1)
        else:
            ns, name = "library", model_part

        return {
            "kind": "ollama",
            "namespace": ns,
            "model_name": name,
            "tag": tag,
            "display_id": f"{name}:{tag}" if ns == "library" else f"{ns}/{name}:{tag}",
        }

    # 2. Kaggle
    if lower.startswith("kaggle ") or lower.startswith("kaggle:") or "kaggle.com/" in lower:
        owner, slug, version = None, None, 1
        if lower.startswith("kaggle "):
            parts = s.split()
            for i, p in enumerate(parts):
                if p in ("-d", "--dataset") and i + 1 < len(parts):
                    target = parts[i + 1].strip("/")
                    if "/" in target:
                        owner, slug = target.split("/", 1)[:2]
                        break
            if not owner and "/" in parts[-1]:
                owner, slug = parts[-1].strip("/").split("/", 1)[:2]
        elif lower.startswith("kaggle:"):
            target = s[7:].strip().strip("/")
            if "/" in target:
                owner, slug = target.split("/", 1)[:2]
        elif "kaggle.com/" in lower:
            if "/datasets/download/" in lower:
                after = s[lower.find("/datasets/download/") + 19 :].split("?")[0].strip("/")
                segs = after.split("/")
                if len(segs) >= 2:
                    owner, slug = segs[0], segs[1]
            elif "/datasets/" in lower:
                after = s[lower.find("/datasets/") + 10 :].split("?")[0].strip("/")
                segs = after.split("/")
                if len(segs) >= 2:
                    owner, slug = segs[0], segs[1]

        if owner and slug:
            return {
                "kind": "kaggle",
                "owner": owner,
                "slug": slug,
                "version": version,
                "display_id": f"{owner}/{slug}",
            }

    # 3. Hugging Face
    if "huggingface.co/" in lower or lower.startswith("hf:"):
        if lower.startswith("hf:"):
            path_part = s[3:].strip().strip("/")
        else:
            parsed = urllib.parse.urlparse(s)
            path_part = parsed.path.strip("/")

        segs = [x for x in path_part.split("/") if x]
        if len(segs) >= 3 and segs[0].lower() == "datasets":
            repo_id = f"{segs[1]}/{segs[2]}"
            single_file = None
            if len(segs) >= 6 and segs[3] in ("resolve", "blob"):
                single_file = "/".join(segs[5:])
            return {
                "kind": "hf_dataset",
                "repo_id": repo_id,
                "revision": segs[4] if len(segs) >= 5 and segs[3] in ("resolve", "blob") else "main",
                "single_file": single_file,
                "display_id": repo_id,
            }
        elif len(segs) >= 2:
            repo_id = f"{segs[0]}/{segs[1]}"
            single_file = None
            if len(segs) >= 5 and segs[2] in ("resolve", "blob"):
                single_file = "/".join(segs[4:])
            return {
                "kind": "hf_model",
                "repo_id": repo_id,
                "revision": segs[3] if len(segs) >= 4 and segs[2] in ("resolve", "blob") else "main",
                "single_file": single_file,
                "display_id": repo_id,
            }

    return {"kind": "unknown", "raw": s}


def resolve_ollama_manifest(ns: str, name: str, tag: str) -> dict:
    url = f"https://registry.ollama.ai/v2/{ns}/{name}/manifests/{tag}"
    req = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.docker.distribution.manifest.v2+json",
            "User-Agent": "otter-engine/0.1.0",
        },
    )
    with urllib.request.urlopen(req, timeout=20) as resp:
        raw_bytes = resp.read()
    manifest = json.loads(raw_bytes.decode("utf-8"))

    seen_digests = set()
    items = []
    dedup_skipped = 0

    cfg = manifest.get("config", {})
    cfg_digest = cfg.get("digest", "").replace("sha256:", "")
    if cfg_digest:
        seen_digests.add(cfg_digest)
        items.append(
            {
                "rfilename": f"blobs/sha256-{cfg_digest}",
                "role": "OCI Config",
                "media_type": cfg.get("mediaType", ""),
                "size": int(cfg.get("size", 0)),
                "sha256": cfg_digest,
                "canonical_url": f"https://registry.ollama.ai/v2/{ns}/{name}/blobs/sha256:{cfg_digest}",
            }
        )

    for layer in manifest.get("layers", []):
        dg = layer.get("digest", "").replace("sha256:", "")
        if not dg:
            continue
        if dg in seen_digests:
            dedup_skipped += 1
            continue
        seen_digests.add(dg)
        mtype = layer.get("mediaType", "")
        role = (
            "Model Weights (GGUF)"
            if "model" in mtype
            else "Prompt Template"
            if "template" in mtype
            else "License"
            if "license" in mtype
            else "Parameters"
            if "params" in mtype
            else "Layer Blob"
        )
        items.append(
            {
                "rfilename": f"blobs/sha256-{dg}",
                "role": role,
                "media_type": mtype,
                "size": int(layer.get("size", 0)),
                "sha256": dg,
                "canonical_url": f"https://registry.ollama.ai/v2/{ns}/{name}/blobs/sha256:{dg}",
            }
        )

    return {
        "namespace": ns,
        "model_name": name,
        "tag": tag,
        "raw_manifest_json": raw_bytes.decode("utf-8"),
        "manifest_rel_path": f"manifests/registry.ollama.ai/{ns}/{name}/{tag}",
        "unique_blob_count": len(items),
        "deduplicated_layers_skipped": dedup_skipped,
        "total_bytes": sum(i["size"] for i in items),
        "items": items,
    }


def resolve_hf_repository(repo_type: str, repo_id: str, revision: str = "main", filter_spec: str = None) -> dict:
    api_prefix = "datasets" if repo_type == "hf_dataset" else "models"
    token = os.environ.get("HF_TOKEN") or os.environ.get("HUGGING_FACE_HUB_TOKEN")
    headers = {"User-Agent": "otter-engine/0.1.0"}
    if token:
        headers["Authorization"] = f"Bearer {token}"

    # Query /tree/{rev}?recursive=true to get exact sizes AND LFS sha256 digests for all files/subdirectories
    tree_url = f"https://huggingface.co/api/{api_prefix}/{repo_id}/tree/{revision}?recursive=true"
    req = urllib.request.Request(tree_url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=25) as resp:
            tree_items = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            raise RuntimeError(
                f"Authentication required or gated Hugging Face repository (HTTP {e.code}): '{repo_id}'. Configure HF_TOKEN."
            )
        raise RuntimeError(f"Hugging Face API error (HTTP {e.code}) for '{repo_id}'")

    items = []
    for entry in tree_items:
        if entry.get("type") != "file":
            continue
        rfn = entry.get("path", "")
        if not rfn or rfn.startswith(".git") or rfn == ".gitattributes":
            continue

        if filter_spec and filter_spec not in ("all", "repo_all", "video_best"):
            # If user selected a specific file or quantization filter
            if filter_spec.startswith("file:"):
                target_fn = filter_spec[5:]
                if rfn != target_fn:
                    continue
            elif filter_spec.startswith("quant:"):
                q = filter_spec[6:].lower()
                if q not in rfn.lower() and not rfn.endswith((".json", ".txt", "LICENSE", "README.md")):
                    continue

        lfs = entry.get("lfs")
        size = int(lfs.get("size") if lfs and "size" in lfs else entry.get("size", 0))
        sha256 = lfs.get("oid") if lfs and "oid" in lfs else None

        url_prefix = f"https://huggingface.co/datasets/{repo_id}" if repo_type == "hf_dataset" else f"https://huggingface.co/{repo_id}"
        dl_url = f"{url_prefix}/resolve/{revision}/{urllib.parse.quote(rfn, safe='/')}"

        items.append(
            {
                "rfilename": rfn,
                "role": "LFS Shard/Weight" if lfs else "Metadata/Config",
                "size": size,
                "sha256": sha256,
                "canonical_url": dl_url,
            }
        )

    return {
        "repo_type": repo_type,
        "repo_id": repo_id,
        "revision": revision,
        "file_count": len(items),
        "total_bytes": sum(i["size"] for i in items),
        "items": items,
    }


def resolve_kaggle_dataset(owner: str, slug: str, version: int = 1) -> dict:
    api_url = f"https://www.kaggle.com/api/v1/datasets/download/{owner}/{slug}?datasetVersionNumber={version}"
    headers = {"User-Agent": "kagglehub/0.3.0 otter-engine/0.1.0", "Range": "bytes=0-0"}

    # Check optional Kaggle credentials
    username = os.environ.get("KAGGLE_USERNAME")
    key = os.environ.get("KAGGLE_KEY")
    if not (username and key):
        cred_file = Path.home() / ".kaggle" / "kaggle.json"
        if cred_file.exists():
            try:
                creds = json.loads(cred_file.read_text(encoding="utf-8"))
                username = creds.get("username")
                key = creds.get("key")
            except Exception:
                pass

    if username and key:
        import base64
        b64 = base64.b64encode(f"{username}:{key}".encode("utf-8")).decode("ascii")
        headers["Authorization"] = f"Basic {b64}"

    req = urllib.request.Request(api_url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=25) as resp:
            signed_url = resp.geturl()
            cr = resp.headers.get("Content-Range", "")
            cl = resp.headers.get("Content-Length", "")
            etag = resp.headers.get("ETag")
            if "/" in cr:
                total_size = int(cr.split("/")[-1])
            elif cl.isdigit():
                total_size = int(cl)
            else:
                total_size = 0
    except urllib.error.HTTPError as e:
        if e.code in (401, 403):
            raise RuntimeError(
                f"Authentication required for Kaggle dataset '{owner}/{slug}' (HTTP {e.code}). Configure KAGGLE_USERNAME/KAGGLE_KEY or ~/.kaggle/kaggle.json."
            )
        raise RuntimeError(f"Kaggle dataset resolution failed (HTTP {e.code}) for '{owner}/{slug}'")

    archive_fn = f"{slug}.zip"
    return {
        "owner": owner,
        "slug": slug,
        "version": version,
        "archive_filename": archive_fn,
        "total_bytes": total_size,
        "etag": etag,
        "items": [
            {
                "rfilename": archive_fn,
                "role": "Kaggle Dataset Archive (ZIP)",
                "size": total_size,
                "sha256": None,
                "canonical_url": api_url,
                "signed_url": signed_url,
                "etag": etag,
            }
        ],
    }


def probe_ai_resource(raw_input: str, dest_dir: str = None) -> dict:
    info = classify_ai_input(raw_input)
    kind = info.get("kind")
    if kind == "unknown":
        return {"is_valid": False, "is_ai_workload": False, "error": "Unrecognized AI resource URL or command"}

    try:
        if kind == "ollama":
            res = resolve_ollama_manifest(info["namespace"], info["model_name"], info["tag"])
            clean_name = f"ollama_{info['model_name']}_{info['tag']}".replace(":", "_").replace("/", "_")
            quality_options = [
                {
                    "id": "ollama_store",
                    "label": f"Full Ollama Model ({res['unique_blob_count']} Verified OCI Layers • {res['total_bytes']/(1024**2):.1f} MB)",
                    "type": "ai_model",
                }
            ]
            return {
                "is_valid": True,
                "is_ai_workload": True,
                "ai_kind": "ollama",
                "platform_name": "Ollama Registry",
                "content_type_label": f"Ollama Model ({res['unique_blob_count']} Unique Layers, {res['deduplicated_layers_skipped']} Deduplicated)",
                "title": f"Ollama Model: {info['display_id']}",
                "suggested_filename": clean_name,
                "expected_size_bytes": res["total_bytes"],
                "file_count": res["unique_blob_count"],
                "quality_options": quality_options,
                "default_quality_id": "ollama_store",
                "files_inventory": res["items"],
                "auth_note": "Public Ollama registry model. Content-addressed SHA-256 verification enabled.",
                "raw_url": raw_input,
                "url": raw_input,
            }

        elif kind in ("hf_model", "hf_dataset"):
            filter_spec = f"file:{info['single_file']}" if info.get("single_file") else None
            res = resolve_hf_repository(kind, info["repo_id"], info["revision"], filter_spec)
            clean_name = info.get("single_file") or info["repo_id"].replace("/", "__")

            quality_options = [
                {
                    "id": "repo_all",
                    "label": f"Complete Repository ({res['file_count']} Files/Shards • {res['total_bytes']/(1024**2):.1f} MB)",
                    "type": "ai_repo",
                }
            ]
            # Add individual GGUF/Safetensors file options if model repo has multiple weight files
            gguf_files = [i for i in res["items"] if i["rfilename"].endswith((".gguf", ".safetensors"))]
            for gf in gguf_files[:10]:
                quality_options.append(
                    {
                        "id": f"file:{gf['rfilename']}",
                        "label": f"Single Weight: {gf['rfilename']} ({gf['size']/(1024**2):.1f} MB)",
                        "type": "ai_file",
                    }
                )

            default_qid = f"file:{info['single_file']}" if info.get("single_file") else "repo_all"
            return {
                "is_valid": True,
                "is_ai_workload": True,
                "ai_kind": kind,
                "platform_name": "Hugging Face",
                "content_type_label": (
                    f"Hugging Face Dataset ({res['file_count']} Shards/Files)"
                    if kind == "hf_dataset"
                    else f"Hugging Face Model ({res['file_count']} Files)"
                ),
                "title": f"Hugging Face: {info['display_id']}",
                "suggested_filename": clean_name,
                "expected_size_bytes": res["total_bytes"],
                "file_count": res["file_count"],
                "quality_options": quality_options,
                "default_quality_id": default_qid,
                "files_inventory": res["items"][:50],
                "auth_note": "LFS SHA-256 digests and exact shard sizes resolved via Hugging Face Tree API.",
                "raw_url": raw_input,
                "url": raw_input,
            }

        elif kind == "kaggle":
            res = resolve_kaggle_dataset(info["owner"], info["slug"], info["version"])
            quality_options = [
                {
                    "id": "kaggle_zip_and_extract",
                    "label": f"Verified ZIP Archive + Auto-Extract ({res['total_bytes']/(1024**2):.1f} MB)",
                    "type": "ai_dataset",
                },
                {
                    "id": "kaggle_zip_only",
                    "label": f"Verified ZIP Archive Only ({res['total_bytes']/(1024**2):.1f} MB)",
                    "type": "ai_dataset",
                },
            ]
            return {
                "is_valid": True,
                "is_ai_workload": True,
                "ai_kind": "kaggle",
                "platform_name": "Kaggle Datasets",
                "content_type_label": "Kaggle Dataset Archive (Signed GCS Stream)",
                "title": f"Kaggle Dataset: {info['display_id']}",
                "suggested_filename": res["archive_filename"],
                "expected_size_bytes": res["total_bytes"],
                "file_count": 1,
                "quality_options": quality_options,
                "default_quality_id": "kaggle_zip_and_extract",
                "files_inventory": res["items"],
                "auth_note": "Signed Google Cloud Storage URL resolved with HTTP byte-range support.",
                "raw_url": raw_input,
                "url": raw_input,
            }
    except Exception as e:
        return {
            "is_valid": False,
            "is_ai_workload": True,
            "error": str(e),
            "raw_url": raw_input,
        }


def compute_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            chunk = f.read(4 * 1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def verify_artifact_usability(path: Path, expected_size: int = None, expected_sha256: str = None) -> dict:
    if not path.exists():
        raise RuntimeError(f"File does not exist on disk: {path}")

    actual_size = path.stat().st_size
    if expected_size is not None and expected_size > 0 and actual_size != expected_size:
        raise RuntimeError(f"Size mismatch for {path.name}: expected {expected_size}, got {actual_size}")

    computed_sha = compute_sha256(path)
    if expected_sha256 and computed_sha.lower() != expected_sha256.lower():
        raise RuntimeError(
            f"SHA-256 mismatch for {path.name}: expected {expected_sha256}, computed {computed_sha}"
        )

    name_lower = path.name.lower()
    format_check = "binary_verified"
    if name_lower.endswith(".gguf"):
        with open(path, "rb") as f:
            magic = f.read(4)
            if magic != b"GGUF":
                raise RuntimeError(f"Invalid GGUF magic header in {path.name}: {magic!r}")
        format_check = "gguf_header_valid"
    elif name_lower.endswith(".parquet"):
        with open(path, "rb") as f:
            head = f.read(4)
            f.seek(max(0, actual_size - 4))
            tail = f.read(4)
            if head != b"PAR1" or tail != b"PAR1":
                raise RuntimeError(f"Invalid Parquet PAR1 header/footer magic in {path.name}")
        format_check = "parquet_magic_valid"
    elif name_lower.endswith(".safetensors"):
        with open(path, "rb") as f:
            hdr_bytes = f.read(8)
            hdr_len = struct.unpack("<Q", hdr_bytes)[0]
            if hdr_len == 0 or hdr_len > 100 * 1024 * 1024 or hdr_len + 8 > actual_size:
                raise RuntimeError(f"Invalid Safetensors header length ({hdr_len}) in {path.name}")
        format_check = "safetensors_header_valid"
    elif name_lower.endswith(".zip"):
        with zipfile.ZipFile(path, "r") as zf:
            bad_member = zf.testzip()
            if bad_member is not None:
                raise RuntimeError(f"Corrupted member inside ZIP archive {path.name}: {bad_member}")
        format_check = "zip_crc32_valid"

    return {
        "file": path.name,
        "size_bytes": actual_size,
        "sha256": computed_sha,
        "format_check": format_check,
        "verified": True,
    }


def resolve_direct_signed_url(canonical_url: str) -> tuple:
    """
    Follow redirect from canonical URL (Ollama registry / HF resolve / Kaggle download API)
    using a 1-byte range request to obtain the direct CDN/GCS signed URL, total size, and ETag.
    """
    headers = {
        "User-Agent": "kagglehub/0.3.0 otter-engine/0.1.0",
        "Range": "bytes=0-0",
    }
    token = os.environ.get("HF_TOKEN") or os.environ.get("HUGGING_FACE_HUB_TOKEN")
    if token and "huggingface.co" in canonical_url:
        headers["Authorization"] = f"Bearer {token}"

    req = urllib.request.Request(canonical_url, headers=headers)
    with urllib.request.urlopen(req, timeout=25) as resp:
        direct_url = resp.geturl()
        cr = resp.headers.get("Content-Range", "")
        cl = resp.headers.get("Content-Length", "")
        etag = resp.headers.get("ETag")
        resumable = resp.status == 206 or "bytes" in resp.headers.get("Accept-Ranges", "").lower()
        if "/" in cr:
            total_size = int(cr.split("/")[-1])
        elif cl.isdigit():
            total_size = int(cl)
        else:
            total_size = 0
        return direct_url, total_size, etag, resumable


def download_segmented_file(
    item: dict,
    dest_path: Path,
    state_file: Path,
    state_lock: threading.Lock,
    job_state: dict,
    governor: AdaptiveWorkerGovernor,
    progress_cb=None,
    stop_at_fraction: float = None,
    simulate_expired_url_once: bool = False,
) -> dict:
    """
    Download a single file/layer/shard using dynamic multi-connection HTTP range segmentation
    with durable segment checkpointing, expired signed-URL refresh, and strict SHA-256 verification.
    """
    dest_path.parent.mkdir(parents=True, exist_ok=True)
    part_path = dest_path.with_name(dest_path.name + ".part")
    rfn = item["rfilename"]
    expected_size = int(item.get("size") or 0)
    expected_sha256 = item.get("sha256")
    canonical_url = item["canonical_url"]

    # Check if already completed and verified on disk
    if dest_path.exists():
        try:
            ver = verify_artifact_usability(dest_path, expected_size, expected_sha256)
            return {
                "rfilename": rfn,
                "status": "already_complete",
                "retransmitted_bytes": 0,
                "downloaded_new_bytes": 0,
                "verification": ver,
            }
        except Exception:
            dest_path.unlink(missing_ok=True)

    # Check available disk space before allocating
    free_space = shutil.disk_usage(dest_path.parent).free
    if expected_size > 0 and free_space < expected_size + 64 * 1024 * 1024:
        raise RuntimeError(
            f"Insufficient disk space on {dest_path.drive}: required {expected_size} bytes, available {free_space} bytes"
        )

    # Resolve direct CDN/GCS URL and validators
    direct_url, probed_size, remote_etag, resumable = resolve_direct_signed_url(canonical_url)
    if expected_size == 0 and probed_size > 0:
        expected_size = probed_size

    def _calc_checkpointed_bytes(segs):
        total = 0
        for s in segs:
            orig = s.get("orig_start", s["start"])
            if s.get("done"):
                total += (s["end"] - orig + 1)
            elif s["start"] > orig:
                total += (s["start"] - orig)
        return total

    # Load or initialize per-file state in job_state
    with state_lock:
        files_state = job_state.setdefault("files", {})
        fstate = files_state.get(rfn)
        # Never-Splice validator check
        if fstate:
            stored_size = fstate.get("expected_size", 0)
            stored_sha = fstate.get("expected_sha256")
            if (stored_size and expected_size and stored_size != expected_size) or (
                stored_sha and expected_sha256 and stored_sha != expected_sha256
            ):
                # Remote resource changed; discard partial to prevent splicing
                part_path.unlink(missing_ok=True)
                fstate = None

        if not fstate or not part_path.exists():
            # Create/truncate .part file to exact expected_size
            with open(part_path, "wb") as pf:
                if expected_size > 0:
                    pf.truncate(expected_size)
            # Build segment map (16 MiB for >=512 MiB, 4 MiB for >=32 MiB, 2 MiB for >=4 MiB, single-range for <4 MiB)
            if expected_size >= 512 * 1024 * 1024:
                seg_size = 16 * 1024 * 1024
            elif expected_size >= 32 * 1024 * 1024:
                seg_size = 4 * 1024 * 1024
            elif expected_size >= 4 * 1024 * 1024:
                seg_size = 2 * 1024 * 1024
            else:
                seg_size = max(1, expected_size)

            segments = []
            if expected_size > 0 and resumable:
                cursor = 0
                seg_id = 0
                while cursor < expected_size:
                    end = min(expected_size - 1, cursor + seg_size - 1)
                    segments.append({"id": seg_id, "orig_start": cursor, "start": cursor, "end": end, "done": False})
                    cursor = end + 1
                    seg_id += 1
            else:
                segments.append({"id": 0, "orig_start": 0, "start": 0, "end": max(0, expected_size - 1), "done": False})

            fstate = {
                "rfilename": rfn,
                "expected_size": expected_size,
                "expected_sha256": expected_sha256,
                "remote_etag": remote_etag,
                "bytes_received_from_network": 0,
                "bytes_written_to_part_file": 0,
                "bytes_flushed_to_disk": 0,
                "bytes_checkpointed_in_state_db": 0,
                "bytes_verified_by_integrity_check": 0,
                "url_refresh_events": [],
                "segments": segments,
            }
            files_state[rfn] = fstate
            _save_state_atomic(state_file, job_state)
        else:
            # Corrupt partial file size check
            actual_part_size = part_path.stat().st_size
            if expected_size > 0 and actual_part_size != expected_size:
                # Repair/truncate without losing valid in-bounds segments, or reset out-of-bounds segments
                with open(part_path, "r+b") as pf:
                    for seg in fstate.get("segments", []):
                        if seg["end"] >= actual_part_size:
                            seg["done"] = False
                            seg["start"] = seg.get("orig_start", seg["start"])
                    pf.truncate(expected_size)
                checkpointed = _calc_checkpointed_bytes(fstate.get("segments", []))
                fstate["bytes_checkpointed_in_state_db"] = checkpointed
                fstate["bytes_flushed_to_disk"] = checkpointed
                _save_state_atomic(state_file, job_state)

    pending_segments = [s for s in fstate["segments"] if not s.get("done")]
    already_checkpointed_bytes = _calc_checkpointed_bytes(fstate["segments"])

    url_holder = {"url": direct_url, "expired_simulated": False}
    if simulate_expired_url_once:
        # Inject an expired signature parameter on initial URL to force a real 403 -> refresh cycle
        sep = "&" if "?" in direct_url else "?"
        url_holder["url"] = direct_url + f"{sep}X-Goog-Signature=deadbeef0000"

    file_io_lock = threading.Lock()
    new_bytes_downloaded = 0
    interrupted_flag = threading.Event()

    def save_checkpoint():
        with state_lock:
            done_bytes = _calc_checkpointed_bytes(fstate["segments"])
            fstate["bytes_flushed_to_disk"] = done_bytes
            fstate["bytes_checkpointed_in_state_db"] = done_bytes
            _save_state_atomic(state_file, job_state)

    part_fd = open(part_path, "r+b")

    def download_one_segment(seg: dict):
        nonlocal new_bytes_downloaded
        if interrupted_flag.is_set():
            return False

        for attempt in range(1, 6):
            if interrupted_flag.is_set():
                return False
            start_b = seg["start"]
            end_b = seg["end"]
            if start_b > end_b:
                seg["done"] = True
                return True
            expected_len = end_b - start_b + 1

            cur_url = url_holder["url"]
            req = urllib.request.Request(
                cur_url,
                headers={
                    "User-Agent": "otter-engine/0.1.0",
                    "Range": f"bytes={start_b}-{end_b}",
                },
            )
            t_req = time.time()
            try:
                with urllib.request.urlopen(req, timeout=30) as resp:
                    rtt_ms = (time.time() - t_req) * 1000.0
                    governor.record_http_event(rtt_ms=rtt_ms)
                    buf = bytearray()
                    stopped_early = False
                    while True:
                        if interrupted_flag.is_set():
                            stopped_early = True
                            break
                        chunk = resp.read(256 * 1024)
                        if not chunk:
                            break
                        buf.extend(chunk)
                        with state_lock:
                            fstate["bytes_received_from_network"] += len(chunk)
                        if progress_cb:
                            progress_cb(len(chunk))
                        if (
                            stop_at_fraction is not None
                            and expected_size > 0
                            and ((already_checkpointed_bytes + new_bytes_downloaded + len(buf)) / expected_size)
                            >= stop_at_fraction
                        ):
                            interrupted_flag.set()
                            stopped_early = True
                            break

                    if stopped_early:
                        # Flush and checkpoint any partial bytes received in buf so 0 bytes are wasted on resume!
                        if len(buf) > 0:
                            with file_io_lock:
                                part_fd.seek(start_b)
                                part_fd.write(buf)
                                part_fd.flush()
                            with state_lock:
                                seg["start"] = start_b + len(buf)
                                if seg["start"] > end_b:
                                    seg["done"] = True
                                fstate["bytes_written_to_part_file"] += len(buf)
                                new_bytes_downloaded += len(buf)
                            save_checkpoint()
                        return True

                    if len(buf) != expected_len and expected_size > 0:
                        raise RuntimeError(
                            f"Short segment read ({len(buf)} != {expected_len}) for {rfn} [{start_b}-{end_b}]"
                        )

                    t_w0 = time.time()
                    with file_io_lock:
                        part_fd.seek(start_b)
                        part_fd.write(buf)
                        part_fd.flush()
                    write_ms = (time.time() - t_w0) * 1000.0
                    governor.record_disk_write(write_ms)

                    with state_lock:
                        seg["start"] = end_b + 1
                        seg["done"] = True
                        fstate["bytes_written_to_part_file"] += len(buf)
                        new_bytes_downloaded += len(buf)

                    save_checkpoint()
                    return True

            except urllib.error.HTTPError as he:
                if he.code == 429:
                    governor.record_http_event(rate_limited=True)
                    time.sleep(min(8.0, 1.5 * attempt))
                elif he.code in (400, 401, 403, 410):
                    # Signed URL expired or rejected -> re-resolve canonical URL and verify identity
                    with state_lock:
                        fresh_url, fresh_size, fresh_etag, _ = resolve_direct_signed_url(canonical_url)
                        if expected_size > 0 and fresh_size > 0 and fresh_size != expected_size:
                            raise RuntimeError(
                                f"Refreshed URL size mismatch ({fresh_size} != {expected_size}) — aborting to prevent splicing!"
                            )
                        url_holder["url"] = fresh_url
                        fstate["url_refresh_events"].append(
                            {
                                "timestamp": time.time(),
                                "http_code": he.code,
                                "segment_id": seg["id"],
                                "verified_size": fresh_size,
                                "verified_etag": fresh_etag,
                            }
                        )
                else:
                    governor.record_http_event(error=True)
                    time.sleep(min(5.0, 0.8 * attempt))
            except Exception:
                governor.record_http_event(error=True)
                time.sleep(min(5.0, 0.8 * attempt))

        raise RuntimeError(f"Segment {seg['id']} [{start_b}-{end_b}] failed after max retries")

    try:
        idx = 0
        total_pending = len(pending_segments)
        while idx < total_pending and not interrupted_flag.is_set():
            rem_bytes = max(0, expected_size - (already_checkpointed_bytes + new_bytes_downloaded))
            target_workers = governor.evaluate(
                already_checkpointed_bytes + new_bytes_downloaded, rem_bytes
            )
            batch_size = min(total_pending - idx, max(1, target_workers))
            batch = pending_segments[idx : idx + batch_size]

            with concurrent.futures.ThreadPoolExecutor(max_workers=target_workers) as pool:
                futures = [pool.submit(download_one_segment, seg) for seg in batch]
                for fut in concurrent.futures.as_completed(futures):
                    fut.result()
                    if interrupted_flag.is_set():
                        break
            with file_io_lock:
                part_fd.flush()
                os.fsync(part_fd.fileno())
            save_checkpoint()
            idx += batch_size
    finally:
        part_fd.flush()
        os.fsync(part_fd.fileno())
        part_fd.close()
        save_checkpoint()

    if interrupted_flag.is_set():
        return {
            "rfilename": rfn,
            "status": "interrupted_checkpoint_saved",
            "already_checkpointed_bytes": already_checkpointed_bytes,
            "downloaded_new_bytes": new_bytes_downloaded,
            "bytes_received_from_network": fstate["bytes_received_from_network"],
            "bytes_written_to_part_file": fstate["bytes_written_to_part_file"],
            "bytes_flushed_to_disk": fstate["bytes_flushed_to_disk"],
            "bytes_checkpointed_in_state_db": fstate["bytes_checkpointed_in_state_db"],
            "bytes_verified_by_integrity_check": fstate["bytes_verified_by_integrity_check"],
            "url_refresh_events": fstate["url_refresh_events"],
        }

    # Verify all segments are marked done
    if not all(s.get("done") for s in fstate["segments"]):
        raise RuntimeError(f"Incomplete segments remain for {rfn}")

    # Atomic rename .part -> final destination and full integrity verification
    if dest_path.exists():
        dest_path.unlink()
    os.replace(part_path, dest_path)

    ver = verify_artifact_usability(dest_path, expected_size, expected_sha256)
    with state_lock:
        fstate["bytes_verified_by_integrity_check"] = ver["size_bytes"]
        fstate["completed"] = True
        fstate["sha256_verified"] = ver["sha256"]
        _save_state_atomic(state_file, job_state)

    return {
        "rfilename": rfn,
        "status": "completed",
        "already_checkpointed_bytes": already_checkpointed_bytes,
        "downloaded_new_bytes": new_bytes_downloaded,
        "retransmitted_bytes": max(0, fstate["bytes_received_from_network"] - expected_size)
        if already_checkpointed_bytes == 0
        else max(0, fstate["bytes_received_from_network"] - fstate["bytes_written_to_part_file"]),
        "bytes_received_from_network": fstate["bytes_received_from_network"],
        "bytes_written_to_part_file": fstate["bytes_written_to_part_file"],
        "bytes_flushed_to_disk": fstate["bytes_flushed_to_disk"],
        "bytes_checkpointed_in_state_db": fstate["bytes_checkpointed_in_state_db"],
        "bytes_verified_by_integrity_check": fstate["bytes_verified_by_integrity_check"],
        "url_refresh_events": fstate["url_refresh_events"],
        "verification": ver,
    }


def _save_state_atomic(state_file: Path, data: dict):
    state_file.parent.mkdir(parents=True, exist_ok=True)
    tmp = state_file.with_name(state_file.name + ".tmp")
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2)
        f.flush()
        os.fsync(f.fileno())
    os.replace(tmp, state_file)


def execute_ai_download(
    raw_input: str,
    variant_id: str,
    target_dir: str,
    resolved_name: str,
    initial_workers: int = 16,
    job_id: str = "ai_job",
    stop_at_fraction: float = None,
    simulate_expired_url_once: bool = False,
) -> dict:
    t0 = time.time()
    out_root = Path(target_dir)
    out_root.mkdir(parents=True, exist_ok=True)

    info = classify_ai_input(raw_input)
    kind = info.get("kind")
    if kind == "unknown":
        raise RuntimeError(f"Unsupported AI resource input: {raw_input}")

    if kind == "ollama":
        meta = resolve_ollama_manifest(info["namespace"], info["model_name"], info["tag"])
        items = meta["items"]
        total_expected = meta["total_bytes"]
        job_dir = out_root / resolved_name if resolved_name and not resolved_name.endswith(".gguf") else out_root
    elif kind in ("hf_model", "hf_dataset"):
        filter_spec = variant_id if variant_id and variant_id != "repo_all" else (
            f"file:{info['single_file']}" if info.get("single_file") else None
        )
        meta = resolve_hf_repository(kind, info["repo_id"], info["revision"], filter_spec)
        items = meta["items"]
        total_expected = meta["total_bytes"]
        if len(items) == 1 and info.get("single_file"):
            job_dir = out_root
        else:
            sub_name = resolved_name or info["repo_id"].replace("/", "__")
            job_dir = out_root / sub_name
    elif kind == "kaggle":
        meta = resolve_kaggle_dataset(info["owner"], info["slug"], info["version"])
        items = meta["items"]
        total_expected = meta["total_bytes"]
        job_dir = out_root
    else:
        raise RuntimeError(f"Unhandled AI workload kind: {kind}")

    job_dir.mkdir(parents=True, exist_ok=True)
    state_file = job_dir / f".otter_ai_state_{job_id}.json"
    state_lock = threading.Lock()

    if state_file.exists():
        try:
            job_state = json.loads(state_file.read_text(encoding="utf-8"))
        except Exception:
            job_state = {"job_id": job_id, "raw_input": raw_input, "files": {}}
    else:
        job_state = {"job_id": job_id, "raw_input": raw_input, "files": {}}

    governor = AdaptiveWorkerGovernor(initial_workers=max(4, min(64, initial_workers or 16)))

    # Calculate already-checkpointed bytes across all items
    already_done_global = 0
    for it in items:
        rfn = it["rfilename"]
        fp = job_dir / rfn
        if fp.exists() and fp.stat().st_size == it.get("size", 0):
            already_done_global += it.get("size", 0)
        elif rfn in job_state.get("files", {}):
            fst = job_state["files"][rfn]
            already_done_global += int(fst.get("bytes_checkpointed_in_state_db", 0))

    downloaded_global = already_done_global
    last_emit = time.time()
    speed_window_bytes = 0
    speed_window_start = time.time()
    peak_speed_bps = 0.0
    progress_lock = threading.Lock()

    def on_progress(chunk_len: int):
        nonlocal downloaded_global, last_emit, speed_window_bytes, speed_window_start, peak_speed_bps
        with progress_lock:
            downloaded_global += chunk_len
            speed_window_bytes += chunk_len
            now = time.time()
            dt = now - last_emit
            if dt >= 0.35:
                win_dt = max(0.001, now - speed_window_start)
                speed_bps = speed_window_bytes / win_dt
                if speed_bps > peak_speed_bps:
                    peak_speed_bps = speed_bps
                rem = max(0, total_expected - downloaded_global)
                eta = int(rem / speed_bps) if speed_bps > 0 else None
                cur_workers = governor.evaluate(downloaded_global, rem)
                msg = {
                    "type": "progress",
                    "downloaded_bytes": downloaded_global,
                    "total_bytes": total_expected,
                    "speed_bps": round(speed_bps, 2),
                    "eta_seconds": eta,
                    "streams": cur_workers,
                }
                print(json.dumps(msg), flush=True)
                last_emit = now
                if win_dt >= 1.5:
                    speed_window_bytes = 0
                    speed_window_start = now

    file_results = []
    interrupted_any = False

    if len(items) > 1 and stop_at_fraction is None and not simulate_expired_url_once:
        small_items = [it for it in items if int(it.get("size") or 0) < 32 * 1024 * 1024]
        large_items = [it for it in items if int(it.get("size") or 0) >= 32 * 1024 * 1024]

        def _dl_item(it):
            return download_segmented_file(
                item=it,
                dest_path=job_dir / it["rfilename"],
                state_file=state_file,
                state_lock=state_lock,
                job_state=job_state,
                governor=governor,
                progress_cb=on_progress,
                stop_at_fraction=None,
                simulate_expired_url_once=False,
            )

        if small_items:
            with concurrent.futures.ThreadPoolExecutor(max_workers=min(6, len(small_items))) as fpool:
                for res in fpool.map(_dl_item, small_items):
                    file_results.append(res)

        if large_items:
            with concurrent.futures.ThreadPoolExecutor(max_workers=min(2, len(large_items))) as lpool:
                for res in lpool.map(_dl_item, large_items):
                    file_results.append(res)
    else:
        for idx, item in enumerate(items):
            dest_file = job_dir / item["rfilename"]
            res = download_segmented_file(
                item=item,
                dest_path=dest_file,
                state_file=state_file,
                state_lock=state_lock,
                job_state=job_state,
                governor=governor,
                progress_cb=on_progress,
                stop_at_fraction=stop_at_fraction,
                simulate_expired_url_once=(simulate_expired_url_once and idx == 0),
            )
            file_results.append(res)
            if res["status"] == "interrupted_checkpoint_saved":
                interrupted_any = True
                break

    elapsed_sec = round(time.time() - t0, 3)

    if interrupted_any:
        summary = {
            "type": "interrupted",
            "job_id": job_id,
            "kind": kind,
            "elapsed_sec": elapsed_sec,
            "downloaded_bytes": downloaded_global,
            "total_bytes": total_expected,
            "worker_history": governor.history,
            "file_results": file_results,
        }
        print(json.dumps(summary), flush=True)
        return summary

    # Post-download registration / extraction / dataset completeness checks
    extra_info = {}
    if kind == "ollama":
        manifest_path = job_dir / meta["manifest_rel_path"]
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        manifest_path.write_text(meta["raw_manifest_json"], encoding="utf-8")
        extra_info["ollama_manifest_path"] = str(manifest_path)
        extra_info["unique_blob_count"] = meta["unique_blob_count"]
        extra_info["deduplicated_layers_skipped"] = meta["deduplicated_layers_skipped"]
    elif kind == "kaggle" and variant_id == "kaggle_zip_and_extract":
        zip_path = job_dir / meta["archive_filename"]
        extract_dir = job_dir / f"{meta['slug']}_extracted"
        t_ext0 = time.time()
        extract_dir.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(zip_path, "r") as zf:
            zf.extractall(extract_dir)
            extracted_files = zf.namelist()
        extra_info["archive_download_completed"] = True
        extra_info["extraction_completed"] = True
        extra_info["extraction_time_sec"] = round(time.time() - t_ext0, 3)
        extra_info["extracted_file_count"] = len(extracted_files)
        extra_info["extracted_dir"] = str(extract_dir)

    # Clean up state file only after 100% of items are verified
    state_file.unlink(missing_ok=True)

    avg_speed_bps = total_expected / max(0.001, elapsed_sec)
    final_path_str = str(job_dir / items[0]["rfilename"]) if len(items) == 1 else str(job_dir)

    completed_msg = {
        "type": "completed",
        "job_id": job_id,
        "kind": kind,
        "file_path": final_path_str,
        "total_bytes": total_expected,
        "elapsed_sec": elapsed_sec,
        "avg_speed_mbps": round(avg_speed_bps / (1024 * 1024), 2),
        "peak_speed_mbps": round(peak_speed_bps / (1024 * 1024), 2),
        "file_count": len(file_results),
        "worker_history": governor.history,
        "file_results": file_results,
        "extra": extra_info,
    }
    print(json.dumps(completed_msg), flush=True)
    return completed_msg


def main():
    if len(sys.argv) < 3:
        print(json.dumps({"error": "Usage: ai_engine.py [probe|download] <url_or_cmd> ..."}))
        sys.exit(1)

    cmd = sys.argv[1]
    raw_input = sys.argv[2]

    if cmd == "probe":
        dest_dir = sys.argv[3] if len(sys.argv) > 3 else None
        res = probe_ai_resource(raw_input, dest_dir)
        print(json.dumps(res), flush=True)
    elif cmd == "download":
        variant_id = sys.argv[3] if len(sys.argv) > 3 else "default"
        target_dir = sys.argv[4] if len(sys.argv) > 4 else "."
        resolved_name = sys.argv[5] if len(sys.argv) > 5 else "ai_download"
        initial_workers = int(sys.argv[6]) if len(sys.argv) > 6 and sys.argv[6].isdigit() else 16
        job_id = sys.argv[7] if len(sys.argv) > 7 else "ai_job"
        stop_pct = float(sys.argv[8]) if len(sys.argv) > 8 and sys.argv[8] != "none" else None
        sim_exp = (sys.argv[9].lower() == "true") if len(sys.argv) > 9 else False
        try:
            execute_ai_download(
                raw_input=raw_input,
                variant_id=variant_id,
                target_dir=target_dir,
                resolved_name=resolved_name,
                initial_workers=initial_workers,
                job_id=job_id,
                stop_at_fraction=stop_pct,
                simulate_expired_url_once=sim_exp,
            )
        except Exception as e:
            print(json.dumps({"type": "error", "error": str(e)}), flush=True)
            sys.exit(1)


if __name__ == "__main__":
    main()
