document.addEventListener("DOMContentLoaded", () => {
  const statusBadge = document.getElementById("statusBadge");
  const indicatorDot = document.getElementById("indicatorDot");
  const statusText = document.getElementById("statusText");
  const autoHandoffToggle = document.getElementById("autoHandoffToggle");

  // Query background service worker for status
  chrome.runtime.sendMessage({ action: "GET_STATUS" }, (response) => {
    if (chrome.runtime.lastError || !response) {
      statusBadge.className = "status-badge status-offline";
      indicatorDot.className = "indicator-dot dot-offline";
      statusText.textContent = "Engine Offline";
      return;
    }

    if (response.connected) {
      statusBadge.className = "status-badge status-online";
      indicatorDot.className = "indicator-dot dot-online";
      statusText.textContent = "Engine Connected";
    } else {
      statusBadge.className = "status-badge status-offline";
      indicatorDot.className = "indicator-dot dot-offline";
      statusText.textContent = "Engine Offline";
    }

    autoHandoffToggle.checked = !!response.autoHandoff;
  });

  autoHandoffToggle.addEventListener("change", (e) => {
    chrome.runtime.sendMessage({
      action: "SET_AUTO_HANDOFF",
      enabled: e.target.checked
    });
  });
});
