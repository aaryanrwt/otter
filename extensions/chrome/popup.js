// OTTER Browser Extension — Popup Application Logic
// Matches official 4-screen design from media_1794066109607_9f5403a4.jpg

const OTTER_BRIDGE_URL = "http://127.0.0.1:45771";
let currentTab = "download";
let isEngineOnline = false;

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
const historyList = document.getElementById("history-items-list");
const btnClearHistory = document.getElementById("btn-clear-history");

const toggleAutohandoff = document.getElementById("toggle-autohandoff");
const toggleNotifications = document.getElementById("toggle-notifications");
const toggleProgressPopup = document.getElementById("toggle-progress-popup");

const engineDot = document.getElementById("engine-dot");
const engineStatusText = document.getElementById("engine-status-text");

// Initialize on popup load
document.addEventListener("DOMContentLoaded", () => {
  setupTabs();
  loadSettings();
  pollActiveState();
  pollHistory();

  // Refresh every 1.5 seconds while popup is open
  setInterval(() => {
    if (currentTab === "download") {
      pollActiveState();
    }
  }, 1500);
});

// Tab Navigation
function setupTabs() {
  tabDownload.addEventListener("click", () => switchTab("download"));
  tabHistory.addEventListener("click", () => {
    switchTab("history");
    pollHistory();
  });
  tabSettings.addEventListener("click", () => switchTab("settings"));

  btnClearHistory.addEventListener("click", async () => {
    try {
      await fetch(`${OTTER_BRIDGE_URL}/api/browser/history/clear`, { method: "POST" });
      historyList.innerHTML = `<div class="empty-state">No download history.</div>`;
    } catch (e) {
      console.warn("Could not clear history:", e);
    }
  });

  toggleAutohandoff.addEventListener("change", (e) => {
    chrome.runtime.sendMessage({ action: "SET_AUTO_HANDOFF", enabled: e.target.checked });
  });

  toggleNotifications.addEventListener("change", (e) => {
    chrome.storage.local.set({ notificationsEnabled: e.target.checked });
  });

  toggleProgressPopup.addEventListener("change", (e) => {
    chrome.storage.local.set({ showProgressPopup: e.target.checked });
  });
}

function switchTab(tabName) {
  currentTab = tabName;
  [tabDownload, tabHistory, tabSettings].forEach(t => t.classList.remove("active"));
  [viewDownloads, viewHistory, viewSettings].forEach(v => v.classList.remove("active"));

  if (tabName === "download") {
    tabDownload.classList.add("active");
    viewDownloads.classList.add("active");
  } else if (tabName === "history") {
    tabHistory.classList.add("active");
    viewHistory.classList.add("active");
  } else if (tabName === "settings") {
    tabSettings.classList.add("active");
    viewSettings.classList.add("active");
  }
}

// Load Settings from chrome.storage
function loadSettings() {
  chrome.storage.local.get(["autoHandoff", "notificationsEnabled", "showProgressPopup"], (res) => {
    if (toggleAutohandoff) toggleAutohandoff.checked = !!res.autoHandoff;
    if (toggleNotifications) toggleNotifications.checked = res.notificationsEnabled !== false;
    if (toggleProgressPopup) toggleProgressPopup.checked = res.showProgressPopup !== false;
  });
}

