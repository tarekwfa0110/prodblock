// Popup script - shows current lock status
chrome.runtime.sendMessage({ type: "GET_STATE" }, (response) => {
  const dot = document.getElementById("status-dot");
  const text = document.getElementById("status-text");

  if (!response) {
    text.textContent = "Not connected";
    return;
  }

  if (response.lockActive) {
    dot.classList.add("active");
    const count = response.allowedDomains?.length || 0;
    text.textContent = count > 0 
      ? `Focus active (${count} sites allowed)`
      : "Focus active (all sites blocked)";
  } else {
    text.textContent = "Not in focus mode";
  }
});
