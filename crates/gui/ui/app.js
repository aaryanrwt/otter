// Otter GUI Client Application Logic

let currentState = {
  jobs: [],
  active_jobs: [],
  completed_jobs: [],
  metrics: {
    active_count: 0,
    completed_count: 0,
    total_speed_str: "0 MB/s",
    total_downloaded_str: "0 GB"
  },
  system_specs: {
    device_name: "Desktop PC",
    cpu_cores: 8,
    total_ram_gb: 16.0,
    free_disk_space_gb: 50.0,
    disk_path: "D:\\Downloads\\Otter"
  },
  engine_stats: {
    aimd_status: "Active (Additive Increase)",
    active_connections: 0,
    max_connections: 16,
    wal_status: "SQLite WAL (Normal Sync)",
    disk_mode: "Win32 FileAllocationInfo"
  },
  settings: {
    default_download_dir: "D:\\Downloads\\Otter",
    concurrent_downloads: 5,
    bandwidth_limit_mb_s: null,
    start_with_windows: false,
    minimize_to_tray: true,
    live_detection: true,
    theme: "light",
    language: "English"
  }
};

let currentDetectedResult = null;
let currentFilter = "all";
let currentLibFilter = "all";

// IPC helper to communicate with the Rust backend
function sendIpc(cmd, data = {}) {
  const payload = Object.assign({ cmd }, data);
  if (window.ipc && window.ipc.postMessage) {
    window.ipc.postMessage(JSON.stringify(payload));
  } else {
    console.log("[IPC Mock Outbound]", payload);
  }
}

// Inbound event dispatcher called from Rust via evaluate_script
window.onOtterEvent = function(event) {
  if (!event || !event.type) return;

  switch (event.type) {
    case "state_update":
      currentState = Object.assign(currentState, event.data);
      renderAll();
      break;

    case "detection_result":
      renderDetectionResult(event.data);
      break;

    case "doctor_result":
      renderDoctorResults(event.data);
      break;

    case "notification":
      showToast(event.message);
      break;

    default:
      console.log("[Inbound Otter Event]", event);
  }
};

// DOM Content Loaded Handler
document.addEventListener("DOMContentLoaded", () => {
  setupNavigation();
  setupWindowControls();
  setupModal();
  setupFilters();
  setupSettings();
  setupEngine();

  // Initial state poll
  sendIpc("get_state");

  // Periodic state refresh every 1.5 seconds
  setInterval(() => {
    sendIpc("get_state");
  }, 1500);
});

// Setup Main Sidebar Navigation
function setupNavigation() {
  const navItems = document.querySelectorAll(".nav-item");
  navItems.forEach(btn => {
    btn.addEventListener("click", () => {
      const targetViewId = btn.getAttribute("data-view");
      switchView(targetViewId);
    });
  });

  const viewAllBtn = document.getElementById("btn-view-all-downloads");
  if (viewAllBtn) {
    viewAllBtn.addEventListener("click", () => {
      switchView("view-downloads");
    });
  }
}

function switchView(viewId) {
  document.querySelectorAll(".nav-item").forEach(item => {
    if (item.getAttribute("data-view") === viewId) {
      item.classList.add("active");
    } else {
      item.classList.remove("active");
    }
  });

  document.querySelectorAll(".view").forEach(v => {
    if (v.id === viewId) {
      v.classList.add("active");
    } else {
      v.classList.remove("active");
    }
  });
}

// Setup Windows Window Controls
function setupWindowControls() {
  document.getElementById("btn-minimize")?.addEventListener("click", () => {
    sendIpc("window_minimize");
  });

  document.getElementById("btn-maximize")?.addEventListener("click", () => {
    sendIpc("window_maximize");
  });

  document.getElementById("btn-close")?.addEventListener("click", () => {
    sendIpc("window_close");
  });
}

