const { invoke } = window.__TAURI__.core;
const { getCurrentWindow, LogicalSize } = window.__TAURI__.window;

const appWindow = getCurrentWindow();

let lastSnapshot = null;
let showRemaining = true;
let isCompact = false;
let isPinned = true;
let isCheckingSession = false;

let manifest = { defaultLocale: "en-US", locales: [] };
let currentLocale = "en-US";
let dict = {};

const widgetEl = document.getElementById("widget");
const titlebarEl = document.getElementById("titlebar");
const edgeDockView = document.getElementById("edgeDockView");
const edgeRows = document.getElementById("edgeRows");
const edgeStatusDot = document.getElementById("edgeStatusDot");
const edgeRow1Label = document.getElementById("edgeRow1Label");
const edgeRow2Label = document.getElementById("edgeRow2Label");
const edgeFlashPct = document.getElementById("edgeFlashPct");
const edgeFlashBar = document.getElementById("edgeFlashBar");
const edgeArgonPct = document.getElementById("edgeArgonPct");
const edgeArgonBar = document.getElementById("edgeArgonBar");

let edgeWindowMode = "5h"; // Alternates calmly between "5h" and "weekly"
const statusDot = document.getElementById("statusDot");
const tierBadge = document.getElementById("tierBadge");
const summaryList = document.getElementById("summaryList");
const modelsList = document.getElementById("modelsList");
const compactView = document.getElementById("compactView");
const compactPills = document.getElementById("compactPills");
const fullView = document.getElementById("fullView");
const loginView = document.getElementById("loginView");
const loginTitle = document.getElementById("loginTitle");
const loginDesc = document.getElementById("loginDesc");
const authedContent = document.getElementById("authedContent");
const btnGoogleLogin = document.getElementById("btnGoogleLogin");
const loginBtnText = document.getElementById("loginBtnText");
const btnSwitchAccount = document.getElementById("btnSwitchAccount");
const errorBanner = document.getElementById("errorBanner");
const userEmail = document.getElementById("userEmail");
const sessionsCount = document.getElementById("sessionsCount");
const updatedAt = document.getElementById("updatedAt");
const modeLabel = document.getElementById("modeLabel");
const btnRefresh = document.getElementById("btnRefresh");
const btnPin = document.getElementById("btnPin");
const btnCompact = document.getElementById("btnCompact");
const btnClose = document.getElementById("btnClose");
const btnToggleMode = document.getElementById("btnToggleMode");
const opacitySlider = document.getElementById("opacitySlider");
const langSelect = document.getElementById("langSelect");

function t(keyPath, vars = {}) {
  const parts = keyPath.split(".");
  let cur = dict;
  for (const p of parts) {
    if (cur && typeof cur === "object" && p in cur) {
      cur = cur[p];
    } else {
      return keyPath;
    }
  }
  if (typeof cur !== "string") return keyPath;
  return cur.replace(/\{(\w+)\}/g, (_, k) => (k in vars ? String(vars[k]) : `{${k}}`));
}

function resolveInitialLocale(availableCodes, fallback) {
  const saved = localStorage.getItem("agm-lang");
  if (saved && availableCodes.includes(saved)) {
    return saved;
  }
  const nav = (navigator.language || "").trim();
  if (availableCodes.includes(nav)) {
    return nav;
  }
  const base = nav.split("-")[0].toLowerCase();
  const match = availableCodes.find((c) => c.toLowerCase() === base || c.toLowerCase().startsWith(base + "-"));
  return match || fallback;
}

async function initI18n() {
  const res = await fetch("locales/manifest.json");
  manifest = await res.json();
  const codes = manifest.locales.map((l) => l.code);
  currentLocale = resolveInitialLocale(codes, manifest.defaultLocale);

  langSelect.innerHTML = manifest.locales
    .map((l) => `<option value="${l.code}">${l.code.toUpperCase()}</option>`)
    .join("");
  langSelect.value = currentLocale;

  await loadLocale(currentLocale);
}

