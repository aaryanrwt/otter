// OTTER v0.1.0 — Browser Integration Background Service Worker
// Manages real-time download interception, capability discovery, and automatic multi-stream handoff.

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
  if (!isBridgeConnected) return;
  chrome.downloads.search({ state: "in_progress" }, (items) => {
    if (items) {
      for (const item of items) {
        if (!item.paused) {
          chrome.downloads.pause(item.id, () => {});
        }
        handleBrowserDownload(item);
      }
    }
  });
}

// Handler for browser downloads - instantly pauses single stream & hands off to OTTER 16-worker multi-stream
async function handleBrowserDownload(item) {
  if (!item || !item.url) return;
  console.log("[OTTER Extension] Download intercepted in Chrome:", item.id, item.filename, item.totalBytes);

  // Instantly pause Chrome's single connection to avoid slow 1-stream download
  if (!item.paused && item.state === "in_progress") {
    chrome.downloads.pause(item.id, () => {
      console.log("[OTTER Extension] Paused Chrome native stream:", item.id);
    });
  }

  // Fetch latest download item details from Chrome
  chrome.downloads.search({ id: item.id }, async (results) => {
    let resolvedItem = (results && results.length > 0) ? results[0] : item;

    // If finalUrl or filename is not yet populated, wait briefly and re-query
    if (!resolvedItem.finalUrl || !resolvedItem.filename || resolvedItem.filename === "download") {
      await new Promise(r => setTimeout(r, 350));
      const secondCheck = await new Promise(r => chrome.downloads.search({ id: item.id }, r));
      if (secondCheck && secondCheck.length > 0) {
        resolvedItem = secondCheck[0];
      }
    }

    const payload = {
      id: resolvedItem.id,
      url: resolvedItem.url,
      final_url: resolvedItem.finalUrl || resolvedItem.url,
      sanitized_url: sanitizeUrlForAudit(resolvedItem.url),
      filename: resolvedItem.filename ? resolvedItem.filename.split(/[\\/]/).pop() : "download",
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
        const response = await res.json();
        console.log("[OTTER Extension] OTTER multi-stream takeover initiated:", response);
        // Ensure Chrome single connection remains paused
        chrome.downloads.pause(resolvedItem.id, () => {});
      }
    } catch (e) {
      console.warn("[OTTER Extension] Could not contact OTTER Bridge:", e.message);
    }
  });
}

// Listen for download creation - immediately pause Chrome and hand off to OTTER
chrome.downloads.onCreated.addListener((item) => {
  chrome.downloads.pause(item.id, () => {});
  handleBrowserDownload(item);
});

// Listen for download state changes - ensure single stream stays paused and OTTER is updated
chrome.downloads.onChanged.addListener(async (delta) => {
  chrome.downloads.search({ id: delta.id }, (items) => {
    if (items && items.length > 0) {
      const item = items[0];
      if (item.state === "in_progress") {
        if (!item.paused) {
          chrome.downloads.pause(item.id, () => {});
        }
        handleBrowserDownload(item);
      }
    }
  });
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
