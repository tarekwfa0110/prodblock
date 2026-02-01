/**
 * Prodblock Service Worker
 * Connects to the desktop app via WebSocket and broadcasts lock state to content scripts
 */

const WS_URL = "ws://127.0.0.1:8766";
let ws = null;
let state = { lockActive: false, allowedDomains: [] };
let reconnectTimeout = null;

function connect() {
  if (reconnectTimeout) {
    clearTimeout(reconnectTimeout);
    reconnectTimeout = null;
  }

  try {
    ws = new WebSocket(WS_URL);

    ws.onopen = () => {
      console.log("[Prodblock] Connected to desktop app");
    };

    ws.onmessage = (event) => {
      try {
        const newState = JSON.parse(event.data);
        state = newState;
        broadcastState();
      } catch (e) {
        console.error("[Prodblock] Failed to parse message:", e);
      }
    };

    ws.onclose = () => {
      console.log("[Prodblock] Disconnected, will retry...");
      state = { lockActive: false, allowedDomains: [] };
      broadcastState();
      scheduleReconnect();
    };

    ws.onerror = () => {
      // Error handling - will trigger onclose
    };
  } catch (e) {
    console.error("[Prodblock] WebSocket error:", e);
    scheduleReconnect();
  }
}

function scheduleReconnect() {
  if (!reconnectTimeout) {
    reconnectTimeout = setTimeout(() => {
      reconnectTimeout = null;
      connect();
    }, 2000);
  }
}

function broadcastState() {
  chrome.tabs.query({}, (tabs) => {
    for (const tab of tabs) {
      if (tab.id) {
        chrome.tabs.sendMessage(tab.id, { type: "STATE", ...state }).catch(() => {});
      }
    }
  });
}

// Handle messages from content scripts
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.type === "GET_STATE") {
    sendResponse(state);
    return true;
  }
});

// Start connection
connect();