async function loadLocale(code) {
  const res = await fetch(`locales/${code}.json`);
  dict = await res.json();
  currentLocale = code;
  localStorage.setItem("agm-lang", code);
  document.documentElement.lang = code;

  document.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    el.textContent = t(key);
  });

  document.querySelectorAll("[data-i18n-title]").forEach((el) => {
    const key = el.getAttribute("data-i18n-title");
    el.title = t(key);
  });

  modeLabel.textContent = showRemaining ? t("mode.remaining") : t("mode.used");

  try {
    await invoke("set_tray_labels", {
      toggle: t("tray.toggle"),
      top: t("tray.top"),
      quit: t("tray.quit"),
    });
  } catch (_) {}

  if (lastSnapshot) {
    renderSnapshot(lastSnapshot);
  }
}

function colorClassForRemaining(remFraction) {
  if (remFraction <= 0.2) return "fill-danger";
  if (remFraction <= 0.5) return "fill-warn";
  return "fill-good";
}

function textColorForRemaining(remFraction) {
  if (remFraction <= 0.2) return "#fb7185";
  if (remFraction <= 0.5) return "#fbbf24";
  return "#34d399";
}

function formatPct(remPct, usedPct) {
  const val = showRemaining ? remPct : usedPct;
  return `${val.toFixed(1).replace(".0", "")}%`;
}

function formatResetSeconds(secs) {
  if (secs === null || secs === undefined) {
    return t("time.none");
  }
  if (secs <= 0) {
    return t("time.now");
  }
  const days = Math.floor(secs / 86400);
  const hours = Math.floor((secs % 86400) / 3600);
  const mins = Math.floor((secs % 3600) / 60);

  if (days > 0) {
    return t("time.daysHours", { d: days, h: hours });
  }
  if (hours > 0) {
    return t("time.hoursMins", { h: hours, m: mins });
  }
  if (mins > 0) {
    return t("time.mins", { m: mins });
  }
  return t("time.secs", { s: secs });
}

function formatError(errKey) {
  const raw = String(errKey);
  if (dict.errors && raw in dict.errors) {
    return t(`errors.${raw}`);
  }
  return t("errors.UNKNOWN", { error: raw });
}

function renderCard(title, subtitle, remFraction, remPct, usedPct, resetSeconds) {
  const barWidth = showRemaining ? remPct : usedPct;
  const fillClass = colorClassForRemaining(remFraction);
  const textColor = textColorForRemaining(remFraction);
  const resetIn = formatResetSeconds(resetSeconds);

  return `
    <div class="quota-card">
      <div class="card-top">
        <div>
          <div class="card-title">${title}</div>
          ${subtitle ? `<div class="card-sub">${subtitle}</div>` : ""}
        </div>
        <div class="card-stats">
          <span class="reset-badge" title="${t("tooltips.resetTimer")}">⏱ ${resetIn}</span>
          <span class="pct-badge" style="color: ${textColor}">${formatPct(remPct, usedPct)}</span>
        </div>
      </div>
      <div class="progress-track">
        <div class="progress-fill ${fillClass}" style="width: ${Math.max(2, barWidth)}%"></div>
      </div>
    </div>
  `;
}

function resolveGroupTitle(bucket) {
  if (bucket.groupKey === "gemini") return t("groups.gemini");
  if (bucket.groupKey === "claude_gpt") return t("groups.claude_gpt");
  return bucket.groupName || t("groups.other");
}

function resolveWindowTitle(bucket) {
  if (bucket.window === "5h") return t("windows.5h");
  if (bucket.window === "weekly") return t("windows.weekly");
  return bucket.displayName;
}

function findEdgeModelStat(snap, familyKey, idHint, windowMode) {
  if (!snap || !snap.authenticated) return null;
  const buckets = snap.buckets || [];

  if (windowMode === "weekly") {
    const weeklyBucket =
      buckets.find((b) => b.id === "gemini-weekly") ||
      buckets.find((b) => b.groupKey === "gemini" && b.window === "weekly") ||
      buckets.find((b) => b.window === "weekly");
    if (weeklyBucket) return weeklyBucket;
  } else {
    const fiveHourBucket =
      buckets.find((b) => b.id === "gemini-5h") ||
      buckets.find((b) => b.groupKey === "gemini" && b.window === "5h");
    const models = snap.models || [];
    const matches = models.filter(
      (m) => m.familyKey === familyKey || (idHint && m.id.toLowerCase().includes(idHint))
    );
    if (matches.length > 0) {
      const modelStat = matches.reduce((min, cur) =>
        cur.remainingFraction < min.remainingFraction ? cur : min
      );
      // If model quota is within the 5h window (<= 5.5 hours) or no 5h bucket exists, prefer modelStat
      if (!fiveHourBucket || (typeof modelStat.resetSeconds === "number" && modelStat.resetSeconds <= 19800)) {
        return modelStat;
      }
    }
    if (fiveHourBucket) return fiveHourBucket;
  }

  const models = snap.models || [];
  const matches = models.filter(
    (m) => m.familyKey === familyKey || (idHint && m.id.toLowerCase().includes(idHint))
  );
  if (matches.length > 0) {
    return matches.reduce((min, cur) =>
      cur.remainingFraction < min.remainingFraction ? cur : min
    );
  }
  return buckets.find((b) => b.groupKey === "gemini") || null;
}

