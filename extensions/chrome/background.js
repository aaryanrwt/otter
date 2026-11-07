// OTTER v0.1.0 — Browser Integration Background Service Worker
// Manages real-time download interception, capability discovery, and automatic multi-stream handoff.

const OTTER_BRIDGE_URL = "http://127.0.0.1:45771";
let isBridgeConnected = false;
let autoHandoffEnabled = true;

// Prevent duplicate processing
const interceptedIds = new Set();

// Load settings from storage safely
chrome.storage.local.get(["autoHandoff"], (result) => {
  autoHandoffEnabled = result.autoHandoff !== undefined ? !!result.autoHandoff : true;
});

// Safe badge updater
function updateBadge(online) {
  try {
    if (chrome.action && chrome.action.setBadgeText) {
      chrome.action.setBadgeText({ text: online ? "ON" : "OFF" });
      chrome.action.setBadgeBackgroundColor({ color: online ? "#10B981" : "#6B7280" });
    }
  } catch (e) {}
}

// Periodic heartbeat to verify OTTER desktop application is running
async function checkBridgeStatus() {
  try {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 2000);
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/status`, {
      method: "GET",
      signal: controller.signal
    });
    clearTimeout(timeoutId);

    if (res.ok) {
      isBridgeConnected = true;
      updateBadge(true);
      scanAndAccelerateActiveDownloads();
    } else {
      isBridgeConnected = false;
      updateBadge(false);
    }
  } catch (err) {
    isBridgeConnected = false;
    updateBadge(false);
  }
}

// Check and automatically accelerate any existing in-progress downloads
async function scanAndAccelerateActiveDownloads() {
  if (!isBridgeConnected || !autoHandoffEnabled) return;
  chrome.downloads.search({ state: "in_progress" }, (items) => {
    if (chrome.runtime.lastError || !items) return;
    for (const item of items) {
      if (!interceptedIds.has(item.id)) {
        handleBrowserDownload(item);
      }
    }
  });
}

// Initial check
checkBridgeStatus();

// Recurring alarms for heartbeat (safe 1 minute interval)
try {
  chrome.alarms.create("otterHeartbeat", { periodInMinutes: 1 });
  chrome.alarms.onAlarm.addListener((alarm) => {
    if (alarm.name === "otterHeartbeat") {
      checkBridgeStatus();
    }
  });
} catch (e) {
  console.warn("Alarm registration:", e);
}

chrome.runtime.onStartup.addListener(() => {
  checkBridgeStatus();
});

chrome.runtime.onInstalled.addListener(() => {
  checkBridgeStatus();
});

// Sanitize URL for privacy: strip session nonces, tokens, and sensitive query strings
function sanitizeUrlForAudit(rawUrl) {
  try {
    const parsed = new URL(rawUrl);
    const sensitiveKeys = ["token", "sig", "nonce", "expires", "auth", "key", "signature", "session"];
    for (const key of Array.from(parsed.searchParams.keys())) {
      if (sensitiveKeys.some(s => key.toLowerCase().includes(s))) {
        parsed.searchParams.set(key, "[REDACTED]");
      }
    }
    return parsed.toString();
  } catch (e) {
    return rawUrl;
  }
}

// Handler for browser downloads - instantly pauses Chrome single stream & hands off to OTTER
async function handleBrowserDownload(item) {
  if (!item || !item.url) return;
  if (item.url.startsWith("blob:") || item.url.startsWith("data:")) return; // In-memory blobs
  if (interceptedIds.has(item.id)) return;
  interceptedIds.add(item.id);

  console.log("[OTTER Extension] Auto-intercepting download for multi-stream acceleration:", item.id, item.filename);

  // Instantly pause Chrome's single connection to avoid bandwidth choking
  if (!item.paused && item.state === "in_progress") {
    chrome.downloads.pause(item.id, () => {
      if (chrome.runtime.lastError) {}
    });
  }

  // Allow CDN redirects to resolve (200ms) then grab fresh item details
  setTimeout(() => {
    chrome.downloads.search({ id: item.id }, async (results) => {
      const resolvedItem = (results && results.length > 0) ? results[0] : item;

      let cleanFn = "download";
      if (resolvedItem.filename) {
        cleanFn = resolvedItem.filename.split(/[\\/]/).pop();
      }

      const payload = {
        id: resolvedItem.id,
        url: resolvedItem.url,
        final_url: resolvedItem.finalUrl || resolvedItem.url,
        sanitized_url: sanitizeUrlForAudit(resolvedItem.url),
        filename: cleanFn,
        target_path: resolvedItem.filename || "",
        mime_type: resolvedItem.mime || "application/octet-stream",
        total_bytes: resolvedItem.totalBytes > 0 ? resolvedItem.totalBytes : null,
        received_bytes: resolvedItem.bytesReceived || 0,
        state: resolvedItem.state || "in_progress",
        referrer: resolvedItem.referrer || "",
        start_time: resolvedItem.startTime || new Date().toISOString(),
        auto_handoff_requested: true
      };

      try {
        const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/download/created`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload)
        });

        if (res.ok) {
          // Keep Chrome single-stream paused so only OTTER multi-stream downloads
          chrome.downloads.pause(resolvedItem.id, () => {
            if (chrome.runtime.lastError) {}
          });
        }
      } catch (e) {
        console.warn("[OTTER Extension] Could not contact OTTER Bridge:", e.message);
      }
    });
  }, 200);
}

// Listen for download creation
chrome.downloads.onCreated.addListener((item) => {
  handleBrowserDownload(item);
});

// Listen for download state changes
chrome.downloads.onChanged.addListener((delta) => {
  // If download just acquired filename and wasn't intercepted yet
  if (delta.filename && delta.filename.current && !interceptedIds.has(delta.id)) {
    chrome.downloads.search({ id: delta.id }, (items) => {
      if (items && items.length > 0) {
        handleBrowserDownload(items[0]);
      }
    });
  }

  // Terminal state transition (complete or interrupted)
  if (delta.state && (delta.state.current === "complete" || delta.state.current === "interrupted")) {
    fetch(`${OTTER_BRIDGE_URL}/api/browser/download/changed`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        id: delta.id,
        state: delta.state.current,
        error: delta.error ? delta.error.current : undefined
      })
    }).catch(() => {});
  }
});

// Listen for messages from popup or options page safely
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  try {
    if (msg.action === "GET_STATUS") {
      sendResponse({ connected: isBridgeConnected, autoHandoff: autoHandoffEnabled });
    } else if (msg.action === "SET_AUTO_HANDOFF") {
      autoHandoffEnabled = !!msg.enabled;
      chrome.storage.local.set({ autoHandoff: autoHandoffEnabled });
      sendResponse({ success: true, autoHandoff: autoHandoffEnabled });
    } else if (msg.action === "FORCE_HANDOFF") {
      if (msg.downloadId) {
        chrome.downloads.search({ id: msg.downloadId }, (items) => {
          if (items && items.length > 0) {
            handleBrowserDownload(items[0]);
          }
        });
      }
      sendResponse({ success: true });
    }
  } catch (e) {
    sendResponse({ error: e.message });
  }
  return true; // Keep response channel open for async
});
