// OTTER Browser Extension — Popup Application Logic v0.1.0
// Real-time integration with high-throughput OTTER Rust Engine

const OTTER_BRIDGE_URL = "http://127.0.0.1:45771";
let currentTab = "download";
let isEngineOnline = false;
let autoHandoffEnabled = true;

// Prevent repeated handoff requests for the same download
const popupHandedOff = new Set();

// Speed tracking state across poll intervals
const speedHistoryMap = new Map(); // downloadId -> { bytes, time, speedBps }

// Preparing loading animation state (2.5 seconds loading screen)
let isPreparing = false;
let prepareTimer = null;
let lastActiveCount = 0;

// DOM Elements
const tabDownload = document.getElementById("tab-download");
const tabHistory = document.getElementById("tab-history");
const tabSettings = document.getElementById("tab-settings");

const viewDownloads = document.getElementById("view-downloads");
const viewHistory = document.getElementById("view-history");
const viewSettings = document.getElementById("view-settings");

const downloadsHeader = document.getElementById("downloads-header");
const badgeActiveCount = document.getElementById("badge-active-count");
const activeList = document.getElementById("active-downloads-list");
const preparingState = document.getElementById("preparing-state");
const idleState = document.getElementById("idle-state");
const btnIdleHistory = document.getElementById("btn-idle-history");

const historyList = document.getElementById("history-items-list");
const btnClearHistory = document.getElementById("btn-clear-history");

const settingHistoryChoice = document.getElementById("setting-history-choice");
const toggleAutohandoff = document.getElementById("toggle-autohandoff");
const toggleNotifications = document.getElementById("toggle-notifications");
const toggleProgressPopup = document.getElementById("toggle-progress-popup");

const engineDot = document.getElementById("engine-dot");
const engineStatusText = document.getElementById("engine-status-text");

const HISTORY_CHOICES = ["Last 7 Days", "Last 30 Days", "Last 90 Days", "Keep Forever"];

// Open folder where file was downloaded (Windows Explorer)
function openFolderForDownload(downloadId, destinationPath) {
  // 1. First attempt to open exact file or containing folder via OTTER Bridge Explorer process
  if (destinationPath) {
    fetch(`${OTTER_BRIDGE_URL}/api/browser/open_folder`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: destinationPath })
    }).catch(() => {});
  }

  // 2. Also invoke Chrome's native show finder if downloadId is a recognized browser id
  if (downloadId && typeof downloadId === "number") {
    try {
      chrome.downloads.show(downloadId);
    } catch (e) {}
  }
}

// Initialize on popup load
document.addEventListener("DOMContentLoaded", () => {
  setupTabs();
  loadSettings();
  checkEngineHeartbeat();
  pollActiveState();
  pollHistory();

  // Continuous background refresh every 1.2 seconds while popup is open
  setInterval(() => {
    checkEngineHeartbeat();
    if (currentTab === "download" && !isPreparing) {
      pollActiveState();
    }
  }, 1200);
});