// Setup Filters
function setupFilters() {
  document.querySelectorAll(".filter-pills-row .pill[data-filter]").forEach(pill => {
    pill.addEventListener("click", () => {
      document.querySelectorAll(".filter-pills-row .pill[data-filter]").forEach(p => p.classList.remove("active"));
      pill.classList.add("active");
      currentFilter = pill.getAttribute("data-filter");
      renderDownloads();
    });
  });

  document.querySelectorAll(".filter-pills-row .pill[data-lib-cat]").forEach(pill => {
    pill.addEventListener("click", () => {
      document.querySelectorAll(".filter-pills-row .pill[data-lib-cat]").forEach(p => p.classList.remove("active"));
      pill.classList.add("active");
      currentLibFilter = pill.getAttribute("data-lib-cat");
      renderLibrary();
    });
  });

  const dlSearch = document.getElementById("downloads-search");
  if (dlSearch) {
    dlSearch.addEventListener("input", () => renderDownloads());
  }

  const libSearch = document.getElementById("library-search");
  if (libSearch) {
    libSearch.addEventListener("input", () => renderLibrary());
  }
}

// Setup Universal URL Modal
function setupModal() {
  const modal = document.getElementById("add-modal");
  const openBtn = document.getElementById("btn-open-add-modal");
  const closeBtn = document.getElementById("btn-close-modal");
  const cancelBtn = document.getElementById("btn-modal-cancel");
  const detectBtn = document.getElementById("btn-detect-url");
  const urlInput = document.getElementById("modal-url-input");
  const startBtn = document.getElementById("btn-start-download");

  openBtn?.addEventListener("click", () => {
    modal.style.display = "flex";
    urlInput.value = "";
    document.getElementById("detection-preview").style.display = "none";
    document.getElementById("detection-error").style.display = "none";
    urlInput.focus();
  });

  const closeModal = () => {
    modal.style.display = "none";
    currentDetectedResult = null;
  };

  closeBtn?.addEventListener("click", closeModal);
  cancelBtn?.addEventListener("click", closeModal);

  detectBtn?.addEventListener("click", () => {
    triggerDetection();
  });

  urlInput?.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      triggerDetection();
    }
  });

  startBtn?.addEventListener("click", () => {
    if (!currentDetectedResult) return;
    const variantSelect = document.getElementById("prev-variant-select");
    const chosenUrl = variantSelect && variantSelect.value ? variantSelect.value : currentDetectedResult.raw_url;
    
    sendIpc("start_download", {
      url: chosenUrl,
      destination: currentDetectedResult.suggested_folder,
      filename: currentDetectedResult.suggested_filename
    });

    closeModal();
    switchView("view-downloads");
  });
}

function triggerDetection() {
  const urlInput = document.getElementById("modal-url-input");
  const url = urlInput.value.trim();
  if (!url) return;

  document.getElementById("detection-error").style.display = "none";
  document.getElementById("detection-preview").style.display = "none";

  sendIpc("detect_url", { url });
}

function renderDetectionResult(res) {
  if (!res) return;
  currentDetectedResult = res;

  if (!res.is_valid) {
    const errBox = document.getElementById("detection-error");
    const errTxt = document.getElementById("detection-error-text");
    errTxt.textContent = res.error_message || "URL validation or security check failed.";
    errBox.style.display = "flex";
    return;
  }

  const prevBox = document.getElementById("detection-preview");
  document.getElementById("prev-platform").textContent = res.platform_name;
  document.getElementById("prev-type").textContent = res.content_type_label;
  document.getElementById("prev-title").textContent = res.title;
  document.getElementById("prev-filename").textContent = res.suggested_filename;
  document.getElementById("prev-folder").textContent = res.suggested_folder;

  // Variants dropdown
  const varWrap = document.getElementById("prev-variants-wrap");
  const varSelect = document.getElementById("prev-variant-select");
  varSelect.innerHTML = "";
  if (res.variants && res.variants.length > 0) {
    res.variants.forEach((v, idx) => {
      const opt = document.createElement("option");
      opt.value = v.download_url;
      opt.textContent = `${v.label} [${v.format.toUpperCase()}]`;
      varSelect.appendChild(opt);
    });
    varWrap.style.display = "block";
  } else {
    varWrap.style.display = "none";
  }

  // Auth notice
  const authNotice = document.getElementById("prev-auth-notice");
  if (res.auth_note) {
    document.getElementById("prev-auth-text").textContent = res.auth_note;
    authNotice.style.display = "flex";
  } else {
    authNotice.style.display = "none";
  }

  prevBox.style.display = "flex";
}