// Poll Active Downloads from OTTER Bridge and Chrome downloads API
async function pollActiveState() {
  let otterItems = [];
  try {
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/downloads`, { method: "GET" });
    if (res.ok) {
      const data = await res.json();
      otterItems = data.downloads || [];
      setEngineStatus(true);
    } else {
      setEngineStatus(false);
    }
  } catch (e) {
    setEngineStatus(false);
  }

  // Also query Chrome native downloads API for active items
  chrome.downloads.search({ state: "in_progress" }, (chromeItems) => {
    const activeCombined = mergeActiveDownloads(otterItems, chromeItems || []);
    renderActiveDownloads(activeCombined);
  });
}

function setEngineStatus(online) {
  isEngineOnline = online;
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

function mergeActiveDownloads(otterItems, chromeItems) {
  const map = new Map();

  // Ingest Otter tracked items
  for (const it of otterItems) {
    if (it.state === "in_progress" || it.state === "paused" || it.status.includes("Active") || it.status.includes("Eligible")) {
      map.set(it.id, {
        id: it.id,
        filename: it.filename || "download",
        total_bytes: it.total_bytes,
        received_bytes: it.received_bytes || 0,
        speed_bps: it.current_speed_bps || 0,
        domain: it.domain || "Direct Web",
        is_otter: !!it.is_otter_managed,
        status: it.status || "Active",
        url: it.url || it.sanitized_url
      });
    }
  }

  // Ingest Chrome native items if not already present
  for (const c of chromeItems) {
    if (!map.has(c.id)) {
      const fn = c.filename ? c.filename.split(/[\\/]/).pop() : "download";
      const domain = extractDomain(c.url || c.finalUrl);
      map.set(c.id, {
        id: c.id,
        filename: fn,
        total_bytes: c.totalBytes > 0 ? c.totalBytes : null,
        received_bytes: c.bytesReceived || 0,
        speed_bps: c.estimatedEndTime ? 0 : 0,
        domain: domain,
        is_otter: false,
        status: c.paused ? "Paused" : "Active in Chrome",
        url: c.url
      });
    }
  }

  return Array.from(map.values());
}

function renderActiveDownloads(items) {
  if (!items || items.length === 0) {
    downloadsHeader.style.display = "none";
    activeList.innerHTML = "";
    preparingState.style.display = "flex";
    return;
  }

  downloadsHeader.style.display = "flex";
  badgeActiveCount.textContent = items.length;
  preparingState.style.display = "none";

  activeList.innerHTML = "";
  items.forEach(item => {
    const card = document.createElement("div");
    card.className = "dl-card";

    const pct = item.total_bytes && item.total_bytes > 0
      ? Math.min(100, Math.round((item.received_bytes / item.total_bytes) * 100))
      : 0;

    const sizeStr = item.total_bytes ? formatBytes(item.total_bytes) : formatBytes(item.received_bytes);
    const speedStr = item.speed_bps > 0 ? `${(item.speed_bps / (1024 * 1024)).toFixed(1)} MB/s` : (item.status.includes("Paused") ? "Paused" : "Active");
    const etaStr = computeEta(item.total_bytes, item.received_bytes, item.speed_bps);

    const iconSvg = getMediaIconSvg(item.filename, item.domain);
    const ownershipBadge = item.is_otter
      ? `<span class="ownership-tag tag-otter">OTTER Multi-Stream</span>`
      : `<span class="ownership-tag tag-native">Chrome Native</span>`;

    card.innerHTML = `
      <div class="dl-top-row">
        <div class="media-type-icon">
          ${iconSvg}
        </div>
        <div class="dl-info">
          <div class="dl-filename" title="${escapeHtml(item.filename)}">${escapeHtml(item.filename)}</div>
          <div class="dl-metadata">${sizeStr} • ${escapeHtml(item.domain)} ${ownershipBadge}</div>
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
          <button class="card-action-btn btn-pause-toggle" data-id="${item.id}" title="${item.status.includes('Paused') ? 'Resume' : 'Pause'}">
            ${item.status.includes('Paused') ? '▶' : '||'}
          </button>
          <button class="card-action-btn btn-cancel" data-id="${item.id}" title="Cancel">✕</button>
        </div>
      </div>
    `;

    // Attach button click actions
    card.querySelector(".btn-pause-toggle")?.addEventListener("click", () => {
      if (item.status.includes("Paused")) {
        chrome.downloads.resume(item.id);
      } else {
        chrome.downloads.pause(item.id);
      }
    });

    card.querySelector(".btn-cancel")?.addEventListener("click", () => {
      chrome.downloads.cancel(item.id);
    });

    activeList.appendChild(card);
  });
}

// Poll Download History from OTTER Manifest
async function pollHistory() {
  try {
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/history`, { method: "GET" });
    if (res.ok) {
      const data = await res.json();
      renderHistory(data.history || []);
    }
  } catch (e) {
    // If desktop engine is offline, query chrome.downloads history fallback
    chrome.downloads.search({ state: "complete", limit: 15 }, (chromeItems) => {
      renderHistoryFromChrome(chromeItems || []);
    });
  }
}