// Tab Navigation
function setupTabs() {
  if (tabDownload) tabDownload.addEventListener("click", () => switchTab("download"));
  if (tabHistory) {
    tabHistory.addEventListener("click", () => {
      switchTab("history");
      pollHistory();
    });
  }
  if (tabSettings) {
    tabSettings.addEventListener("click", () => {
      switchTab("settings");
      checkEngineHeartbeat();
    });
  }

  if (btnIdleHistory) {
    btnIdleHistory.addEventListener("click", () => {
      switchTab("history");
      pollHistory();
    });
  }

  // Clear All Button - Erases completed downloads everywhere
  if (btnClearHistory) {
    btnClearHistory.addEventListener("click", async () => {
      try {
        chrome.downloads.erase({ state: "complete" }, () => {});
      } catch (e) {}

      try {
        await fetch(`${OTTER_BRIDGE_URL}/api/browser/history/clear`, { method: "POST" });
      } catch (e) {}

      try {
        chrome.storage.local.set({ completedHistory: [] });
      } catch (e) {}

      if (historyList) {
        historyList.innerHTML = `<div class="empty-state">No download history.</div>`;
      }
    });
  }

  // Settings: Keep History Choice
  if (settingHistoryChoice) {
    settingHistoryChoice.addEventListener("click", () => {
      const current = settingHistoryChoice.querySelector("span")?.textContent || HISTORY_CHOICES[1];
      const nextIdx = (HISTORY_CHOICES.indexOf(current) + 1) % HISTORY_CHOICES.length;
      const nextVal = HISTORY_CHOICES[nextIdx];
      settingHistoryChoice.querySelector("span").textContent = nextVal;
      chrome.storage.local.set({ keepHistory: nextVal });
    });
  }

  // Settings: Toggles
  if (toggleAutohandoff) {
    toggleAutohandoff.addEventListener("change", (e) => {
      autoHandoffEnabled = e.target.checked;
      try {
        chrome.runtime.sendMessage({ action: "SET_AUTO_HANDOFF", enabled: autoHandoffEnabled });
      } catch (err) {}
      chrome.storage.local.set({ autoHandoff: autoHandoffEnabled });
    });
  }

  if (toggleNotifications) {
    toggleNotifications.addEventListener("change", (e) => {
      chrome.storage.local.set({ notificationsEnabled: e.target.checked });
    });
  }

  if (toggleProgressPopup) {
    toggleProgressPopup.addEventListener("change", (e) => {
      chrome.storage.local.set({ showProgressPopup: e.target.checked });
    });
  }
}

function switchTab(tabName) {
  currentTab = tabName;
  [tabDownload, tabHistory, tabSettings].forEach(t => t && t.classList.remove("active"));
  [viewDownloads, viewHistory, viewSettings].forEach(v => v && v.classList.remove("active"));

  if (tabName === "download") {
    if (tabDownload) tabDownload.classList.add("active");
    if (viewDownloads) viewDownloads.classList.add("active");
  } else if (tabName === "history") {
    if (tabHistory) tabHistory.classList.add("active");
    if (viewHistory) viewHistory.classList.add("active");
  } else if (tabName === "settings") {
    if (tabSettings) tabSettings.classList.add("active");
    if (viewSettings) viewSettings.classList.add("active");
  }
}

// Load Settings from chrome.storage
function loadSettings() {
  chrome.storage.local.get(
    ["autoHandoff", "notificationsEnabled", "showProgressPopup", "keepHistory"],
    (res) => {
      if (chrome.runtime.lastError) return;
      autoHandoffEnabled = res.autoHandoff !== undefined ? !!res.autoHandoff : true;
      if (toggleAutohandoff) toggleAutohandoff.checked = autoHandoffEnabled;

      if (toggleNotifications) toggleNotifications.checked = res.notificationsEnabled !== false;
      if (toggleProgressPopup) toggleProgressPopup.checked = res.showProgressPopup !== false;

      if (settingHistoryChoice && res.keepHistory) {
        const span = settingHistoryChoice.querySelector("span");
        if (span) span.textContent = res.keepHistory;
      }
    }
  );
}

