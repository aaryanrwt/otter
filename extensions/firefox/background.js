// OTTER v0.1.0 — Browser Integration Background Script (Firefox)
const OTTER_BRIDGE_URL = "http://127.0.0.1:45771";
let isBridgeConnected = false;
let autoHandoffEnabled = true;

const interceptedIds = new Set();

chrome.storage.local.get(["autoHandoff"], (result) => {
  autoHandoffEnabled = result.autoHandoff !== undefined ? !!result.autoHandoff : true;
});

function updateBadge(online) {
  try {
    if (chrome.browserAction && chrome.browserAction.setBadgeText) {
      chrome.browserAction.setBadgeText({ text: online ? "ON" : "OFF" });
      chrome.browserAction.setBadgeBackgroundColor({ color: online ? "#10B981" : "#6B7280" });
    }
  } catch (e) {}
}

async function checkBridgeStatus() {
  try {
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/status`, { method: "GET" });
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

checkBridgeStatus();
try {
  chrome.alarms.create("otterHeartbeat", { periodInMinutes: 1 });
  chrome.alarms.onAlarm.addListener((alarm) => {
    if (alarm.name === "otterHeartbeat") checkBridgeStatus();
  });
} catch (e) {}

chrome.runtime.onStartup.addListener(checkBridgeStatus);
chrome.runtime.onInstalled.addListener(checkBridgeStatus);

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

async function handleBrowserDownload(item) {
  if (!item || !item.url) return;
  if (item.url.startsWith("blob:") || item.url.startsWith("data:")) return;
  if (interceptedIds.has(item.id)) return;
  interceptedIds.add(item.id);

  if (!item.paused && item.state === "in_progress") {
    chrome.downloads.pause(item.id, () => {
      if (chrome.runtime.lastError) {}
    });
  }

  setTimeout(() => {
    chrome.downloads.search({ id: item.id }, async (results) => {
      const resolvedItem = (results && results.length > 0) ? results[0] : item;
      const cleanFn = resolvedItem.filename ? resolvedItem.filename.split(/[\\/]/).pop() : "download";

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
          chrome.downloads.pause(resolvedItem.id, () => {});
        }
      } catch (e) {}
    });
  }, 200);
}

chrome.downloads.onCreated.addListener(handleBrowserDownload);

chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.action === "GET_STATUS") {
    sendResponse({ connected: isBridgeConnected, autoHandoff: autoHandoffEnabled });
  } else if (msg.action === "SET_AUTO_HANDOFF") {
    autoHandoffEnabled = !!msg.enabled;
    chrome.storage.local.set({ autoHandoff: autoHandoffEnabled });
    sendResponse({ success: true, autoHandoff: autoHandoffEnabled });
  }
  return true;
});
