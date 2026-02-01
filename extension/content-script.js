/**
 * Prodblock Content Script
 * Shows full-page overlay on blocked sites when lock is active
 */
(function () {
  "use strict";

  const OVERLAY_ID = "prodblock-overlay";
  let shouldBeBlocked = false;
  let mutationObserverActive = false;

  function isAllowed(hostname, allowedDomains) {
    if (!allowedDomains || allowedDomains.length === 0) return false;
    const host = hostname.replace(/^www\./, "").toLowerCase();
    return allowedDomains.some((d) => {
      const domain = (d || "").trim().toLowerCase();
      if (!domain) return false;
      return host === domain || host.endsWith("." + domain);
    });
  }

  function getOverlay() {
    return document.getElementById(OVERLAY_ID);
  }

  function createOverlay() {
    if (getOverlay()) return;

    const overlay = document.createElement("div");
    overlay.id = OVERLAY_ID;
    const shadow = overlay.attachShadow({ mode: "closed" });

    shadow.innerHTML = `
      <style>
        :host {
          all: initial;
        }
        .overlay {
          position: fixed;
          inset: 0;
          z-index: 2147483647;
          background: linear-gradient(135deg, #0a0a0b 0%, #111113 100%);
          color: #fafafa;
          font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          text-align: center;
          padding: 2rem;
        }
        .icon {
          font-size: 4rem;
          margin-bottom: 1.5rem;
          animation: pulse 2s infinite;
        }
        @keyframes pulse {
          0%, 100% { transform: scale(1); opacity: 1; }
          50% { transform: scale(1.1); opacity: 0.8; }
        }
        .title {
          font-size: 1.75rem;
          font-weight: 600;
          margin-bottom: 0.75rem;
          background: linear-gradient(135deg, #fafafa 0%, #818cf8 100%);
          -webkit-background-clip: text;
          -webkit-text-fill-color: transparent;
        }
        .message {
          font-size: 1rem;
          color: #a1a1aa;
          max-width: 400px;
          line-height: 1.5;
        }
        .hint {
          margin-top: 2rem;
          font-size: 0.85rem;
          color: #71717a;
        }
      </style>
      <div class="overlay">
        <div class="icon">🔒</div>
        <div class="title">Site Blocked</div>
        <div class="message">This site is not in your focus session's allowed list.</div>
        <div class="hint">Complete your task in Prodblock to browse freely.</div>
      </div>
    `;

    document.documentElement.appendChild(overlay);
  }

  function removeOverlay() {
    const overlay = getOverlay();
    if (overlay) overlay.remove();
  }

  function setupMutationObserver() {
    if (mutationObserverActive) return;
    mutationObserverActive = true;

    const observer = new MutationObserver(() => {
      if (shouldBeBlocked && !getOverlay()) {
        createOverlay();
      }
    });

    observer.observe(document.documentElement, {
      childList: true,
      subtree: true
    });
  }

  function handleState(newState) {
    const hostname = window.location.hostname || "";

    if (!newState.lockActive) {
      shouldBeBlocked = false;
      removeOverlay();
      return;
    }

    const allowed = isAllowed(hostname, newState.allowedDomains || []);
    
    if (allowed) {
      shouldBeBlocked = false;
      removeOverlay();
      return;
    }

    shouldBeBlocked = true;
    createOverlay();
    setupMutationObserver();
  }

  function requestState() {
    chrome.runtime.sendMessage({ type: "GET_STATE" }, (response) => {
      if (chrome.runtime.lastError || !response) {
        setTimeout(requestState, 1000);
        return;
      }
      handleState(response);
    });
  }

  // Listen for state updates from service worker
  chrome.runtime.onMessage.addListener((msg) => {
    if (msg.type === "STATE") {
      handleState(msg);
    }
  });

  // Initial state request
  requestState();
})();