// Setup Settings Page
function setupSettings() {
  const subnavBtns = document.querySelectorAll(".subnav-btn");
  subnavBtns.forEach(btn => {
    btn.addEventListener("click", () => {
      subnavBtns.forEach(b => b.classList.remove("active"));
      btn.classList.add("active");
      const targetPane = btn.getAttribute("data-settings-pane");
      document.querySelectorAll(".settings-pane").forEach(p => {
        if (p.id === targetPane) {
          p.classList.add("active");
        } else {
          p.classList.remove("active");
        }
      });
    });
  });

  // Settings inputs
  const startWin = document.getElementById("set-start-with-windows");
  const minTray = document.getElementById("set-minimize-to-tray");
  const concurrent = document.getElementById("set-concurrent-downloads");
  const bandwidth = document.getElementById("set-bandwidth-limit");
  const downloadDir = document.getElementById("set-download-dir");

  const saveCurrentSettings = () => {
    currentState.settings.start_with_windows = startWin.checked;
    currentState.settings.minimize_to_tray = minTray.checked;
    currentState.settings.concurrent_downloads = parseInt(concurrent.value) || 5;
    currentState.settings.bandwidth_limit_mb_s = bandwidth.value === "none" ? null : parseFloat(bandwidth.value);
    sendIpc("save_settings", { settings: currentState.settings });
  };

  startWin?.addEventListener("change", saveCurrentSettings);
  minTray?.addEventListener("change", saveCurrentSettings);
  concurrent?.addEventListener("change", saveCurrentSettings);
  bandwidth?.addEventListener("change", saveCurrentSettings);

  document.getElementById("btn-change-download-dir")?.addEventListener("click", () => {
    sendIpc("choose_directory");
  });
}

// Setup Engine Page
function setupEngine() {
  document.getElementById("btn-run-doctor")?.addEventListener("click", () => {
    sendIpc("run_doctor");
  });
}

function renderDoctorResults(report) {
  const container = document.getElementById("doctor-results");
  const itemsList = document.getElementById("doctor-items-container");
  if (!report || !container || !itemsList) return;

  itemsList.innerHTML = "";
  if (report.checks && report.checks.length > 0) {
    report.checks.forEach(c => {
      const row = document.createElement("div");
      row.className = "prev-item";
      row.style.padding = "6px 0";
      row.style.borderBottom = "1px solid #F0EAE6";
      row.innerHTML = `<span class="prev-k">${c.name}:</span> <span class="prev-v" style="color: ${c.status === 'Ok' ? '#10B981' : '#EF4444'}">${c.status} - ${c.detail}</span>`;
      itemsList.appendChild(row);
    });
  }
  container.style.display = "block";
}

// Master Render Loop
function renderAll() {
  renderMetrics();
  renderOverviewActive();
  renderDownloads();
  renderLibrary();
  renderEngine();
  renderSettingsInputs();
}

function renderMetrics() {
  const activeCount = currentState.metrics.active_count || (currentState.active_jobs ? currentState.active_jobs.length : 0);
  const completedCount = currentState.metrics.completed_count || 0;
  
  document.getElementById("stat-active-count").textContent = activeCount;
  document.getElementById("stat-completed-count").textContent = completedCount;
  document.getElementById("stat-total-speed").textContent = currentState.metrics.total_speed_str || "0 MB/s";
  document.getElementById("stat-downloaded-size").textContent = currentState.metrics.total_downloaded_str || "0 GB";

  // Sidebar badge
  const badge = document.getElementById("nav-badge");
  if (badge) {
    badge.textContent = activeCount;
    badge.style.display = activeCount > 0 ? "inline-block" : "none";
  }
}

function renderOverviewActive() {
  const container = document.getElementById("overview-active-list");
  const emptyState = document.getElementById("overview-empty-state");
  if (!container) return;

  const activeJobs = currentState.active_jobs || [];
  if (activeJobs.length === 0) {
    if (emptyState) emptyState.style.display = "block";
    container.innerHTML = "";
    if (emptyState) container.appendChild(emptyState);
    return;
  }

  if (emptyState) emptyState.style.display = "none";
  container.innerHTML = "";

  activeJobs.slice(0, 3).forEach(job => {
    container.appendChild(createDownloadRow(job, false));
  });
}

