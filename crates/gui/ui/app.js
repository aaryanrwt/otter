// Otter GUI Client Application Logic

let currentState = {
  jobs: [],
  active_jobs: [],
  metrics: {
    active_count: 0,
    completed_count: 0,
    total_speed_str: "0.0 MB/s",
    total_downloaded_str: "0.0 GB"
  },
  system_specs: {
    device_name: "Desktop PC",
    cpu_cores: 8,
    total_ram_gb: 16.0,
    free_disk_space_gb: 50.0,
    disk_path: "D:\\Downloads\\Otter"
  },
  engine_stats: {
    aimd_status: "Active (AIMD)",
    connection_count: 0,
    wal_status: "WAL Mode",
    prealloc_status: "SetFileValidData"
  },
  settings: {
    user_name: "",
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
function sendIpc(action, data = {}) {
  const payload = Object.assign({ action }, data);
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
      handleDetectionResult(event.data);
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
  setupOnboarding();

  // Initial render
  renderAll();
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
  document.querySelectorAll(".filter-pills-row .pill[data-filter], .filter-pills-row .pill-btn[data-filter]").forEach(pill => {
    pill.addEventListener("click", () => {
      document.querySelectorAll(".filter-pills-row .pill[data-filter], .filter-pills-row .pill-btn[data-filter]").forEach(p => p.classList.remove("active"));
      pill.classList.add("active");
      currentFilter = pill.getAttribute("data-filter");
      renderDownloads();
    });
  });

  document.querySelectorAll(".filter-pills-row .pill[data-lib-filter], .filter-pills-row .pill-btn[data-lib-filter]").forEach(pill => {
    pill.addEventListener("click", () => {
      document.querySelectorAll(".filter-pills-row .pill[data-lib-filter], .filter-pills-row .pill-btn[data-lib-filter]").forEach(p => p.classList.remove("active"));
      pill.classList.add("active");
      currentLibFilter = pill.getAttribute("data-lib-filter");
      renderLibrary();
    });
  });

  const searchInput = document.getElementById("search-downloads-input");
  if (searchInput) {
    searchInput.addEventListener("input", () => {
      renderDownloads();
    });
  }
}

// Setup Universal URL Modal
function setupModal() {
  const modalBackdrop = document.getElementById("urlModalBackdrop");
  const openBtn = document.getElementById("btn-open-add-modal");
  const closeBtn = document.getElementById("btn-close-modal");
  const cancelBtn = document.getElementById("btn-modal-cancel");
  const startBtn = document.getElementById("btn-start-download");
  const urlInput = document.getElementById("modal-url-input");

  if (!modalBackdrop || !urlInput) return;

  function openModal() {
    modalBackdrop.style.display = "flex";
    urlInput.value = "";
    document.getElementById("detection-result-card").style.display = "none";
    startBtn.disabled = true;
    urlInput.focus();
  }

  function closeModal() {
    modalBackdrop.style.display = "none";
  }

  openBtn?.addEventListener("click", openModal);
  closeBtn?.addEventListener("click", closeModal);
  cancelBtn?.addEventListener("click", closeModal);

  modalBackdrop.addEventListener("click", (e) => {
    if (e.target === modalBackdrop) closeModal();
  });

  // Debounced input detection
  let debounceTimer = null;
  urlInput.addEventListener("input", (e) => {
    clearTimeout(debounceTimer);
    const val = e.target.value.trim();
    if (val.length < 4) {
      document.getElementById("detection-result-card").style.display = "none";
      startBtn.disabled = true;
      return;
    }

    debounceTimer = setTimeout(() => {
      sendIpc("detect_url", { url: val });
    }, 250);
  });

  startBtn?.addEventListener("click", () => {
    if (!currentDetectedResult) return;
    const finalFilename = document.getElementById("det-filename-input")?.value || currentDetectedResult.suggested_filename;
    const finalDest = document.getElementById("det-dest-input")?.value;

    sendIpc("start_download", {
      url: currentDetectedResult.raw_url,
      filename: finalFilename,
      destination: finalDest,
      connections: 8
    });

    closeModal();
    switchView("view-downloads");
  });
}

function handleDetectionResult(res) {
  currentDetectedResult = res;
  const card = document.getElementById("detection-result-card");
  const startBtn = document.getElementById("btn-start-download");
  if (!card || !res) return;

  if (!res.is_valid) {
    card.style.display = "none";
    startBtn.disabled = true;
    return;
  }

  document.getElementById("det-badge-platform").textContent = res.platform_name;
  document.getElementById("det-content-type").textContent = res.content_type_label;
  document.getElementById("det-title").textContent = res.title;
  document.getElementById("det-size").textContent = res.expected_size_bytes ? formatBytes(res.expected_size_bytes) : "Estimating...";
  document.getElementById("det-resumable").textContent = "Supported";

  const fnInput = document.getElementById("det-filename-input");
  if (fnInput) fnInput.value = res.suggested_filename;

  const destInput = document.getElementById("det-dest-input");
  if (destInput && (!destInput.value || destInput.value.trim() === "")) {
    destInput.value = currentState.settings.default_download_dir;
  }

  card.style.display = "block";
  startBtn.disabled = false;
}

// Setup Settings Tab
function setupSettings() {
  document.querySelectorAll(".subnav-btn").forEach(btn => {
    btn.addEventListener("click", () => {
      document.querySelectorAll(".subnav-btn").forEach(b => b.classList.remove("active"));
      document.querySelectorAll(".settings-pane").forEach(p => p.classList.remove("active"));

      btn.classList.add("active");
      const targetPane = btn.getAttribute("data-settings-pane");
      const paneEl = document.getElementById(targetPane);
      if (paneEl) paneEl.classList.add("active");
    });
  });

  // Theme selector
  const themeSelect = document.getElementById("set-theme");
  if (themeSelect) {
    themeSelect.addEventListener("change", (e) => {
      const theme = e.target.value;
      document.body.setAttribute("data-theme", theme);
      currentState.settings.theme = theme;
      sendIpc("save_settings", { settings: currentState.settings });
    });
  }

  // Concurrent downloads selector
  const connSelect = document.getElementById("set-concurrent-downloads");
  if (connSelect) {
    connSelect.addEventListener("change", (e) => {
      currentState.settings.concurrent_downloads = parseInt(e.target.value, 10);
      sendIpc("save_settings", { settings: currentState.settings });
    });
  }
}

// Setup User Onboarding Modal
function setupOnboarding() {
  const modal = document.getElementById("nameModalBackdrop");
  const input = document.getElementById("inputUserName");
  const btn = document.getElementById("btnSaveName");
  if (!modal || !input || !btn) return;

  function submitName() {
    const val = input.value.trim();
    const name = val || "User";
    currentState.settings.user_name = name;
    updateUserGreeting(name);
    sendIpc("save_settings", { settings: currentState.settings });
    modal.style.display = "none";
  }

  btn.addEventListener("click", submitName);
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      submitName();
    }
  });

  const saveUserNameBtn = document.getElementById("btn-save-user-name");
  const settingUserNameInput = document.getElementById("setting-user-name");
  if (saveUserNameBtn && settingUserNameInput) {
    saveUserNameBtn.addEventListener("click", () => {
      const newName = settingUserNameInput.value.trim() || "User";
      currentState.settings.user_name = newName;
      updateUserGreeting(newName);
      sendIpc("save_settings", { settings: currentState.settings });
      showToast("Name saved");
    });
  }
}