function renderHistory(items) {
  if (!items || items.length === 0) {
    historyList.innerHTML = `<div class="empty-state">No completed downloads found.</div>`;
    return;
  }

  historyList.innerHTML = "";
  items.forEach(h => {
    const card = document.createElement("div");
    card.className = "history-card";
    const iconSvg = getMediaIconSvg(h.filename, h.url || "");

    card.innerHTML = `
      <div class="hist-icon" style="background: #FFE4E8;">
        ${iconSvg}
      </div>
      <div class="hist-info">
        <div class="hist-title" title="${escapeHtml(h.filename)}">${escapeHtml(h.filename)}</div>
        <div class="hist-meta">${h.total_str || formatBytes(h.total_bytes)} • ${h.elapsed_desc || 'Completed'}</div>
      </div>
      <div class="completed-badge">
        <span>✓ Completed</span>
      </div>
      <button class="menu-dots-btn" title="Actions">⋮</button>
    `;
    historyList.appendChild(card);
  });
}

function renderHistoryFromChrome(items) {
  if (!items || items.length === 0) {
    historyList.innerHTML = `<div class="empty-state">No completed downloads found.</div>`;
    return;
  }

  historyList.innerHTML = "";
  items.forEach(c => {
    const fn = c.filename ? c.filename.split(/[\\/]/).pop() : "download";
    const iconSvg = getMediaIconSvg(fn, c.url || "");
    const card = document.createElement("div");
    card.className = "history-card";

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
      <button class="menu-dots-btn" title="Actions">⋮</button>
    `;
    historyList.appendChild(card);
  });
}

// Helpers
function formatBytes(bytes) {
  if (!bytes || bytes <= 0) return "0 B";
  const KB = 1024, MB = KB * 1024, GB = MB * 1024;
  if (bytes >= GB) return (bytes / GB).toFixed(1) + " GB";
  if (bytes >= MB) return (bytes / MB).toFixed(1) + " MB";
  if (bytes >= KB) return (bytes / KB).toFixed(0) + " KB";
  return bytes + " B";
}

function computeEta(total, rx, speedBps) {
  if (!total || !speedBps || speedBps <= 0) return "00:00:00 left";
  const rem = Math.max(0, total - rx);
  const sec = Math.round(rem / speedBps);
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = sec % 60;
  return `${padZero(h)}:${padZero(m)}:${padZero(s)} left`;
}

function padZero(n) {
  return n < 10 ? "0" + n : n;
}

function extractDomain(rawUrl) {
  try {
    return new URL(rawUrl).hostname.replace(/^www\./, "");
  } catch (e) {
    return "Direct Web";
  }
}

function escapeHtml(str) {
  return (str || "").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

function getMediaIconSvg(filename, urlOrDomain) {
  const lower = (filename + " " + urlOrDomain).toLowerCase();
  if (lower.includes("youtube") || lower.endsWith(".mp4") || lower.endsWith(".webm") || lower.endsWith(".mkv")) {
    return `<svg viewBox="0 0 24 24" width="22" height="22" fill="#E11D48"><polygon points="5 3 19 12 5 21 5 3"/></svg>`;
  }
  if (lower.includes("huggingface") || lower.endsWith(".gguf") || lower.endsWith(".safetensors") || lower.endsWith(".parquet")) {
    return `<span style="font-size: 20px;">🤗</span>`;
  }
  if (lower.endsWith(".mp3") || lower.endsWith(".wav") || lower.endsWith(".flac") || lower.endsWith(".m4a")) {
    return `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="#9333EA" stroke-width="2.2"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>`;
  }
  if (lower.endsWith(".zip") || lower.endsWith(".rar") || lower.endsWith(".7z") || lower.endsWith(".tar.gz")) {
    return `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="#D97706" stroke-width="2.2"><rect x="3" y="3" width="18" height="18" rx="2"/><line x1="12" y1="3" x2="12" y2="21"/><rect x="10" y="8" width="4" height="4"/></svg>`;
  }
  return `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="#4B5563" stroke-width="2.2"><path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><polyline points="13 2 13 9 20 9"/></svg>`;
}