function applyEdgeRowStat(pctEl, barEl, stat) {
  if (!stat) {
    pctEl.textContent = "--%";
    pctEl.style.color = "var(--text-muted)";
    barEl.className = "edge-bar-fill fill-good";
    barEl.style.width = "2%";
    return;
  }
  const barWidth = showRemaining ? stat.remainingPercent : stat.usedPercent;
  const fillClass = colorClassForRemaining(stat.remainingFraction);
  const textColor = textColorForRemaining(stat.remainingFraction);
  pctEl.textContent = formatPct(stat.remainingPercent, stat.usedPercent);
  pctEl.style.color = textColor;
  barEl.className = `edge-bar-fill ${fillClass}`;
  barEl.style.width = `${Math.max(3, barWidth)}%`;
}

function updateEdgeDockPill(snap) {
  const isWeekly = edgeWindowMode === "weekly";
  if (edgeRow1Label) {
    edgeRow1Label.textContent = isWeekly ? "Flash Wk" : "Flash 5h";
  }
  if (edgeRow2Label) {
    edgeRow2Label.textContent = isWeekly ? "Argon Wk" : "Argon 5h";
  }
  const flashStat = findEdgeModelStat(snap, "gemini_3_8_flash", "flash", edgeWindowMode);
  const argonStat = findEdgeModelStat(snap, "gemini_4_argon", "argon", edgeWindowMode);
  applyEdgeRowStat(edgeFlashPct, edgeFlashBar, flashStat);
  applyEdgeRowStat(edgeArgonPct, edgeArgonBar, argonStat);
}

window.__onDockStateChanged = function (collapsed, dockEdge, taskbarColor) {
  if (dockEdge) {
    widgetEl.setAttribute("data-dock", dockEdge);
  }
  if (taskbarColor) {
    document.documentElement.style.setProperty("--taskbar-bg", taskbarColor);
  }
  if (collapsed) {
    widgetEl.classList.add("dock-collapsed");
    edgeDockView.classList.remove("hidden");
    titlebarEl.classList.add("hidden");
    fullView.classList.add("hidden");
    compactView.classList.add("hidden");
  } else {
    widgetEl.classList.remove("dock-collapsed");
    edgeDockView.classList.add("hidden");
    titlebarEl.classList.remove("hidden");
    compactView.classList.add("hidden");
    fullView.classList.remove("hidden");
  }
};

window.addEventListener("resize", () => {
  const shouldBeCollapsed = window.innerWidth < 200;
  const isCurrentlyCollapsed = widgetEl.classList.contains("dock-collapsed");
  if (shouldBeCollapsed !== isCurrentlyCollapsed) {
    window.__onDockStateChanged(shouldBeCollapsed);
  }
});