function renderDownloads() {
  const container = document.getElementById("downloads-list");
  if (!container) return;

  const searchInput = document.getElementById("downloads-search");
  const query = searchInput ? searchInput.value.toLowerCase().trim() : "";

  let list = currentState.jobs || [];

  // Filter chips
  if (currentFilter === "active") {
    list = list.filter(j => j.status === "Active");
  } else if (currentFilter === "paused") {
    list = list.filter(j => j.status === "Paused");
  } else if (currentFilter === "completed") {
    list = list.filter(j => j.status === "Completed");
  } else if (currentFilter === "error") {
    list = list.filter(j => j.status === "Error");
  }

  // Search filter
  if (query) {
    list = list.filter(j => j.filename.toLowerCase().includes(query) || (j.source_url && j.source_url.toLowerCase().includes(query)));
  }

  container.innerHTML = "";
  if (list.length === 0) {
    container.innerHTML = `<div class="empty-state-card"><p>No downloads match the current filter.</p></div>`;
    return;
  }

  list.forEach(job => {
    container.appendChild(createDownloadRow(job, true));
  });
}

function renderLibrary() {
  const container = document.getElementById("library-items-list");
  if (!container) return;

  const searchInput = document.getElementById("library-search");
  const query = searchInput ? searchInput.value.toLowerCase().trim() : "";

  let completed = (currentState.jobs || []).filter(j => j.status === "Completed");

  if (currentLibFilter === "models") {
    completed = completed.filter(j => j.filename.endsWith(".gguf") || j.filename.endsWith(".safetensors") || j.filename.endsWith(".bin"));
  } else if (currentLibFilter === "datasets") {
    completed = completed.filter(j => j.filename.endsWith(".parquet") || j.filename.endsWith(".arrow") || j.filename.endsWith(".jsonl"));
  } else if (currentLibFilter === "videos") {
    completed = completed.filter(j => j.filename.endsWith(".mp4") || j.filename.endsWith(".mkv") || j.filename.endsWith(".mov"));
  } else if (currentLibFilter === "archives") {
    completed = completed.filter(j => j.filename.endsWith(".zip") || j.filename.endsWith(".tar.gz") || j.filename.endsWith(".7z"));
  } else if (currentLibFilter === "documents") {
    completed = completed.filter(j => j.filename.endsWith(".pdf") || j.filename.endsWith(".docx") || j.filename.endsWith(".txt"));
  }

  if (query) {
    completed = completed.filter(j => j.filename.toLowerCase().includes(query));
  }

  container.innerHTML = "";
  if (completed.length === 0) {
    container.innerHTML = `<div class="empty-state-card"><p>Your completed library is currently empty.</p></div>`;
    return;
  }

  completed.forEach(job => {
    container.appendChild(createLibraryRow(job));
  });
}

function renderEngine() {
  document.getElementById("eng-aimd-status").textContent = currentState.engine_stats.aimd_status || "Active (AIMD)";
  document.getElementById("eng-connections").textContent = `${currentState.engine_stats.active_connections} / ${currentState.engine_stats.max_connections} Max Per Host`;
  document.getElementById("eng-wal-status").textContent = currentState.engine_stats.wal_status || "SQLite WAL";
  document.getElementById("eng-disk-mode").textContent = currentState.engine_stats.disk_mode || "Win32 FileAllocationInfo";

  document.getElementById("spec-device").textContent = currentState.system_specs.device_name || "-";
  document.getElementById("spec-cores").textContent = `${currentState.system_specs.cpu_cores} Logical Cores`;
  document.getElementById("spec-ram").textContent = `${currentState.system_specs.total_ram_gb} GB RAM`;
  document.getElementById("spec-disk").textContent = `${currentState.system_specs.free_disk_space_gb} GB free (${currentState.system_specs.disk_path})`;
}

function renderSettingsInputs() {
  const downloadDir = document.getElementById("set-download-dir");
  if (downloadDir) {
    downloadDir.value = currentState.settings.default_download_dir || "D:\\Downloads\\Otter";
  }
}