// Real-time Engine Connection Heartbeat
async function checkEngineHeartbeat() {
  try {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 1500);
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/status`, {
      method: "GET",
      signal: controller.signal
    });
    clearTimeout(timeoutId);
    if (res.ok) {
      setEngineStatus(true);
    } else {
      setEngineStatus(false);
    }
  } catch (e) {
    setEngineStatus(false);
  }
}

function setEngineStatus(online) {
  isEngineOnline = online;
  if (!engineDot || !engineStatusText) return;
  if (online) {
    engineDot.className = "status-indicator-dot dot-connected";
    engineStatusText.textContent = "Connected (v0.1.0)";
    engineStatusText.style.color = "#10B981";
  } else {
    engineDot.className = "status-indicator-dot dot-disconnected";
    engineStatusText.textContent = "Disconnected (Desktop App Offline)";
    engineStatusText.style.color = "#EF4444";
  }
}

// Trigger Preparing Animation for 2.5 seconds (loading screen before showing download card)
function startPreparingAnimation(onComplete) {
  isPreparing = true;
  if (preparingState) preparingState.style.display = "flex";
  if (idleState) idleState.style.display = "none";
  if (activeList) activeList.style.display = "none";
  if (downloadsHeader) downloadsHeader.style.display = "none";

  if (prepareTimer) clearTimeout(prepareTimer);
  prepareTimer = setTimeout(() => {
    isPreparing = false;
    if (preparingState) preparingState.style.display = "none";
    if (onComplete) onComplete();
    pollActiveState();
  }, 2500);
}

// Poll Active Downloads
async function pollActiveState() {
  if (isPreparing) return;

  let otterItems = [];
  try {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 1500);
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/downloads`, {
      method: "GET",
      signal: controller.signal
    });
    clearTimeout(timeoutId);
    if (res.ok) {
      const data = await res.json();
      otterItems = data.downloads || [];
      setEngineStatus(true);
    }
  } catch (e) {}

  chrome.downloads.search({ state: "in_progress" }, (chromeItems) => {
    if (chrome.runtime.lastError) chromeItems = [];
    const activeCombined = mergeActiveDownloads(otterItems, chromeItems || []);

    // If new downloads appeared while idle, show the preparing loading screen for 2.5s
    if (activeCombined.length > 0 && lastActiveCount === 0 && !isPreparing) {
      lastActiveCount = activeCombined.length;
      startPreparingAnimation(() => {
        renderActiveDownloads(activeCombined);
      });
      return;
    }

    lastActiveCount = activeCombined.length;
    renderActiveDownloads(activeCombined);
  });
}

function mergeActiveDownloads(otterItems, chromeItems) {
  const map = new Map();
  const now = Date.now();

  // 1. Ingest Otter backend downloads
  for (const it of (otterItems || [])) {
    const st = (it.status || "").toLowerCase();
    const isPaused = it.state === "paused" || st.includes("paused");
    if (it.state === "in_progress" || isPaused || st.includes("active") || st.includes("eligible")) {
      map.set(it.id, {
        id: it.id,
        filename: it.filename || "download",
        total_bytes: it.total_bytes,
        received_bytes: it.received_bytes || 0,
        speed_bps: it.current_speed_bps || 0,
        domain: it.domain || "Direct Web",
        is_otter: true,
        is_paused: isPaused,
        status: isPaused ? "Paused" : (it.status || "Active"),
        url: it.url || it.sanitized_url,
        estimatedEndTime: null
      });
    }
  }

  // 2. Ingest Chrome native downloads and auto-accelerate
  for (const c of (chromeItems || [])) {
    if (!map.has(c.id)) {
      const fn = c.filename ? c.filename.split(/[\\/]/).pop() : "download";
      const domain = extractDomain(c.url || c.finalUrl);
      const rx = c.bytesReceived || 0;

      // Real speed calculation via delta bytes over delta time
      let speedBps = 0;
      const prev = speedHistoryMap.get(c.id);
      if (prev) {
        const dt = (now - prev.time) / 1000;
        if (dt >= 0.5) {
          const db = rx - prev.bytes;
          if (db >= 0) {
            speedBps = db / dt;
            speedHistoryMap.set(c.id, { bytes: rx, time: now, speedBps });
          } else {
            speedBps = prev.speedBps || 0;
          }
        } else {
          speedBps = prev.speedBps || 0;
        }
      } else {
        speedHistoryMap.set(c.id, { bytes: rx, time: now, speedBps: 0 });
      }

      // Auto-handoff to OTTER multi-stream: pause single stream & send to bridge
      if (autoHandoffEnabled && !popupHandedOff.has(c.id)) {
        popupHandedOff.add(c.id);
        const payload = {
          id: c.id,
          url: c.url,
          final_url: c.finalUrl || c.url,
          sanitized_url: c.url,
          filename: fn,
          target_path: c.filename || "",
          mime_type: c.mime || "application/octet-stream",
          total_bytes: c.totalBytes > 0 ? c.totalBytes : null,
          received_bytes: rx,
          state: "in_progress",
          referrer: c.referrer || "",
          start_time: c.startTime || new Date().toISOString(),
          auto_handoff_requested: true
        };
        fetch(`${OTTER_BRIDGE_URL}/api/browser/download/created`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload)
        }).catch(() => {});

        chrome.downloads.pause(c.id, () => {
          if (chrome.runtime.lastError) {}
        });
      }

      const isPaused = !!c.paused;
      map.set(c.id, {
        id: c.id,
        filename: fn,
        total_bytes: c.totalBytes > 0 ? c.totalBytes : null,
        received_bytes: rx,
        speed_bps: speedBps,
        domain: domain,
        is_otter: true,
        is_paused: isPaused,
        status: isPaused ? "Paused" : "Active",
        url: c.url,
        estimatedEndTime: c.estimatedEndTime
      });
    }
  }

  return Array.from(map.values());
}