function renderSnapshot(snap) {
  if (!snap) return;

  updateEdgeDockPill(snap);

  if (snap.authenticated) {
    tierBadge.textContent = snap.paidTierName || snap.tierName || t("brandTitle");
    userEmail.textContent = snap.email;
    btnSwitchAccount.title = t("tooltips.accountConnected", {
      source: snap.authSource || t("brandTitle"),
    });
  } else {
    tierBadge.textContent = snap.installed ? t("status.notSignedIn") : t("status.missingInstall");
    userEmail.textContent = t("status.notConnected");
    btnSwitchAccount.title = t("tooltips.accountDisconnected");
  }

  sessionsCount.textContent = t("status.activeSessions", { count: snap.activeSessions });
  updatedAt.textContent = snap.updatedAt;

  if (!snap.authenticated) {
    authedContent.classList.add("hidden");
    loginView.classList.remove("hidden");
    if (!snap.installed) {
      loginTitle.textContent = t("onboarding.missingTitle");
      loginDesc.textContent = t("onboarding.missingDesc");
      loginBtnText.textContent = isCheckingSession
        ? t("status.checkingSession")
        : t("onboarding.missingAction");
    } else {
      loginTitle.textContent = t("onboarding.loginTitle");
      loginDesc.textContent = t("onboarding.loginDesc");
      loginBtnText.textContent = isCheckingSession
        ? t("status.checkingSession")
        : t("onboarding.loginAction");
    }
    return;
  }

  loginView.classList.add("hidden");
  authedContent.classList.remove("hidden");

  const infoEl = document.getElementById("infoSessionDetails");
  if (infoEl) {
    infoEl.innerHTML = `
      <div><strong>${t("info.sourceLabel")}</strong> ${snap.authSource || "—"}</div>
      <div><strong>${t("info.accountLabel")}</strong> ${snap.email || "—"}</div>
      <div><strong>${t("info.projectLabel")}</strong> ${snap.projectId || "—"}</div>
    `;
  }

  // 1. Hero Quota Groups: Side-by-side 5h & Weekly tiles per family group
  let summaryHtml = "";
  if (snap.buckets && snap.buckets.length > 0) {
    const groupsMap = new Map();
    for (const b of snap.buckets) {
      const grpLabel = resolveGroupTitle(b);
      if (!groupsMap.has(grpLabel)) {
        groupsMap.set(grpLabel, []);
      }
      groupsMap.get(grpLabel).push(b);
    }

    for (const [grpLabel, groupBuckets] of groupsMap.entries()) {
      const tilesHtml = groupBuckets
        .map((b) => {
          const barWidth = showRemaining ? b.remainingPercent : b.usedPercent;
          const fillClass = colorClassForRemaining(b.remainingFraction);
          const textColor = textColorForRemaining(b.remainingFraction);
          const resetIn = formatResetSeconds(b.resetSeconds);
          const winShort = b.window === "5h" ? "5h" : b.window === "weekly" ? "7d" : b.displayName;
          const winFull = resolveWindowTitle(b);
          return `
            <div class="window-tile" title="${winFull}">
              <div class="window-tile-top">
                <span class="window-tag ${b.window === "weekly" ? "tag-weekly" : "tag-5h"}">${winShort}</span>
                <span class="reset-badge" title="${t("tooltips.resetTimer")}">⏱ ${resetIn}</span>
              </div>
              <div class="window-pct" style="color: ${textColor}">${formatPct(b.remainingPercent, b.usedPercent)}</div>
              <div class="progress-track">
                <div class="progress-fill ${fillClass}" style="width: ${Math.max(2, barWidth)}%"></div>
              </div>
            </div>
          `;
        })
        .join("");

      summaryHtml += `
        <div class="group-hero-card">
          <div class="group-hero-header">
            <span class="group-hero-title">${grpLabel}</span>
          </div>
          <div class="window-tiles">
            ${tilesHtml}
          </div>
        </div>
      `;
    }
  }

  summaryList.innerHTML = summaryHtml || `<div class="skeleton">${t("status.noQuotas")}</div>`;

  // 2. Unified Per-Model Quotas Grid (directly visible below Hero Groups)
  const modelsArr = snap.models || [];
  const modelsCountBadge = document.getElementById("modelsCountBadge");
  if (modelsCountBadge) {
    modelsCountBadge.textContent = modelsArr.length > 0 ? String(modelsArr.length) : "";
  }

  let modelsHtml = "";
  for (const m of modelsArr) {
    const famLabel = t(`families.${m.familyKey}`);
    const barWidth = showRemaining ? m.remainingPercent : m.usedPercent;
    const fillClass = colorClassForRemaining(m.remainingFraction);
    const textColor = textColorForRemaining(m.remainingFraction);
    const resetIn = formatResetSeconds(m.resetSeconds);

    modelsHtml += `
      <div class="model-tile">
        <div class="model-tile-top">
          <div class="model-name" title="${m.displayName}">${m.displayName}</div>
          <span class="model-pct" style="color: ${textColor}">${formatPct(m.remainingPercent, m.usedPercent)}</span>
        </div>
        <div class="model-tile-sub">
          <span class="model-fam">${famLabel}</span>
          <span class="model-reset" title="${t("tooltips.resetTimer")}">⏱ ${resetIn}</span>
        </div>
        <div class="progress-track" style="height: 4px">
          <div class="progress-fill ${fillClass}" style="width: ${Math.max(2, barWidth)}%"></div>
        </div>
      </div>
    `;
  }
  modelsList.innerHTML = modelsHtml || `<div class="skeleton">${t("status.noModels")}</div>`;

  // 3. Compact Mini-Pills view
  const miniMap = [
    { id: "gemini-5h", labelKey: "mini.gemini_5h" },
    { id: "gemini-weekly", labelKey: "mini.gemini_weekly" },
    { id: "3p-5h", labelKey: "mini.claude_5h" },
    { id: "3p-weekly", labelKey: "mini.claude_weekly" },
  ];

  const miniItems = [];
  for (const m of miniMap) {
    const found = (snap.buckets || []).find((b) => b.id === m.id);
    if (found) {
      miniItems.push({ shortLabel: t(m.labelKey), ...found });
    }
  }

  compactPills.innerHTML = miniItems
    .map((item) => {
      const barWidth = showRemaining ? item.remainingPercent : item.usedPercent;
      const fillClass = colorClassForRemaining(item.remainingFraction);
      const textColor = textColorForRemaining(item.remainingFraction);
      const grpLabel = resolveGroupTitle(item);
      const winLabel = resolveWindowTitle(item);
      const resetIn = formatResetSeconds(item.resetSeconds);
      return `
        <div class="mini-pill" title="${grpLabel} - ${winLabel} (${resetIn})">
          <div class="mini-top">
            <span class="mini-name">${item.shortLabel}</span>
            <span style="color:${textColor}">${formatPct(item.remainingPercent, item.usedPercent)}</span>
          </div>
          <div class="progress-track" style="height:4px">
            <div class="progress-fill ${fillClass}" style="width:${Math.max(2, barWidth)}%"></div>
          </div>
        </div>
      `;
    })
    .join("");
}

