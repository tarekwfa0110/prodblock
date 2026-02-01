// Prodblock Frontend - Main JavaScript
const { invoke } = window.__TAURI__.core;

// DOM helpers
const $ = (sel, el = document) => el.querySelector(sel);
const $$ = (sel, el = document) => el.querySelectorAll(sel);

// State
let activities = [];
let suggested = [];
let selectedActivity = null;
let lockEndTime = null;
let lockTimerInterval = null;

// ============================================================================
// SCREEN NAVIGATION
// ============================================================================

function showScreen(id) {
  $$(".screen").forEach((s) => s.classList.remove("active"));
  const screen = $(`#${id}`);
  if (screen) screen.classList.add("active");
}

// ============================================================================
// UTILITIES
// ============================================================================

function uuid() {
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    const v = c === "x" ? r : (r & 0x3) | 0x8;
    return v.toString(16);
  });
}

function formatDuration(mins) {
  if (mins < 60) return `${mins} min`;
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

// ============================================================================
// DATA LOADING
// ============================================================================

async function loadActivities() {
  try {
    activities = await invoke("get_activities");
    return activities;
  } catch (e) {
    console.error("Failed to load activities:", e);
    activities = [];
    return [];
  }
}

async function loadSuggested() {
  try {
    suggested = await invoke("get_suggested_three");
    return suggested;
  } catch (e) {
    console.error("Failed to load suggestions:", e);
    suggested = [];
    return [];
  }
}

// ============================================================================
// CHOICE SCREEN
// ============================================================================

function renderChoiceScreen() {
  const container = $("#choice-choices");
  const emptyState = $("#choice-empty");
  const confirmBtn = $("#choice-confirm");

  if (!container) return;

  container.innerHTML = "";

  if (suggested.length === 0) {
    container.style.display = "none";
    emptyState.style.display = "block";
    confirmBtn.style.display = "none";
    return;
  }

  container.style.display = "flex";
  emptyState.style.display = "none";
  confirmBtn.style.display = "block";

  suggested.forEach((a) => {
    const div = document.createElement("div");
    div.className = "choice" + (selectedActivity?.id === a.id ? " selected" : "");
    div.dataset.id = a.id;

    const hasApps = a.allowed_apps && a.allowed_apps.length > 0;
    const hasDomains = a.allowed_domains && a.allowed_domains.length > 0;
    let meta = `${a.minimum_lock_minutes || 10} min`;
    if (!hasApps && !hasDomains) {
      meta += " • Full focus (no apps/sites)";
    } else {
      if (hasApps) meta += ` • ${a.allowed_apps.length} app${a.allowed_apps.length > 1 ? 's' : ''}`;
      if (hasDomains) meta += ` • ${a.allowed_domains.length} site${a.allowed_domains.length > 1 ? 's' : ''}`;
    }

    div.innerHTML = `
      <div class="choice-name">${escapeHtml(a.name)}</div>
      <div class="choice-meta">${meta}</div>
    `;

    div.addEventListener("click", () => {
      selectedActivity = suggested.find((x) => x.id === div.dataset.id);
      renderChoiceScreen();
    });

    container.appendChild(div);
  });

  confirmBtn.disabled = !selectedActivity;
}

function escapeHtml(text) {
  const div = document.createElement("div");
  div.textContent = text;
  return div.innerHTML;
}

async function goToChoice() {
  selectedActivity = null;
  await loadSuggested();
  renderChoiceScreen();
  showScreen("choice");
}

// ============================================================================
// CONFIRMATION SCREEN
// ============================================================================

function showConfirmation() {
  if (!selectedActivity) return;
  
  $("#confirmation-name").textContent = selectedActivity.name;
  
  const hasApps = selectedActivity.allowed_apps?.length > 0;
  const hasDomains = selectedActivity.allowed_domains?.length > 0;
  let details = `${selectedActivity.minimum_lock_minutes || 10} minutes minimum`;
  
  if (!hasApps && !hasDomains) {
    details += " • All distractions blocked";
  }
  
  $("#confirmation-details").textContent = details;
  showScreen("confirmation");
}

// ============================================================================
// LOCK SCREEN
// ============================================================================

async function startLock() {
  if (!selectedActivity) return;

  const lockMinutes = selectedActivity.minimum_lock_minutes || 10;
  
  try {
    await invoke("start_lock", {
      activityId: selectedActivity.id,
      whitelist: selectedActivity.allowed_apps || [],
      allowedDomains: selectedActivity.allowed_domains || [],
      minimumLockMinutes: lockMinutes,
    });
  } catch (e) {
    console.error("Failed to start lock:", e);
    alert("Failed to start focus session: " + e);
    return;
  }

  lockEndTime = Date.now() + lockMinutes * 60 * 1000;
  $("#lock-activity-name").textContent = selectedActivity.name;
  showScreen("lock");
  startLockTimer();
}

function startLockTimer() {
  const timerEl = $("#lock-timer");
  const doneBtn = $("#lock-done");

  if (lockTimerInterval) clearInterval(lockTimerInterval);

  const updateTimer = () => {
    const remaining = Math.max(0, lockEndTime - Date.now());
    const mins = Math.floor(remaining / 60000);
    const secs = Math.floor((remaining % 60000) / 1000);
    timerEl.textContent = `${mins}:${secs.toString().padStart(2, "0")}`;

    const canFinish = remaining <= 0;
    doneBtn.disabled = !canFinish;

    if (canFinish) {
      clearInterval(lockTimerInterval);
      lockTimerInterval = null;
    }
  };

  updateTimer();
  lockTimerInterval = setInterval(updateTimer, 500);
}

async function endLock() {
  try {
    await invoke("end_lock");
  } catch (e) {
    console.error("Failed to end lock:", e);
  }

  if (lockTimerInterval) {
    clearInterval(lockTimerInterval);
    lockTimerInterval = null;
  }

  await goToChoice();
}

// ============================================================================
// CONFIG SCREEN
// ============================================================================

function renderConfigList() {
  const list = $("#config-activity-list");
  if (!list) return;

  list.innerHTML = "";

  if (activities.length === 0) {
    list.innerHTML = '<li style="justify-content: center; color: var(--text-muted);">No activities yet</li>';
    return;
  }

  activities.forEach((a) => {
    const li = document.createElement("li");
    li.innerHTML = `
      <div class="activity-info">
        <span class="activity-title">${escapeHtml(a.name)}</span>
        <span class="activity-time">${a.typical_time || "Any time"} • ${a.minimum_lock_minutes || 10} min</span>
      </div>
      <div class="activity-actions">
        <button data-id="${a.id}" class="edit">Edit</button>
        <button data-id="${a.id}" class="delete">Delete</button>
      </div>
    `;
    list.appendChild(li);
  });

  list.querySelectorAll(".edit").forEach((btn) =>
    btn.addEventListener("click", () => openActivityForm(btn.dataset.id))
  );

  list.querySelectorAll(".delete").forEach((btn) =>
    btn.addEventListener("click", () => deleteActivity(btn.dataset.id))
  );
}

async function loadRunAtStartup() {
  try {
    const enabled = await invoke("get_run_at_startup");
    const cb = $("#config-run-at-startup");
    if (cb) cb.checked = enabled;
  } catch (e) {
    console.error("Failed to get run at startup:", e);
  }
}

async function toggleRunAtStartup(enabled) {
  try {
    await invoke("set_run_at_startup", { enabled });
  } catch (e) {
    console.error("Failed to set run at startup:", e);
  }
}

// ============================================================================
// CONFIG FORM
// ============================================================================

function openActivityForm(id) {
  const a = id ? activities.find((x) => x.id === id) : null;

  $("#form-title").textContent = a ? "Edit Activity" : "Add Activity";
  $("#form-id").value = a?.id || "";
  $("#form-name").value = a?.name || "";
  $("#form-time").value = a?.typical_time || "";
  $("#form-min-lock").value = a?.minimum_lock_minutes || 25;
  $("#form-apps").value = (a?.allowed_apps || []).join("\n");
  $("#form-domains").value = (a?.allowed_domains || []).join("\n");

  showScreen("config-form");
}

async function saveActivity(ev) {
  ev.preventDefault();

  const id = $("#form-id").value || uuid();
  const name = $("#form-name").value.trim();
  const typical_time = $("#form-time").value.trim() || "00:00";
  const minimum_lock_minutes = parseInt($("#form-min-lock").value, 10) || 25;
  const allowed_apps = $("#form-apps").value
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
  const allowed_domains = $("#form-domains").value
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);

  if (!name) {
    alert("Please enter an activity name");
    return;
  }

  const activity = {
    id,
    name,
    typical_time,
    duration_minutes: 0,
    minimum_lock_minutes,
    allowed_apps,
    allowed_domains,
  };

  const idx = activities.findIndex((x) => x.id === id);
  if (idx >= 0) {
    activities[idx] = activity;
  } else {
    activities.push(activity);
  }

  try {
    await invoke("save_activities", { activities });
  } catch (e) {
    console.error("Failed to save activities:", e);
    alert("Failed to save: " + e);
    return;
  }

  renderConfigList();
  showScreen("config");
}

