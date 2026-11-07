// OTTER v0.1.0 — Chrome Integration Background Service Worker
// Manages real-time download interception, capability discovery, and safe handoff to the Rust engine.

const OTTER_BRIDGE_URL = "http://127.0.0.1:45771";
let isBridgeConnected = false;
let autoHandoffEnabled = true;

// Load settings from storage (defaults to true for auto-acceleration)
chrome.storage.local.get(["autoHandoff"], (result) => {
  autoHandoffEnabled = result.autoHandoff !== undefined ? !!result.autoHandoff : true;
});

// Periodic heartbeat to verify OTTER desktop application is running
async function checkBridgeStatus() {
  try {
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/status`, { method: "GET" });
    if (res.ok) {
      isBridgeConnected = true;
      chrome.action.setBadgeText({ text: "ON" });
      chrome.action.setBadgeBackgroundColor({ color: "#10B981" }); // Emerald
      scanAndAccelerateActiveDownloads();
    } else {
      throw new Error("Bridge not OK");
    }
  } catch (err) {
    isBridgeConnected = false;
    chrome.action.setBadgeText({ text: "OFF" });
    chrome.action.setBadgeBackgroundColor({ color: "#6B7280" }); // Gray
  }
}

// Initial check and recurring alarm
checkBridgeStatus();
chrome.alarms.create("otterHeartbeat", { periodInMinutes: 0.5 });
chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === "otterHeartbeat") {
    checkBridgeStatus();
  }
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

// Check and automatically accelerate any existing in-progress downloads
async function scanAndAccelerateActiveDownloads() {
  if (!isBridgeConnected || !autoHandoffEnabled) return;
  chrome.downloads.search({ state: "in_progress" }, (items) => {
    if (items) {
      for (const item of items) {
        if (!item.paused) {
          handleBrowserDownload(item);
        }
      }
    }
  });
}

// Handler for browser downloads
async function handleBrowserDownload(item) {
  if (!item || !item.url) return;
  console.log("[OTTER Extension] Download initiated in Chrome:", item.id, item.filename, item.totalBytes);

  const payload = {
    id: item.id,
    url: item.url,
    final_url: item.finalUrl || item.url,
    sanitized_url: sanitizeUrlForAudit(item.url),
    filename: item.filename ? item.filename.split(/[\\/]/).pop() : "download",
    target_path: item.filename || "",
    mime_type: item.mime || "application/octet-stream",
    total_bytes: item.totalBytes > 0 ? item.totalBytes : null,
    received_bytes: item.bytesReceived || 0,
    state: item.state || "in_progress",
    referrer: item.referrer || "",
    start_time: item.startTime || new Date().toISOString(),
    auto_handoff_requested: autoHandoffEnabled
  };

  try {
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/download/created`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload)
    });

    if (res.ok) {
      const response = await res.json();
      console.log("[OTTER Extension] Bridge response:", response);

      // Safe Handoff Protocol:
      // If OTTER confirms eligibility and requests takeover:
      // 1. Chrome download is paused to prevent duplicate bandwidth consumption.
      // 2. OTTER Multi-Stream Engine (16 workers) takes over the transfer.
      if (response.handoff === "PAUSE_AND_TAKEOVER") {
        chrome.downloads.pause(item.id, () => {
          console.log("[OTTER Extension] Successfully paused Chrome download for OTTER handoff:", item.id);
        });
      } else if (response.handoff === "CANCEL_DUPLICATE") {
        chrome.downloads.cancel(item.id, () => {
          console.log("[OTTER Extension] Successfully canceled Chrome download; OTTER engine assumed full control:", item.id);
        });
      }
    }
  } catch (e) {
    console.warn("[OTTER Extension] Could not contact OTTER Bridge:", e.message);
  }
}

// Listen for download creation
chrome.downloads.onCreated.addListener((item) => {
  handleBrowserDownload(item);
});

// Listen for download state changes
chrome.downloads.onChanged.addListener(async (delta) => {
  const payload = {
    id: delta.id,
    state: delta.state ? delta.state.current : undefined,
    paused: delta.paused ? delta.paused.current : undefined,
    error: delta.error ? delta.error.current : undefined,
    total_bytes: delta.totalBytes ? delta.totalBytes.current : undefined,
    filename: delta.filename ? delta.filename.current.split(/[\\/]/).pop() : undefined
  };

  try {
    await fetch(`${OTTER_BRIDGE_URL}/api/browser/download/changed`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload)
    });
  } catch (e) {
    // Ignore offline errors during background sync
  }
});

// Listen for messages from popup or options page
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.action === "GET_STATUS") {
    sendResponse({ connected: isBridgeConnected, autoHandoff: autoHandoffEnabled });
  } else if (msg.action === "SET_AUTO_HANDOFF") {
    autoHandoffEnabled = !!msg.enabled;
    chrome.storage.local.set({ autoHandoff: autoHandoffEnabled });
    sendResponse({ success: true, autoHandoff: autoHandoffEnabled });
  } else if (msg.action === "FORCE_HANDOFF") {
    // Trigger manual handoff for a specific download ID
    chrome.downloads.pause(msg.downloadId, async () => {
      try {
        const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/download/handoff`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ id: msg.downloadId, force: true })
        });
        const data = await res.json();
        sendResponse({ success: true, data });
      } catch (err) {
        sendResponse({ success: false, error: err.message });
      }
    });
    return true; // async response
  }
});