// Component Builders
function createDownloadRow(job, showAllActions = true) {
  const card = document.createElement("div");
  card.className = "download-row-card";

  const brandIcon = getBrandIcon(job.filename, job.source_url);
  const pct = Math.round(job.progress_pct || 0);
  const speedStr = job.speed_str || "0 MB/s";
  const sizeStr = job.total_size_str || formatBytes(job.total_bytes);

  card.innerHTML = `
    <div class="brand-icon">${brandIcon}</div>
    <div class="row-main-col">
      <span class="row-filename" title="${job.filename}">${job.filename}</span>
      <span class="row-meta-sub">${job.platform || 'Direct'} • ${sizeStr}</span>
    </div>
    <div class="row-progress-col">
      <div class="progress-track">
        <div class="progress-fill" style="width: ${pct}%"></div>
      </div>
      <div class="progress-info">
        <span>${pct}%</span>
        <span>${job.status === 'Active' ? speedStr : job.status}</span>
      </div>
    </div>
    <div class="row-actions-col">
      ${job.status === 'Active' 
        ? `<button class="action-icon-btn btn-pause" title="Pause">⏸</button>`
        : job.status === 'Paused' 
          ? `<button class="action-icon-btn btn-resume" title="Resume">▶</button>`
          : ''}
      <button class="action-icon-btn btn-cancel" title="Cancel & Delete">✕</button>
      <button class="action-icon-btn btn-open" title="Open containing folder">📁</button>
    </div>
  `;

  card.querySelector(".btn-pause")?.addEventListener("click", () => {
    sendIpc("pause_download", { id: job.id });
  });

  card.querySelector(".btn-resume")?.addEventListener("click", () => {
    sendIpc("resume_download", { id: job.id });
  });

  card.querySelector(".btn-cancel")?.addEventListener("click", () => {
    sendIpc("cancel_download", { id: job.id });
  });

  card.querySelector(".btn-open")?.addEventListener("click", () => {
    sendIpc("open_path", { path: job.destination_path || currentState.settings.default_download_dir });
  });

  return card;
}

function createLibraryRow(job) {
  const card = document.createElement("div");
  card.className = "download-row-card";
  const brandIcon = getBrandIcon(job.filename, job.source_url);
  const sizeStr = job.total_size_str || formatBytes(job.total_bytes);

  card.innerHTML = `
    <div class="brand-icon">${brandIcon}</div>
    <div class="row-main-col">
      <span class="row-filename" title="${job.filename}">${job.filename}</span>
      <span class="row-meta-sub">Verified SHA-256 • ${sizeStr}</span>
    </div>
    <div class="row-actions-col">
      <button class="btn-subtle btn-lib-open">Open File</button>
      <button class="btn-subtle btn-lib-folder">Open Folder</button>
      <button class="action-icon-btn btn-lib-remove" title="Remove from list">✕</button>
    </div>
  `;

  card.querySelector(".btn-lib-open")?.addEventListener("click", () => {
    sendIpc("open_file", { path: job.destination_path });
  });

  card.querySelector(".btn-lib-folder")?.addEventListener("click", () => {
    sendIpc("open_path", { path: job.destination_path });
  });

  card.querySelector(".btn-lib-remove")?.addEventListener("click", () => {
    sendIpc("remove_job", { id: job.id, delete_file: false });
  });

  return card;
}

function getBrandIcon(filename, url) {
  const fn = (filename || "").toLowerCase();
  const u = (url || "").toLowerCase();

  if (fn.endsWith(".gguf") || u.includes("ollama")) return "🤖";
  if (fn.endsWith(".safetensors") || u.includes("huggingface")) return "🤗";
  if (fn.endsWith(".parquet") || fn.endsWith(".arrow")) return "📊";
  if (u.includes("steam")) return "🎮";
  if (u.includes("epicgames")) return "⚡";
  if (u.includes("youtube") || fn.endsWith(".mp4")) return "🎬";
  if (fn.endsWith(".zip") || fn.endsWith(".tar.gz")) return "📦";
  if (fn.endsWith(".pdf")) return "📄";
  return "💾";
}

function formatBytes(bytes) {
  if (!bytes || bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

function showToast(msg) {
  console.log("[Toast]", msg);
}