async function refreshUsage() {
  statusDot.className = "status-dot loading";
  if (edgeStatusDot) edgeStatusDot.className = "status-dot loading";
  btnRefresh.classList.add("spinning");
  errorBanner.classList.add("hidden");

  try {
    const snap = await invoke("get_usage");
    lastSnapshot = snap;
    renderSnapshot(snap);
    statusDot.className = "status-dot";
    if (edgeStatusDot) edgeStatusDot.className = "status-dot";
  } catch (err) {
    statusDot.className = "status-dot error";
    if (edgeStatusDot) edgeStatusDot.className = "status-dot error";
    errorBanner.textContent = formatError(err);
    errorBanner.classList.remove("hidden");
  } finally {
    btnRefresh.classList.remove("spinning");
  }
}

async function startGoogleLogin() {
  statusDot.className = "status-dot loading";
  if (edgeStatusDot) edgeStatusDot.className = "status-dot loading";
  errorBanner.classList.add("hidden");
  btnGoogleLogin.disabled = true;
  isCheckingSession = true;
  loginBtnText.textContent = t("status.checkingSession");

  try {
    const snap = await invoke("launch_antigravity");
    lastSnapshot = snap;
    renderSnapshot(snap);
    statusDot.className = "status-dot";
    if (edgeStatusDot) edgeStatusDot.className = "status-dot";
  } catch (err) {
    statusDot.className = "status-dot error";
    if (edgeStatusDot) edgeStatusDot.className = "status-dot error";
    errorBanner.textContent = formatError(err);
    errorBanner.classList.remove("hidden");
  } finally {
    isCheckingSession = false;
    btnGoogleLogin.disabled = false;
    if (lastSnapshot) {
      renderSnapshot(lastSnapshot);
    }
  }
}

// Event Listeners
titlebarEl.addEventListener("mousedown", (e) => {
  if (e.button === 0 && !e.target.closest(".controls")) {
    invoke("start_window_drag");
  }
});

edgeDockView.addEventListener("mousedown", (e) => {
  if (e.button === 0) {
    invoke("start_window_drag");
  }
});