function renderActiveDownloads(items) {
  if (isPreparing) return;

  if (!items || items.length === 0) {
    if (downloadsHeader) downloadsHeader.style.display = "none";
    if (activeList) {
      activeList.innerHTML = "";
      activeList.style.display = "none";
    }
    if (preparingState) preparingState.style.display = "none";
    if (idleState) idleState.style.display = "flex";
    return;
  }

  if (idleState) idleState.style.display = "none";
  if (preparingState) preparingState.style.display = "none";
  if (downloadsHeader) downloadsHeader.style.display = "flex";
  if (activeList) activeList.style.display = "block";
  if (badgeActiveCount) badgeActiveCount.textContent = items.length;

  if (!activeList) return;
  activeList.innerHTML = "";

  items.forEach(item => {
    const card = document.createElement("div");
    card.className = "dl-card";

    const pct = item.total_bytes && item.total_bytes > 0
      ? Math.min(100, Math.round((item.received_bytes / item.total_bytes) * 100))
      : 0;

    const sizeStr = item.total_bytes ? formatBytes(item.total_bytes) : formatBytes(item.received_bytes);
    const iconSvg = getMediaIconSvg(item.filename, item.domain);
    const isPaused = item.is_paused || (item.status && item.status.toLowerCase().includes("paused"));
    const pauseIcon = isPaused ? '▶' : '||';
    const pauseTitle = isPaused ? 'Resume / Restart' : 'Pause';

    // Dynamic speed formatting
    let speedStr = "";
    if (isPaused) {
      speedStr = "Paused";
    } else if (item.speed_bps > 0) {
      speedStr = `${(item.speed_bps / (1024 * 1024)).toFixed(1)} MB/s`;
    } else {
      speedStr = "Accelerating...";
    }

    // Accurate ETA calculation
    let etaStr = "";
    if (isPaused) {
      etaStr = "Paused";
    } else if (item.speed_bps > 0 && item.total_bytes && item.total_bytes > item.received_bytes) {
      const remSec = Math.round((item.total_bytes - item.received_bytes) / item.speed_bps);
      etaStr = formatDuration(remSec) + " left";
    } else if (item.estimatedEndTime) {
      const remSec = Math.max(0, Math.round((new Date(item.estimatedEndTime).getTime() - Date.now()) / 1000));
      etaStr = formatDuration(remSec) + " left";
    } else {
      etaStr = "In Progress";
    }

    // Clean UI: zero badges next to filename, pure speed and accurate progress
    card.innerHTML = `
      <div class="dl-top-row">
        <div class="media-type-icon">
          ${iconSvg}
        </div>
        <div class="dl-info">
          <div class="dl-filename" title="${escapeHtml(item.filename)}">${escapeHtml(item.filename)}</div>
          <div class="dl-metadata">${sizeStr} • ${escapeHtml(item.domain)}</div>
        </div>
      </div>

      <div class="dl-progress-row">
        <div class="progress-track">
          <div class="progress-fill" style="width: ${pct}%;"></div>
        </div>
        <div class="progress-pct-label">${pct}%</div>
      </div>

      <div class="dl-bottom-row">
        <div class="dl-stats-text">
          <svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2.5">
            <polyline points="7 10 12 15 17 10"/>
            <line x1="12" y1="15" x2="12" y2="3"/>
          </svg>
          <span>${speedStr} • ~ ${etaStr}</span>
        </div>
        <div class="dl-actions">
          <button class="card-action-btn btn-pause-toggle" data-id="${item.id}" title="${pauseTitle}">
            ${pauseIcon}
          </button>
          <button class="card-action-btn btn-cancel" data-id="${item.id}" title="Cancel">✕</button>
        </div>
      </div>
    `;

    // Pause/Resume button
    card.querySelector(".btn-pause-toggle")?.addEventListener("click", async (e) => {
      e.stopPropagation();
      const currentlyPaused = item.is_paused || (item.status && item.status.toLowerCase().includes("paused"));
      if (currentlyPaused) {
        try {
          await fetch(`${OTTER_BRIDGE_URL}/api/browser/download/resume`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ id: item.id })
          });
        } catch (err) {}
        try {
          chrome.downloads.resume(item.id, () => {
            if (chrome.runtime.lastError) {}
          });
        } catch (err) {}
      } else {
        try {
          await fetch(`${OTTER_BRIDGE_URL}/api/browser/download/pause`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ id: item.id })
          });
        } catch (err) {}
        try {
          chrome.downloads.pause(item.id, () => {
            if (chrome.runtime.lastError) {}
          });
        } catch (err) {}
      }
      setTimeout(pollActiveState, 200);
    });

    // Cancel / Close button
    card.querySelector(".btn-cancel")?.addEventListener("click", async (e) => {
      e.stopPropagation();
      try {
        await fetch(`${OTTER_BRIDGE_URL}/api/browser/download/cancel`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ id: item.id })
        });
      } catch (err) {}
      try {
        chrome.downloads.cancel(item.id, () => {
          if (chrome.runtime.lastError) {}
        });
      } catch (err) {}
      setTimeout(pollActiveState, 200);
    });

    activeList.appendChild(card);
  });
}

