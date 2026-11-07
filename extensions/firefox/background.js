// OTTER v0.1.0 — Mozilla Firefox Integration Background Script
// Manages real-time download interception and communication with OTTER desktop application.

const browserAPI = typeof browser !== "undefined" ? browser : chrome;
const OTTER_BRIDGE_URL = "http://127.0.0.1:45771";
let isBridgeConnected = false;
let autoHandoffEnabled = false;

// Load settings
browserAPI.storage.local.get(["autoHandoff"], (result) => {
  autoHandoffEnabled = !!(result && result.autoHandoff);
});

async function checkBridgeStatus() {
  try {
    const res = await fetch(`${OTTER_BRIDGE_URL}/api/browser/status`, { method: "GET" });
    if (res.ok) {
      isBridgeConnected = true;
      if (browserAPI.browserAction && browserAPI.browserAction.setBadgeText) {
        browserAPI.browserAction.setBadgeText({ text: "ON" });
        browserAPI.browserAction.setBadgeBackgroundColor({ color: "#10B981" });
      }
    } else {
      throw new Error("Bridge not OK");
    }
  } catch (err) {
    isBridgeConnected = false;
    if (browserAPI.browserAction && browserAPI.browserAction.setBadgeText) {
      browserAPI.browserAction.setBadgeText({ text: "OFF" });
      browserAPI.browserAction.setBadgeBackgroundColor({ color: "#6B7280" });
    }
  }
}

checkBridgeStatus();
setInterval(checkBridgeStatus, 30000);

function sanitizeUrl(rawUrl) {
  try {
    const parsed = new URL(rawUrl);
    const sensitive = ["token", "sig", "nonce", "expires", "auth", "key", "signature", "session"];
    for (const key of Array.from(parsed.searchParams.keys())) {
      if (sensitive.some(s => key.toLowerCase().includes(s))) {
        parsed.searchParams.set(key, "[REDACTED]");
      }
    }
    return parsed.toString();
  } catch (e) {
    return rawUrl;
  }
}

// Intercept downloads
browserAPI.downloads.onCreated.addListener(async (item) => {
  const payload = {
    id: item.id,
    url: item.url,
    final_url: item.finalUrl || item.url,
    sanitized_url: sanitizeUrl(item.url),
    filename: item.filename ? item.filename.split(/[\\/]/).pop() : "download",
    target_path: item.filename || "",
    mime_type: item.mime || "application/octet-stream",
    total_bytes: item.totalBytes > 0 ? item.totalBytes : null,
    received_bytes: item.bytesReceived || 0,
    state: item.state || "in_progress",
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
      const respData = await res.json();
      if (respData.handoff === "PAUSE_AND_TAKEOVER") {
        browserAPI.downloads.pause(item.id);
      }
    }
  } catch (e) {
    // Desktop engine offline
  }
});

browserAPI.downloads.onChanged.addListener(async (delta) => {
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
  } catch (e) {}
});

browserAPI.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg.action === "GET_STATUS") {
    sendResponse({ connected: isBridgeConnected, autoHandoff: autoHandoffEnabled });
  } else if (msg.action === "SET_AUTO_HANDOFF") {
    autoHandoffEnabled = !!msg.enabled;
    browserAPI.storage.local.set({ autoHandoff: autoHandoffEnabled });
    sendResponse({ success: true, autoHandoff: autoHandoffEnabled });
  }
});
