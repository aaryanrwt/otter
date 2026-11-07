// OTTER Browser Extension — Popup Application Logic
// Matches official 4-screen design from media_1794066109607_9f5403a4.jpg

const OTTER_BRIDGE_URL = "http://127.0.0.1:45771";
let currentTab = "download";
let isEngineOnline = false;

// Speed tracking state across poll intervals
const speedHistoryMap = new Map(); // downloadId -> { bytes, time, speedBps }

// Preparing loading animation state (2-3 seconds as requested)
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

const settingEngineChoice = document.getElementById("setting-engine-choice");
const settingHistoryChoice = document.getElementById("setting-history-choice");
const toggleAutohandoff = document.getElementById("toggle-autohandoff");
const toggleNotifications = document.getElementById("toggle-notifications");
const toggleProgressPopup = document.getElementById("toggle-progress-popup");

const engineDot = document.getElementById("engine-dot");
const engineStatusText = document.getElementById("engine-status-text");

const ENGINE_CHOICES = ["Auto (Recommended)", "Force OTTER Multi-Stream", "Chrome Native Only"];
const HISTORY_CHOICES = ["Last 7 Days", "Last 30 Days", "Last 90 Days", "Keep Forever"];

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
  tabDownload.addEventListener("click", () => switchTab("download"));
  tabHistory.addEventListener("click", () => {
    switchTab("history");
    pollHistory();
  });
  tabSettings.addEventListener("click", () => {
    switchTab("settings");
    checkEngineHeartbeat();
  });

  if (btnIdleHistory) {
    btnIdleHistory.addEventListener("click", () => {
      switchTab("history");
      pollHistory();
    });
  }

  // Clear All Button - Erases completed downloads everywhere
  btnClearHistory.addEventListener("click", async () => {
    try {
      chrome.downloads.erase({ state: "complete" });
    } catch (e) {
      console.warn("Could not erase Chrome downloads:", e);
    }

    try {
      await fetch(`${OTTER_BRIDGE_URL}/api/browser/history/clear`, { method: "POST" });
    } catch (e) {
      console.warn("Could not clear bridge history:", e);
    }

    chrome.storage.local.set({ completedHistory: [] });
    historyList.innerHTML = `<div class="empty-state">No download history.</div>`;
  });

  // Settings: Download Engine Choice
  if (settingEngineChoice) {
    settingEngineChoice.addEventListener("click", () => {
      const current = settingEngineChoice.querySelector("span")?.textContent || ENGINE_CHOICES[0];
      const nextIdx = (ENGINE_CHOICES.indexOf(current) + 1) % ENGINE_CHOICES.length;
      const nextVal = ENGINE_CHOICES[nextIdx];
      settingEngineChoice.querySelector("span").textContent = nextVal;
      chrome.storage.local.set({ downloadEngine: nextVal });
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
  toggleAutohandoff.addEventListener("change", (e) => {
    chrome.runtime.sendMessage({ action: "SET_AUTO_HANDOFF", enabled: e.target.checked });
    chrome.storage.local.set({ autoHandoff: e.target.checked });
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
  chrome.storage.local.get(
    ["autoHandoff", "notificationsEnabled", "showProgressPopup", "downloadEngine", "keepHistory"],
    (res) => {
      // Auto-Handoff is enabled by default for seamless acceleration
      const autoHandoffVal = res.autoHandoff !== undefined ? !!res.autoHandoff : true;
      if (toggleAutohandoff) toggleAutohandoff.checked = autoHandoffVal;
      if (res.autoHandoff === undefined) {
        chrome.storage.local.set({ autoHandoff: true });
        chrome.runtime.sendMessage({ action: "SET_AUTO_HANDOFF", enabled: true });
      }

      if (toggleNotifications) toggleNotifications.checked = res.notificationsEnabled !== false;
      if (toggleProgressPopup) toggleProgressPopup.checked = res.showProgressPopup !== false;

      if (settingEngineChoice && res.downloadEngine) {
        settingEngineChoice.querySelector("span").textContent = res.downloadEngine;
      }
      if (settingHistoryChoice && res.keepHistory) {
        settingHistoryChoice.querySelector("span").textContent = res.keepHistory;
      }
    }
  );
}

// Real-time Engine Connection Heartbeat
async function checkEngineHeartbeat() {
  try {
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/status`, { method: "GET" });
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
  preparingState.style.display = "flex";
  idleState.style.display = "none";
  activeList.style.display = "none";
  downloadsHeader.style.display = "none";

  if (prepareTimer) clearTimeout(prepareTimer);
  prepareTimer = setTimeout(() => {
    isPreparing = false;
    preparingState.style.display = "none";
    if (onComplete) onComplete();
    pollActiveState();
  }, 2500);
}

// Poll Active Downloads
async function pollActiveState() {
  if (isPreparing) return;

  let otterItems = [];
  try {
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/downloads`, { method: "GET" });
    if (res.ok) {
      const data = await res.json();
      otterItems = data.downloads || [];
      setEngineStatus(true);
    }
  } catch (e) {
    // Desktop engine offline
  }

  chrome.downloads.search({ state: "in_progress" }, (chromeItems) => {
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
        url: it.url || it.sanitized_url,
        estimatedEndTime: null
      });
    }
  }

  // 2. Ingest Chrome native downloads
  for (const c of chromeItems) {
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

      map.set(c.id, {
        id: c.id,
        filename: fn,
        total_bytes: c.totalBytes > 0 ? c.totalBytes : null,
        received_bytes: rx,
        speed_bps: speedBps,
        domain: domain,
        is_otter: false,
        status: c.paused ? "Paused" : "Active in Chrome",
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
    downloadsHeader.style.display = "none";
    activeList.innerHTML = "";
    activeList.style.display = "none";
    preparingState.style.display = "none";
    idleState.style.display = "flex";
    return;
  }

  idleState.style.display = "none";
  preparingState.style.display = "none";
  downloadsHeader.style.display = "flex";
  activeList.style.display = "block";
  badgeActiveCount.textContent = items.length;

  activeList.innerHTML = "";
  items.forEach(item => {
    const card = document.createElement("div");
    card.className = "dl-card";

    const pct = item.total_bytes && item.total_bytes > 0
      ? Math.min(100, Math.round((item.received_bytes / item.total_bytes) * 100))
      : 0;

    const sizeStr = item.total_bytes ? formatBytes(item.total_bytes) : formatBytes(item.received_bytes);

    // Formatted real speed
    let speedStr = "Active";
    if (item.speed_bps > 0) {
      speedStr = `${(item.speed_bps / (1024 * 1024)).toFixed(1)} MB/s`;
    } else if (item.status.includes("Paused")) {
      speedStr = "Paused";
    }

    // Accurate ETA calculation
    let etaStr = "Calculating...";
    if (item.estimatedEndTime) {
      const remSec = Math.max(0, Math.round((new Date(item.estimatedEndTime).getTime() - Date.now()) / 1000));
      etaStr = formatDuration(remSec) + " left";
    } else if (item.speed_bps > 0 && item.total_bytes && item.total_bytes > item.received_bytes) {
      const remSec = Math.round((item.total_bytes - item.received_bytes) / item.speed_bps);
      etaStr = formatDuration(remSec) + " left";
    } else {
      etaStr = item.status.includes("Paused") ? "Paused" : "In Progress";
    }

    const iconSvg = getMediaIconSvg(item.filename, item.domain);
    const ownershipBadge = item.is_otter
      ? `<span class="ownership-tag tag-otter">OTTER Multi-Stream</span>`
      : `<span class="ownership-tag tag-native">Chrome Native</span>`;

    // Accelerate button for Chrome native downloads
    const handoffButton = !item.is_otter
      ? `<button class="btn-handoff" data-id="${item.id}" title="Take over with OTTER 16-connection acceleration">⚡ Accelerate with OTTER</button>`
      : "";

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
      ${handoffButton}
    `;

    // Button actions
    card.querySelector(".btn-pause-toggle")?.addEventListener("click", () => {
      if (item.status.includes("Paused")) {
        chrome.downloads.resume(item.id);
      } else {
        chrome.downloads.pause(item.id);
      }
      setTimeout(pollActiveState, 300);
    });

    card.querySelector(".btn-cancel")?.addEventListener("click", () => {
      chrome.downloads.cancel(item.id);
      setTimeout(pollActiveState, 300);
    });

    // ⚡ Accelerate with OTTER Click Action
    card.querySelector(".btn-handoff")?.addEventListener("click", () => {
      startPreparingAnimation(() => {
        chrome.runtime.sendMessage({ action: "FORCE_HANDOFF", downloadId: item.id }, (resp) => {
          console.log("[OTTER Popup] Handoff response:", resp);
          pollActiveState();
        });
      });
    });

    activeList.appendChild(card);
  });
}

// Poll History
async function pollHistory() {
  let otterHistory = [];
  try {
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/history`, { method: "GET" });
    if (res.ok) {
      const data = await res.json();
      otterHistory = data.history || [];
    }
  } catch (e) {
    // Engine offline
  }

  if (otterHistory.length > 0) {
    renderHistoryCards(otterHistory);
  } else {
    chrome.downloads.search({ state: "complete", limit: 20 }, (chromeItems) => {
      renderHistoryFromChrome(chromeItems || []);
    });
  }
}

function renderHistoryCards(items) {
  if (!items || items.length === 0) {
    historyList.innerHTML = `<div class="empty-state">No download history.</div>`;
    return;
  }

  historyList.innerHTML = "";
  items.forEach(item => {
    const card = document.createElement("div");
    card.className = "history-card";
    const iconSvg = getMediaIconSvg(item.filename, "");

    card.innerHTML = `
      <div class="hist-icon" style="background: #FFE4E8;">
        ${iconSvg}
      </div>
      <div class="hist-info">
        <div class="hist-title" title="${escapeHtml(item.filename)}">${escapeHtml(item.filename)}</div>
        <div class="hist-meta">${formatBytes(item.total_bytes)} • ${escapeHtml(item.time_ago)}</div>
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
    historyList.innerHTML = `<div class="empty-state">No download history.</div>`;
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

function formatDuration(sec) {
  if (isNaN(sec) || sec <= 0) return "00:00:00";
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = Math.floor(sec % 60);
  return `${padZero(h)}:${padZero(m)}:${padZero(s)}`;
}

function padZero(n) {
  return n < 10 ? "0" + n : n;
}

function extractDomain(url) {
  try {
    const u = new URL(url);
    return u.hostname.replace(/^www\./, "");
  } catch (e) {
    return "Direct Download";
  }
}

function escapeHtml(str) {
  return String(str || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#039;");
}

function getMediaIconSvg(filename, domain) {
  const lower = (filename + " " + domain).toLowerCase();
  if (lower.includes("youtube") || lower.endsWith(".mp4") || lower.endsWith(".mkv") || lower.endsWith(".webm")) {
    return `<svg viewBox="0 0 24 24" width="22" height="22" fill="#E11D48"><path d="M22.54 6.42a2.78 2.78 0 0 0-1.94-2C18.88 4 12 4 12 4s-6.88 0-8.6.46a2.78 2.78 0 0 0-1.94 2A29 29 0 0 0 1 11.75a29 29 0 0 0 .46 5.33A2.78 2.78 0 0 0 3.4 19c1.72.46 8.6.46 8.6.46s6.88 0 8.6-.46a2.78 2.78 0 0 0 1.94-2 29 29 0 0 0 .46-5.25 29 29 0 0 0-.46-5.33z"/><polygon points="9.75 15.02 15.5 11.75 9.75 8.48 9.75 15.02" fill="white"/></svg>`;
  }
  if (lower.endsWith(".mp3") || lower.endsWith(".m4a") || lower.endsWith(".wav") || lower.endsWith(".flac")) {
    return `<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="#E11D48" stroke-width="2"><path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/></svg>`;
  }
  if (lower.endsWith(".zip") || lower.endsWith(".rar") || lower.endsWith(".7z") || lower.endsWith(".tar") || lower.endsWith(".gz")) {
    return `<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="#E11D48" stroke-width="2"><path d="M21 8v13H3V3h12l6 5z"/><path d="M10 3v18"/><path d="M10 7h4"/><path d="M10 11h4"/><path d="M10 15h4"/></svg>`;
  }
  return `<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="#E11D48" stroke-width="2"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>`;
}