// Poll History
async function pollHistory() {
  let otterHistory = [];
  try {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 1500);
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/history`, {
      method: "GET",
      signal: controller.signal
    });
    clearTimeout(timeoutId);
    if (res.ok) {
      const data = await res.json();
      otterHistory = data.history || [];
    }
  } catch (e) {}

  if (otterHistory.length > 0) {
    renderHistoryCards(otterHistory);
  } else {
    chrome.downloads.search({ state: "complete", limit: 30 }, (chromeItems) => {
      if (chrome.runtime.lastError) chromeItems = [];
      renderHistoryFromChrome(chromeItems || []);
    });
  }
}

function renderHistoryCards(items) {
  if (!historyList) return;
  if (!items || items.length === 0) {
    historyList.innerHTML = `<div class="empty-state">No download history.</div>`;
    return;
  }

  historyList.innerHTML = "";
  items.forEach(item => {
    const card = document.createElement("div");
    card.className = "history-card";
    const iconSvg = getMediaIconSvg(item.filename, "");

    card.style.cursor = "pointer";
    card.innerHTML = `
      <div class="hist-icon" style="background: #FFE4E8;">
        ${iconSvg}
      </div>
      <div class="hist-info">
        <div class="hist-title" title="${escapeHtml(item.filename)}">${escapeHtml(item.filename)}</div>
        <div class="hist-meta">${formatBytes(item.total_bytes)} • ${escapeHtml(item.time_ago || "Completed")}</div>
      </div>
      <div class="completed-badge">
        <span>✓ Completed</span>
      </div>
      <button class="menu-dots-btn" title="Open containing folder">⋮</button>
    `;

    // Click on 3 dots or click anywhere on card opens folder
    card.querySelector(".menu-dots-btn")?.addEventListener("click", (e) => {
      e.stopPropagation();
      openFolderForDownload(item.id, item.destination || item.filename);
    });
    card.addEventListener("click", () => {
      openFolderForDownload(item.id, item.destination || item.filename);
    });

    historyList.appendChild(card);
  });
}

function renderHistoryFromChrome(items) {
  if (!historyList) return;
  if (!items || items.length === 0) {
    historyList.innerHTML = `<div class="empty-state">No download history.</div>`;
    return;
  }

  historyList.innerHTML = "";
  items.forEach(c => {
    const fn = c.filename ? c.filename.split(/[\\/]/).pop() : "download";
    const iconSvg = getMediaIconSvg(fn, c.url || "");
    const card = document.createElement("div");
    card.className = "history-card";
    card.style.cursor = "pointer";

    card.innerHTML = `
      <div class="hist-icon" style="background: #FFE4E8;">
        ${iconSvg}
      </div>
      <div class="hist-info">
        <div class="hist-title" title="${escapeHtml(fn)}">${escapeHtml(fn)}</div>
        <div class="hist-meta">${formatBytes(c.fileSize || c.totalBytes || 0)} • Completed</div>
      </div>
      <div class="completed-badge">
        <span>✓ Completed</span>
      </div>
      <button class="menu-dots-btn" title="Open containing folder">⋮</button>
    `;

    // Click on 3 dots or card opens folder
    card.querySelector(".menu-dots-btn")?.addEventListener("click", (e) => {
      e.stopPropagation();
      openFolderForDownload(c.id, c.filename);
    });
    card.addEventListener("click", () => {
      openFolderForDownload(c.id, c.filename);
    });

    historyList.appendChild(card);
  });
}

// Format Utilities
function formatBytes(bytes) {
  if (!bytes || bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
}

function formatDuration(sec) {
  if (sec <= 0 || isNaN(sec)) return "00:00:00";
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  return `${padZero(h)}:${padZero(m)}:${padZero(s)}`;
}

function padZero(n) {
  return n < 10 ? "0" + n : n;
}

function extractDomain(url) {
  if (!url) return "Direct Web";
  try {
    const parsed = new URL(url);
    return parsed.hostname.replace(/^www\./, "");
  } catch (e) {
    return "Direct Web";
  }
}

function escapeHtml(str) {
  if (!str) return "";
  return String(str)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

function getMediaIconSvg(filename, domain) {
  const lower = (filename || "").toLowerCase();
  const lowerDomain = (domain || "").toLowerCase();

  // Video
  if (lower.match(/\.(mp4|mkv|webm|avi|mov|flv|wmv)$/) || lowerDomain.includes("youtube") || lowerDomain.includes("vimeo")) {
    return `<svg viewBox="0 0 24 24" width="22" height="22" fill="#E11D48"><polygon points="5 3 19 12 5 21 5 3"/></svg>`;
  }
  // Audio
  if (lower.match(/\.(mp3|wav|ogg|flac|aac|m4a)$/) || lowerDomain.includes("soundcloud") || lowerDomain.includes("spotify")) {
    return `<svg viewBox="0 0 24 24" width="22" height="22" fill="#E11D48"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>`;
  }
  // Archives / ISOs / Game Installers
  if (lower.match(/\.(zip|rar|7z|tar|gz|iso|exe|msi|dmg|pkg)$/)) {
    return `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="#E11D48" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="10" y1="12" x2="14" y2="12"/><line x1="10" y1="15" x2="14" y2="15"/></svg>`;
  }
  // Generic Document / Data
  return `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="#E11D48" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/></svg>`;
}