async function deleteActivity(id) {
  if (!confirm("Delete this activity?")) return;

  activities = activities.filter((x) => x.id !== id);

  try {
    await invoke("save_activities", { activities });
  } catch (e) {
    console.error("Failed to delete activity:", e);
  }

  renderConfigList();
}

// ============================================================================
// INITIALIZATION
// ============================================================================

async function init() {
  await loadActivities();

  // Choice screen
  $("#choice-confirm")?.addEventListener("click", showConfirmation);
  $("#choice-go-config")?.addEventListener("click", () => {
    renderConfigList();
    loadRunAtStartup();
    showScreen("config");
  });

  // Confirmation screen
  $("#confirmation-back")?.addEventListener("click", () => goToChoice());
  $("#confirmation-start")?.addEventListener("click", startLock);

  // Lock screen
  $("#lock-done")?.addEventListener("click", endLock);
  
  // Dev-only escape hatch
  const testingBtn = $("#lock-exit-testing");
  if (testingBtn) {
    // Show only in dev mode (check if localhost or dev env)
    const isDev = window.location.hostname === "localhost" || window.location.hostname === "127.0.0.1" || window.location.hostname === "tauri.localhost";
    testingBtn.style.display = isDev ? "block" : "none";
    testingBtn.addEventListener("click", endLock);
  }

  // Keyboard escape for dev
  document.addEventListener("keydown", (e) => {
    if (e.ctrlKey && e.shiftKey && e.key === "E" && $("#lock")?.classList.contains("active")) {
      e.preventDefault();
      endLock();
    }
  });

  // Config screen
  $("#nav-config")?.addEventListener("click", async () => {
    await loadActivities();
    renderConfigList();
    loadRunAtStartup();
    showScreen("config");
  });

  $("#config-add")?.addEventListener("click", () => openActivityForm(null));
  $("#nav-choice")?.addEventListener("click", goToChoice);
  
  $("#config-run-at-startup")?.addEventListener("change", (ev) => {
    toggleRunAtStartup(ev.target.checked);
  });

  // Config form
  $("#activity-form")?.addEventListener("submit", saveActivity);
  $("#form-cancel")?.addEventListener("click", () => {
    renderConfigList();
    showScreen("config");
  });

  // Start on choice screen
  await goToChoice();
}

document.addEventListener("DOMContentLoaded", init);