compactView.addEventListener("mousedown", (e) => {
  if (e.button === 0) {
    invoke("start_window_drag");
  }
});

btnRefresh.addEventListener("click", () => refreshUsage());
btnGoogleLogin.addEventListener("click", () => startGoogleLogin());
btnSwitchAccount.addEventListener("click", () => startGoogleLogin());

langSelect.addEventListener("change", (e) => {
  loadLocale(e.target.value);
});

btnToggleMode.addEventListener("click", () => {
  showRemaining = !showRemaining;
  modeLabel.textContent = showRemaining ? t("mode.remaining") : t("mode.used");
  invoke("set_display_mode", { showRemaining }).catch(() => {});
  if (lastSnapshot) renderSnapshot(lastSnapshot);
});

if (btnPin) {
  btnPin.addEventListener("click", async () => {
    isPinned = !isPinned;
    await invoke("set_flyout_pinned", { pinned: isPinned });
    btnPin.classList.toggle("active", isPinned);
  });
}

if (btnCompact) {
  btnCompact.addEventListener("click", async () => {
    isPinned = false;
    if (btnPin) btnPin.classList.remove("active");
    await invoke("collapse_to_edge");
  });
}

if (btnClose) {
  btnClose.addEventListener("click", async () => {
    isPinned = false;
    if (btnPin) btnPin.classList.remove("active");
    await invoke("collapse_to_edge");
  });
}

if (opacitySlider) {
  opacitySlider.addEventListener("input", (e) => {
    const pct = Number(e.target.value);
    invoke("set_window_opacity", { percent: pct });
  });
}

const btnInfoToggle = document.getElementById("btnInfoToggle");
const dashboardView = document.getElementById("dashboardView");
const tabInfo = document.getElementById("tabInfo");
if (btnInfoToggle && dashboardView && tabInfo) {
  btnInfoToggle.addEventListener("click", () => {
    const showInfo = tabInfo.classList.contains("hidden");
    tabInfo.classList.toggle("hidden", !showInfo);
    dashboardView.classList.toggle("hidden", showInfo);
    btnInfoToggle.classList.toggle("active", showInfo);
  });
}

window.refreshUsage = refreshUsage;

async function syncInitialDockState() {
  try {
    const st = await invoke("get_dock_state");
    if (st) {
      isPinned = Boolean(st.isPinned);
      if (btnPin) btnPin.classList.toggle("active", isPinned);
      if (opacitySlider && typeof st.opacity === "number") {
        opacitySlider.value = String(st.opacity);
      }
      window.__onDockStateChanged(
        Boolean(st.isCollapsed),
        st.dockEdge || "taskbar",
        st.taskbarColor || null
      );
    }
  } catch (_) {}
}

// Initialize dock state & locales first, then load usage & start 5-minute (300s) auto-refresh
Promise.all([syncInitialDockState(), initI18n()]).then(() => {
  refreshUsage();
  setInterval(refreshUsage, 300000);

  // Calmly alternate taskbar pill between 5-hour ("5h") and Weekly ("weekly") usage every 10 seconds
  setInterval(() => {
    if (!lastSnapshot || !lastSnapshot.authenticated) return;
    if (edgeRows) {
      edgeRows.classList.add("fading");
      setTimeout(() => {
        edgeWindowMode = edgeWindowMode === "5h" ? "weekly" : "5h";
        updateEdgeDockPill(lastSnapshot);
        edgeRows.classList.remove("fading");
      }, 420);
    } else {
      edgeWindowMode = edgeWindowMode === "5h" ? "weekly" : "5h";
      updateEdgeDockPill(lastSnapshot);
    }
  }, 10000);

  // Locally decrement reset countdown timers once per minute without hitting the network
  setInterval(() => {
    if (!lastSnapshot || !lastSnapshot.authenticated) return;
    for (const b of lastSnapshot.buckets || []) {
      if (typeof b.resetSeconds === "number" && b.resetSeconds > 0) {
        b.resetSeconds = Math.max(0, b.resetSeconds - 60);
      }
    }
    for (const m of lastSnapshot.models || []) {
      if (typeof m.resetSeconds === "number" && m.resetSeconds > 0) {
        m.resetSeconds = Math.max(0, m.resetSeconds - 60);
      }
    }
    renderSnapshot(lastSnapshot);
  }, 60000);
});