function updateUserGreeting(name) {
  const greetingEl = document.getElementById("userGreetingName");
  if (greetingEl) {
    greetingEl.textContent = name;
  }
  const settingInput = document.getElementById("setting-user-name");
  if (settingInput) {
    settingInput.value = name;
  }
}

// Setup Engine Diagnostics
function setupEngine() {
  document.getElementById("btn-run-doctor")?.addEventListener("click", () => {
    sendIpc("run_doctor");
  });
}

function renderDoctorResults(report) {
  const container = document.getElementById("doctor-results");
  const itemsList = document.getElementById("doctor-items-list");
  if (!report || !container || !itemsList) return;

  itemsList.innerHTML = "";
  if (report.checks && report.checks.length > 0) {
    report.checks.forEach(c => {
      const row = document.createElement("div");
      row.className = "prev-item";
      row.style.padding = "8px 0";
      row.style.borderBottom = "1px solid var(--border-subtle)";
      const isOk = c.status === "Pass" || c.status === "Ok";
      const statusColor = isOk ? "#10B981" : "#EF4444";
      row.innerHTML = `<span class="prev-k" style="font-weight: 600;">${c.name}:</span> <span class="prev-v" style="color: ${statusColor}; font-weight: 600;">[${c.status}] ${c.message}</span>`;
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
  
  const elActive = document.getElementById("stat-active-count");
  if (elActive) elActive.textContent = activeCount;

  const elComp = document.getElementById("stat-completed-count");
  if (elComp) elComp.textContent = completedCount;

  const elSpeed = document.getElementById("stat-total-speed");
  if (elSpeed) elSpeed.textContent = currentState.metrics.total_speed_str || "0.0 MB/s";

  const elVol = document.getElementById("stat-downloaded-vol");
  if (elVol) elVol.textContent = currentState.metrics.total_downloaded_str || "0.0 GB";

  // Sidebar badge
  const badge = document.getElementById("nav-badge");
  if (badge) {
    badge.textContent = activeCount;
    badge.style.display = activeCount > 0 ? "inline-block" : "none";
  }
}

function renderOverviewActive() {
  const container = document.getElementById("overview-active-list");
  if (!container) return;

  const activeJobs = currentState.active_jobs || [];
  if (activeJobs.length === 0) {
    container.innerHTML = `<div class="empty-state-card"><p>No active downloads in flight. Paste a URL or click Add Download to begin.</p></div>`;
    return;
  }

  container.innerHTML = "";
  activeJobs.slice(0, 3).forEach(job => {
    container.appendChild(createDownloadRow(job, false));
  });
}

function renderDownloads() {
  const container = document.getElementById("downloads-container");
  if (!container) return;

  const searchInput = document.getElementById("search-downloads-input");
  const query = searchInput ? searchInput.value.toLowerCase().trim() : "";

  let list = currentState.jobs || [];

  // Filter chips
  if (currentFilter === "active") {
    list = list.filter(j => j.status === "Active");
  } else if (currentFilter === "paused") {
    list = list.filter(j => j.status === "Paused");
  } else if (currentFilter === "completed") {
    list = list.filter(j => j.status === "Completed");
  } else if (currentFilter === "failed") {
    list = list.filter(j => j.status === "Error");
  }

  // Search query filter
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
  const container = document.getElementById("library-container");
  if (!container) return;

  let completed = (currentState.jobs || []).filter(j => j.status === "Completed");

  if (currentLibFilter === "models") {
    completed = completed.filter(j => j.filename.endsWith(".gguf") || j.filename.endsWith(".safetensors") || j.filename.endsWith(".bin") || j.filename.endsWith(".onnx"));
  } else if (currentLibFilter === "datasets") {
    completed = completed.filter(j => j.filename.endsWith(".parquet") || j.filename.endsWith(".arrow") || j.filename.endsWith(".jsonl") || j.filename.endsWith(".csv"));
  } else if (currentLibFilter === "videos") {
    completed = completed.filter(j => j.filename.endsWith(".mp4") || j.filename.endsWith(".mkv") || j.filename.endsWith(".webm") || j.filename.endsWith(".mov"));
  } else if (currentLibFilter === "archives") {
    completed = completed.filter(j => j.filename.endsWith(".zip") || j.filename.endsWith(".tar.gz") || j.filename.endsWith(".7z") || j.filename.endsWith(".rar"));
  } else if (currentLibFilter === "documents") {
    completed = completed.filter(j => j.filename.endsWith(".pdf") || j.filename.endsWith(".docx") || j.filename.endsWith(".txt") || j.filename.endsWith(".epub"));
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
  const engAimd = document.getElementById("eng-aimd-status");
  if (engAimd) engAimd.textContent = currentState.engine_stats.aimd_status || "Active (AIMD)";

  const engConn = document.getElementById("eng-connections");
  if (engConn) engConn.textContent = `${currentState.engine_stats.connection_count} Active`;

  const engWal = document.getElementById("eng-wal-status");
  if (engWal) engWal.textContent = currentState.engine_stats.wal_status || "WAL Mode";

  const engPrealloc = document.getElementById("eng-prealloc-status");
  if (engPrealloc) engPrealloc.textContent = currentState.engine_stats.prealloc_status || "SetFileValidData";

  const specDev = document.getElementById("spec-device-name");
  if (specDev) specDev.textContent = currentState.system_specs.device_name || "-";

  const specCores = document.getElementById("spec-cpu-cores");
  if (specCores) specCores.textContent = `${currentState.system_specs.cpu_cores} Logical Cores`;

  const specRam = document.getElementById("spec-ram");
  if (specRam) specRam.textContent = `${currentState.system_specs.total_ram_gb || 16.0} GB RAM`;

  const specDisk = document.getElementById("spec-disk-free");
  if (specDisk) specDisk.textContent = `${currentState.system_specs.free_disk_space_gb} GB free (${currentState.system_specs.disk_path})`;
}

function renderSettingsInputs() {
  if (currentState.settings) {
    const s = currentState.settings;

    // Show name modal on first run if name is not set
    if (!s.user_name || s.user_name.trim() === "") {
      const modal = document.getElementById("nameModalBackdrop");
      if (modal) modal.style.display = "flex";
      document.getElementById("inputUserName")?.focus();
    } else {
      updateUserGreeting(s.user_name);
    }

    // Apply theme
    if (s.theme) {
      document.body.setAttribute("data-theme", s.theme);
      const themeSelect = document.getElementById("set-theme");
      if (themeSelect) themeSelect.value = s.theme;
    }

    const downloadDir = document.getElementById("set-download-dir");
    if (downloadDir) {
      downloadDir.value = s.default_download_dir || "";
    }

    const settingUserName = document.getElementById("setting-user-name");
    if (settingUserName && s.user_name) {
      settingUserName.value = s.user_name;
    }
  }
}

// Component Builders with Zero Emojis (SVG Only)
function createDownloadRow(job) {
  const card = document.createElement("div");
  card.className = "download-card-item";

  const brandIcon = getBrandIcon(job.filename, job.source_url);
  const pct = Math.round(job.progress_pct || 0);
  const speedStr = job.speed_str || "0.0 MB/s";
  const sizeStr = job.total_size_str || formatBytes(job.total_bytes);

  const pauseSvg = `<svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><rect x="6" y="4" width="4" height="16" rx="1"/><rect x="14" y="4" width="4" height="16" rx="1"/></svg>`;
  const resumeSvg = `<svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor"><polygon points="5 3 19 12 5 21 5 3"/></svg>`;
  const cancelSvg = `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>`;
  const folderSvg = `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>`;

  card.innerHTML = `
    <div class="row-icon-cell">${brandIcon}</div>
    <div class="row-info-cell">
      <span class="row-title" title="${job.filename}">${job.filename}</span>
      <span class="row-meta">${job.platform || 'Direct'} • ${sizeStr}</span>
    </div>
    <div class="row-progress-cell">
      <div class="progress-bar-bg">
        <div class="progress-bar-fill" style="width: ${pct}%"></div>
      </div>
      <div class="progress-labels">
        <span>${pct}%</span>
        <span>${job.status === 'Active' ? speedStr : job.status}</span>
      </div>
    </div>
    <div class="row-actions-cell">
      ${job.status === 'Active' 
        ? `<button class="action-btn btn-pause" title="Pause">${pauseSvg}</button>`
        : job.status === 'Paused' 
          ? `<button class="action-btn btn-resume" title="Resume">${resumeSvg}</button>`
          : ''}
      <button class="action-btn btn-cancel" title="Cancel & Delete">${cancelSvg}</button>
      <button class="action-btn btn-folder" title="Open containing folder">${folderSvg}</button>
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

  card.querySelector(".btn-folder")?.addEventListener("click", () => {
    sendIpc("open_path", { path: job.destination_path || currentState.settings.default_download_dir });
  });

  return card;
}

function createLibraryRow(job) {
  const card = document.createElement("div");
  card.className = "download-card-item";
  const brandIcon = getBrandIcon(job.filename, job.source_url);
  const sizeStr = job.total_size_str || formatBytes(job.total_bytes);
  const deleteSvg = `<svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2.5"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>`;

  card.innerHTML = `
    <div class="row-icon-cell">${brandIcon}</div>
    <div class="row-info-cell">
      <span class="row-title" title="${job.filename}">${job.filename}</span>
      <span class="row-meta">Verified SHA-256 • ${sizeStr}</span>
    </div>
    <div class="row-actions-cell">
      <button class="btn-subtle btn-lib-open">Open</button>
      <button class="btn-subtle btn-lib-folder">Folder</button>
      <button class="action-btn btn-lib-remove" title="Remove from list">${deleteSvg}</button>
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

// Brand SVG vector logos (Zero Emojis)
function getBrandIcon(filename, url) {
  const fn = (filename || "").toLowerCase();
  const u = (url || "").toLowerCase();

  // Hugging Face
  if (fn.endsWith(".safetensors") || u.includes("huggingface") || u.startsWith("hf:")) {
    return `<svg viewBox="0 0 24 24" class="brand-svg-icon" fill="none"><circle cx="12" cy="12" r="10" fill="#FFD21E"/><path d="M8 10a1.5 1.5 0 0 1 3 0M13 10a1.5 1.5 0 0 1 3 0" stroke="#000" stroke-width="2" stroke-linecap="round"/><path d="M8 15c1.5 2 6.5 2 8 0" stroke="#000" stroke-width="2" stroke-linecap="round"/></svg>`;
  }
  // Ollama
  if (fn.endsWith(".gguf") || u.includes("ollama") || u.startsWith("ollama:")) {
    return `<svg viewBox="0 0 24 24" class="brand-svg-icon" fill="currentColor"><path d="M12 2a4 4 0 0 0-4 4v2H7a2 2 0 0 0-2 2v6a4 4 0 0 0 4 4h6a4 4 0 0 0 4-4v-6a2 2 0 0 0-2-2h-1V6a4 4 0 0 0-4-4zm-2 6V6a2 2 0 1 1 4 0v2h-4zm-1 5a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0zm5 0a1.5 1.5 0 1 1 3 0 1.5 1.5 0 0 1-3 0z"/></svg>`;
  }
  // Steam
  if (u.includes("steam")) {
    return `<svg viewBox="0 0 24 24" class="brand-svg-icon" fill="currentColor"><path d="M11.979 0C5.678 0 .511 4.86.022 11.037l6.432 2.658c.545-.371 1.203-.59 1.912-.59.063 0 .125.004.188.006l2.861-4.142V8.91c0-2.495 2.028-4.524 4.524-4.524 2.494 0 4.524 2.031 4.524 4.527s-2.03 4.525-4.524 4.525h-.105l-4.076 2.911c0 .052.005.105.005.159 0 1.875-1.515 3.396-3.39 3.396-1.635 0-3.016-1.173-3.331-2.704L.445 14.85C1.848 20.098 6.64 24 12.001 24c6.627 0 12-5.373 12-12S18.605 0 11.979 0z"/></svg>`;
  }
  // Epic Games
  if (u.includes("epicgames")) {
    return `<svg viewBox="0 0 24 24" class="brand-svg-icon" fill="currentColor"><path d="M4 1.5h16L22 17l-10 5.5L2 17 4 1.5zm13.5 13.5l1.2-10.5H5.3l1.2 10.5 5.5 3 5.5-3zM8 6h8v2H10v2h5v2h-5v2h6v2H8V6z"/></svg>`;
  }
  // YouTube / Media
  if (u.includes("youtube") || u.includes("youtu.be") || fn.endsWith(".mp4") || fn.endsWith(".mkv") || fn.endsWith(".webm")) {
    return `<svg viewBox="0 0 24 24" class="brand-svg-icon" fill="#FF0000"><path d="M23.498 6.186a3.016 3.016 0 0 0-2.122-2.136C19.505 3.545 12 3.545 12 3.545s-7.505 0-9.377.505A3.017 3.017 0 0 0 .502 6.186C0 8.07 0 12 0 12s0 3.93.502 5.814a3.016 3.016 0 0 0 2.122 2.136c1.871.505 9.376.505 9.376.505s7.505 0 9.377-.505a3.015 3.015 0 0 0 2.122-2.136C24 15.93 24 12 24 12s0-3.93-.502-5.814zM9.545 15.568V8.432L15.818 12l-6.273 3.568z"/></svg>`;
  }
  // Datasets / Parquet
  if (fn.endsWith(".parquet") || fn.endsWith(".arrow") || fn.endsWith(".jsonl") || fn.endsWith(".csv")) {
    return `<svg viewBox="0 0 24 24" class="brand-svg-icon" fill="none" stroke="currentColor" stroke-width="2"><path d="M4 6h16M4 12h16M4 18h16"/><rect x="2" y="3" width="20" height="18" rx="2"/></svg>`;
  }
  // Archives
  if (fn.endsWith(".zip") || fn.endsWith(".tar.gz") || fn.endsWith(".7z") || fn.endsWith(".rar")) {
    return `<svg viewBox="0 0 24 24" class="brand-svg-icon" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16z"/><polyline points="3.27 6.96 12 12.01 20.73 6.96"/><line x1="12" y1="22.08" x2="12" y2="12"/></svg>`;
  }
  // Documents
  if (fn.endsWith(".pdf") || fn.endsWith(".docx") || fn.endsWith(".txt") || fn.endsWith(".epub")) {
    return `<svg viewBox="0 0 24 24" class="brand-svg-icon" fill="none" stroke="currentColor" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>`;
  }
  // Default direct file
  return `<svg viewBox="0 0 24 24" class="brand-svg-icon" fill="none" stroke="currentColor" stroke-width="2"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>`;
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
