const API_BASE = "http://127.0.0.1:17890";

// ── Auth helpers ─────────────────────────────────────────────
function getToken() {
  return sessionStorage.getItem("sb_access_token") || "";
}

function requireAuth() {
  if (!getToken()) {
    window.location.href = "login.html";
    return false;
  }
  return true;
}

// ── State ────────────────────────────────────────────────────
let allCompanies   = [];
let tallyCompanies = [];
const selectedFYMap = {};  // company → "20250401|20260331" (persists across re-renders)

// ── FY Period Helpers ────────────────────────────────────
function getFYOptions() {
  const now = new Date();
  const curYear = now.getFullYear();
  const fyStart = now.getMonth() >= 3 ? curYear : curYear - 1;
  const options = [];
  // Show last 5 FYs + current + next
  for (let y = fyStart - 5; y <= fyStart + 1; y++) {
    const short = String(y + 1).slice(-2);
    options.push({
      label: `FY ${y}-${short}`,
      from_date: `${y}0401`,
      to_date:   `${y + 1}0331`,
      isCurrent: y === fyStart,
    });
  }
  return options;
}

function buildFYSelect(companyName) {
  const opts = getFYOptions();
  const id = `fy-select-${companyName.replace(/[^a-zA-Z0-9]/g, '_')}`;
  const savedVal = selectedFYMap[companyName];
  let html = `<select class="fy-select" id="${id}" data-company="${companyName}">`;
  opts.forEach(o => {
    const val = `${o.from_date}|${o.to_date}`;
    const isSelected = savedVal ? (val === savedVal) : o.isCurrent;
    html += `<option value="${val}"${isSelected ? ' selected' : ''}>${o.label}</option>`;
  });
  html += '</select>';
  return html;
}

function getSelectedFY(companyName) {
  const id = `fy-select-${companyName.replace(/[^a-zA-Z0-9]/g, '_')}`;
  const sel = document.getElementById(id);
  if (!sel) return { from_date: '', to_date: '' };
  const [from_date, to_date] = sel.value.split('|');
  // Persist the selection so it survives card re-renders
  selectedFYMap[companyName] = sel.value;
  return { from_date, to_date };
}

// ── DOM ──────────────────────────────────────────────────────
const connectedList    = document.getElementById("connected-list");
const disconnectedList = document.getElementById("disconnected-list");
const searchInput      = document.getElementById("search-input");
const refreshBtn       = document.getElementById("refresh-btn");
const tallyText        = document.getElementById("tally-text");
const tallyStatusItem  = document.getElementById("tally-status-item");
const tallyIcon        = document.getElementById("tally-icon");
const toast            = document.getElementById("toast");
const drawer           = document.getElementById("add-companies-drawer");
const drawerBackdrop   = document.getElementById("drawer-backdrop");
const drawerList       = document.getElementById("drawer-list");
const drawerSearch     = document.getElementById("drawer-search");

// ── Toast ────────────────────────────────────────────────────
let toastTimer;
function showToast(msg, type = "") {
  toast.textContent = msg;
  toast.className = "toast show " + type;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toast.className = "toast", 3500);
}

// ── Icons ────────────────────────────────────────────────────
const buildingIcon = `<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><rect x="2" y="7" width="20" height="14" rx="1.5"/><path d="M16 7V5a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v2"/><line x1="12" y1="12" x2="12" y2="16"/><line x1="10" y1="14" x2="14" y2="14"/></svg>`;
const syncIcon     = `<svg class="spin-icon" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>`;
const voucherIcon  = `<svg class="spin-icon" width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><rect x="2" y="5" width="20" height="14" rx="1.5"/><line x1="2" y1="10" x2="22" y2="10"/></svg>`;
const clockIcon    = `<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>`;
const warnIcon     = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg>`;
const checkIcon    = `<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="20 6 9 17 4 12"/></svg>`;

// ═══════════════════════════════════════════════════════
// MAIN COMPANY CARDS
// ═══════════════════════════════════════════════════════

function buildCard(company) {
  const card = document.createElement("div");
  card.className = "card";
  card.dataset.company = company.name;

  if (company.connected) {
    const masterDate  = company.lastMasterSync  || "Never";
    const voucherDate = company.lastVoucherSync || "Never";
    const isActive = company.activeInTally;
    const activeBadge = isActive
      ? `<span class="tally-active-badge active"><svg width="6" height="6" viewBox="0 0 12 12"><circle cx="6" cy="6" r="6" fill="#22c55e"/></svg> Active in Tally</span>`
      : `<span class="tally-active-badge offline"><svg width="6" height="6" viewBox="0 0 12 12"><circle cx="6" cy="6" r="6" fill="#94a3b8"/></svg> Offline</span>`;
    card.innerHTML = `
      <div class="card-top">
        <div class="card-icon connected-icon">${buildingIcon}</div>
        <div class="card-meta">
          <div class="card-name">${company.name}</div>
        </div>
        ${activeBadge}
      </div>
      <div class="card-actions">
        <div class="card-fy-row">
          <span class="fy-label">Sync Period:</span>
          ${buildFYSelect(company.name)}
        </div>
        <div class="card-btns">
          <button class="btn-sync btn-master" data-action="master" data-company="${company.name}"${isActive ? '' : ' disabled title="Open this company in Tally to sync"'}>${syncIcon} Sync Master</button>
          <button class="btn-sync btn-vouchers" data-action="vouchers" data-company="${company.name}"${isActive ? '' : ' disabled title="Open this company in Tally to sync"'}>${voucherIcon} Sync Vouchers</button>
        </div>
        <div class="card-sync-info">
          <span class="sync-date-item">${clockIcon} Masters: <strong>${masterDate}</strong></span>
          <span class="sync-date-item">${clockIcon} Vouchers: <strong>${voucherDate}</strong></span>
        </div>
      </div>`;
  } else {
    card.innerHTML = `
      <div class="card-top">
        <div class="card-icon">${buildingIcon}</div>
        <div class="card-meta">
          <div class="card-name">${company.name}</div>
          <div class="card-id">In database · not yet synced with Tally</div>
        </div>
        <div class="card-pending">${warnIcon} Sync Pending</div>
      </div>
      <div class="card-actions">
        <div class="card-btns">
          <button class="btn-sync btn-master" data-action="master" data-company="${company.name}">${syncIcon} Sync Master</button>
          <button class="btn-sync btn-vouchers" data-action="vouchers" data-company="${company.name}">${voucherIcon} Sync Vouchers</button>
        </div>
        <div class="card-sync-info">
          <span class="sync-date-item sync-hint">First sync will move this to Connected</span>
        </div>
      </div>`;
  }
  return card;
}

function renderCompanies(filter = "") {
  const q = filter.toLowerCase();
  const filtered = allCompanies.filter(c => c.name.toLowerCase().includes(q));
  const connected    = filtered.filter(c =>  c.connected);
  const disconnected = filtered.filter(c => !c.connected);

  connectedList.innerHTML    = "";
  disconnectedList.innerHTML = "";

  if (connected.length === 0) {
    connectedList.innerHTML = emptyState("No connected companies");
  } else {
    connected.forEach((c, i) => {
      const card = buildCard(c);
      card.style.animationDelay = `${i * 40}ms`;
      connectedList.appendChild(card);
    });
  }

  if (disconnected.length === 0) {
    disconnectedList.innerHTML = emptyState("No disconnected companies");
  } else {
    disconnected.forEach((c, i) => {
      const card = buildCard(c);
      card.style.animationDelay = `${i * 40}ms`;
      disconnectedList.appendChild(card);
    });
  }
}

function emptyState(msg) {
  return `<div class="empty-state">
    <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="#ccc" stroke-width="1.2">
      <rect x="2" y="7" width="20" height="14" rx="1.5"/>
      <path d="M16 7V5a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v2"/>
    </svg>
    ${msg}
  </div>`;
}

async function loadCompanies() {
  const spinner = `<div class="empty-state"><svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="#1a56db" stroke-width="2" style="animation:spin 0.8s linear infinite"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg>Loading…</div>`;
  connectedList.innerHTML = disconnectedList.innerHTML = spinner;
  try {
    const res  = await fetch(`${API_BASE}/tally/companies`);
    const data = await res.json();
    allCompanies = (data.ok && Array.isArray(data.companies)) ? data.companies : [];
    if (!data.ok) showToast(`✗ ${data.error || "Failed to load"}`, "error");
  } catch {
    allCompanies = [];
    showToast("✗ Could not reach server", "error");
  }
  renderCompanies(searchInput.value);
}

// ═══════════════════════════════════════════════════════
// ADD COMPANIES DRAWER
// ═══════════════════════════════════════════════════════

function openDrawer() {
  drawerSearch.value = "";
  drawer.classList.add("open");
  drawerBackdrop.classList.add("open");
  document.body.style.overflow = "hidden";
  loadDrawerCompanies();
}

function closeDrawer() {
  drawer.classList.remove("open");
  drawerBackdrop.classList.remove("open");
  document.body.style.overflow = "";
}

async function loadDrawerCompanies() {
  drawerList.innerHTML = `<div class="drawer-loading">
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="#1a56db" stroke-width="2" style="animation:spin 0.8s linear infinite">
      <polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/>
    </svg>
    Fetching companies from Tally…
  </div>`;

  try {
    const res  = await fetch(`${API_BASE}/tally/available-companies?token=${encodeURIComponent(getToken())}`);
    const data = await res.json();

    // Session expired or invalid — redirect to login
    if (res.status === 401 || res.status === 403) {
      sessionStorage.clear();
      window.location.href = "login.html";
      return;
    }

    if (!data.ok || !Array.isArray(data.companies)) {
      drawerList.innerHTML = `<div class="drawer-error">
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#e8621a" stroke-width="2">
          <circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/>
        </svg>
        ${data.error || "Could not load companies"}
      </div>`;
      return;
    }

    // Show firm info in drawer subtitle if returned
    if (data.firm_id) {
      const subtitle = document.querySelector(".drawer-subtitle");
      if (subtitle) subtitle.textContent = `${data.total} client${data.total !== 1 ? 's' : ''} in your firm · select to sync`;
    }

    tallyCompanies = data.companies;
    renderDrawerList(drawerSearch.value);
  } catch {
    drawerList.innerHTML = `<div class="drawer-error">Could not reach server. Is Tally running?</div>`;
  }
}

function renderDrawerList(filter = "") {
  const q = filter.toLowerCase();
  const filtered = tallyCompanies.filter(c => c.name.toLowerCase().includes(q));

  if (filtered.length === 0) {
    drawerList.innerHTML = `<div class="drawer-empty">
      <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="#ccc" stroke-width="1.2">
        <rect x="2" y="7" width="20" height="14" rx="1.5"/>
        <path d="M16 7V5a2 2 0 0 0-2-2h-4a2 2 0 0 0-2 2v2"/>
      </svg>
      No companies found in Tally
    </div>`;
    return;
  }

  drawerList.innerHTML = "";
  filtered.forEach((c, i) => {
    const row = document.createElement("div");
    row.className = "drawer-row" + (c.already_added ? " already-added" : "");
    row.dataset.company = c.name;
    row.style.animationDelay = `${i * 30}ms`;

    // Determine status label and button state
    let statusClass, statusHtml, btnHtml;
    if (c.already_added) {
      statusClass = 'already';
      statusHtml  = `${checkIcon} Already connected`;
      btnHtml     = `<button class="btn-add-company btn-added" disabled>${checkIcon} Added</button>`;
    } else if (c.in_tally) {
      statusClass = 'pending';
      statusHtml  = `<span class="tally-badge">Open in Tally</span> Ready to sync`;
      btnHtml     = `<button class="btn-add-company" data-action="add-company" data-company="${c.name}">
                       <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><circle cx="12" cy="12" r="10"/><path d="M12 8v8M8 12h8"/></svg>
                       Add &amp; Sync
                     </button>`;
    } else {
      statusClass = 'not-in-tally';
      statusHtml  = `<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="10"/><line x1="12" y1="8" x2="12" y2="12"/><line x1="12" y1="16" x2="12.01" y2="16"/></svg> Not open in Tally`;
      btnHtml     = `<button class="btn-add-company btn-not-in-tally" disabled title="Open this company in Tally first">
                       <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>
                       Open in Tally
                     </button>`;
    }

    row.innerHTML = `
      <div class="drawer-row-icon ${c.already_added ? 'connected-icon' : ''}">${buildingIcon}</div>
      <div class="drawer-row-meta">
        <div class="drawer-row-name">${c.name}</div>
        <div class="drawer-row-status ${statusClass}">${statusHtml}</div>
      </div>
      ${btnHtml}`;

    drawerList.appendChild(row);
  });
}

// ── Full onboarding: syncs master + vouchers atomically ──────
async function addCompany(btn) {
  const company  = btn.dataset.company;
  const row      = btn.closest(".drawer-row");
  const statusEl = row.querySelector(".drawer-row-status");

  btn.disabled = true;
  btn.innerHTML = `<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" style="animation:spin 0.7s linear infinite"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg> Syncing…`;

  if (statusEl) {
    statusEl.className = "drawer-row-status syncing";
    statusEl.innerHTML = `<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="#1a56db" stroke-width="2" style="animation:spin 0.7s linear infinite"><polyline points="23 4 23 10 17 10"/><path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"/></svg> Syncing master data &amp; vouchers…`;
  }

  try {
    const res  = await fetch(`${API_BASE}/tally/add-company?company=${encodeURIComponent(company)}`);
    const data = await res.json();

    if (data.ok) {
      // Update drawer row → success state
      btn.className = "btn-add-company btn-added";
      btn.innerHTML = `${checkIcon} Added`;
      row.classList.add("already-added");
      if (statusEl) {
        statusEl.className = "drawer-row-status already";
        statusEl.innerHTML = `${checkIcon} ${data.ledgers_synced} ledgers · ${data.stock_items_synced || 0} items · ${data.vouchers_synced} vouchers`;
      }

      // Mark in tallyCompanies so filter re-renders correctly
      const ti = tallyCompanies.findIndex(c => c.name === company);
      if (ti !== -1) tallyCompanies[ti].already_added = true;

      // Add / update in main companies list as connected
      const ai = allCompanies.findIndex(c => c.name === company);
      if (ai !== -1) {
        allCompanies[ai].connected       = true;
        allCompanies[ai].lastMasterSync  = data.syncedAt;
        allCompanies[ai].lastVoucherSync = data.syncedAt;
      } else {
        allCompanies.push({
          name: company,
          connected: true,
          lastMasterSync:  data.syncedAt,
          lastVoucherSync: data.syncedAt,
        });
      }

      // Refresh main view live — new card appears in Connected
      renderCompanies(searchInput.value);

      showToast(`✓ ${company} — ${data.ledgers_synced} ledgers, ${data.stock_items_synced || 0} stock items, ${data.vouchers_synced} vouchers`, "success");
    } else {
      btn.disabled = false;
      btn.innerHTML = `Retry`;
      if (statusEl) {
        statusEl.className = "drawer-row-status error-status";
        statusEl.innerHTML = `✗ ${data.error}`;
      }
      showToast(`✗ ${data.error}`, "error");
    }
  } catch {
    btn.disabled = false;
    btn.innerHTML = `Retry`;
    showToast("✗ Could not reach server", "error");
  }
}

// ═══════════════════════════════════════════════════════
// INDIVIDUAL SYNC (on existing cards)
// ═══════════════════════════════════════════════════════

async function syncMaster(btn) {
  const company = btn.dataset.company;
  const orig    = btn.innerHTML;
  btn.classList.add("loading");
  btn.innerHTML = `${syncIcon} Syncing…`;
  const fy = getSelectedFY(company);

  showSyncPanel(company, "master");
  syncLog(`Starting master sync for ${company} (${fy.from_date}–${fy.to_date})`, "info");

  let totalLedgers = 0;
  let totalStockItems = 0;
  let totalErrors = 0;

  // ── Step 1: Fetch group list from Tally ──────────────────────────
  syncLog("Fetching ledger groups from Tally...", "info");
  let groups = [];
  try {
    const gRes = await fetch(`${API_BASE}/tally/list-groups?company=${encodeURIComponent(company)}`);
    const gData = await gRes.json();
    if (gData.ok && gData.groups) {
      groups = gData.groups.map(g => g.name).filter(Boolean);
      syncLog(`Found ${groups.length} ledger groups`, "success");
    } else {
      syncLog(`Failed to fetch groups: ${gData.error || "unknown"}`, "error");
      totalErrors++;
    }
  } catch (e) {
    syncLog("Cannot reach Tally for group list", "error");
    totalErrors++;
  }

  // ── Step 2: Sync ledgers group-by-group ──────────────────────────
  // Total steps = groups + 1 (stock items)
  const totalSteps = groups.length + 1;

  for (let i = 0; i < groups.length; i++) {
    const groupName = groups[i];
    updateSyncProgress(i, totalSteps);
    syncLog(`[${i + 1}/${groups.length}] Syncing ${groupName}...`, "info");

    try {
      let url = `${API_BASE}/tally/sync-ledgers?company=${encodeURIComponent(company)}&step=ledgers&group=${encodeURIComponent(groupName)}`;
      if (fy.from_date) url += `&from_date=${fy.from_date}`;
      if (fy.to_date)   url += `&to_date=${fy.to_date}`;

      const res = await fetch(url);
      const data = await res.json();

      if (data.ok) {
        const count = data.synced || 0;
        totalLedgers += count;
        syncLog(`${groupName}: ${count} ledgers synced`, count > 0 ? "success" : "info");
      } else {
        totalErrors++;
        syncLog(`${groupName}: ${data.error || "failed"}`, "error");
      }
    } catch (e) {
      totalErrors++;
      syncLog(`${groupName}: Connection error`, "error");
    }
  }

  // ── Step 3: Sync stock items ─────────────────────────────────────
  updateSyncProgress(groups.length, totalSteps);
  syncLog(`[Stock Items] Syncing stock items (HSN/GST)...`, "info");
  try {
    let url = `${API_BASE}/tally/sync-ledgers?company=${encodeURIComponent(company)}&step=stock_items`;
    if (fy.from_date) url += `&from_date=${fy.from_date}`;
    if (fy.to_date)   url += `&to_date=${fy.to_date}`;

    const res = await fetch(url);
    const data = await res.json();
    if (data.ok) {
      totalStockItems = data.stock_items_synced || 0;
      syncLog(`Stock Items: ${totalStockItems} synced`, totalStockItems > 0 ? "success" : "info");
    } else {
      totalErrors++;
      syncLog(`Stock Items: ${data.error || "failed"}`, "error");
    }
  } catch (e) {
    totalErrors++;
    syncLog("Stock Items: Connection error", "error");
  }

  // ── Done ─────────────────────────────────────────────────────────
  updateSyncProgress(totalSteps, totalSteps);
  document.getElementById("sync-panel").classList.add("done");
  document.getElementById("sync-panel-label").textContent =
    `Sync Complete — ${totalLedgers} ledgers, ${totalStockItems} stock items`;
  syncLog(`Done! ${totalLedgers} ledgers, ${totalStockItems} stock items synced, ${totalErrors} errors`, totalErrors > 0 ? "error" : "success");

  if (totalLedgers > 0 || totalStockItems > 0) {
    showToast(`✓ Synced ${totalLedgers} ledgers, ${totalStockItems} stock items — ${company}`, "success");
    const idx = allCompanies.findIndex(c => c.name === company);
    if (idx !== -1) { allCompanies[idx].connected = true; allCompanies[idx].lastMasterSync = new Date().toLocaleString(); }
    renderCompanies(searchInput.value);
  } else {
    btn.classList.remove("loading");
    btn.innerHTML = orig;
  }
}

const VOUCHER_TYPES = [
  "Sales", "Purchase", "Receipt", "Payment", "Contra",
  "Journal", "Credit Note", "Debit Note",
  "Sales - Automatic", "Purchase - Automatic"
];

function syncLog(msg, level = "info") {
  const logs = document.getElementById("sync-logs");
  const now = new Date().toLocaleTimeString("en-IN", { hour12: false });
  const icons = { info: "⏳", success: "✅", error: "❌" };
  logs.innerHTML += `<div class="sync-log-entry ${level}">
    <span class="sync-log-time">${now}</span>
    <span class="sync-log-icon">${icons[level] || "·"}</span>
    <span class="sync-log-msg">${msg}</span>
  </div>`;
  logs.scrollTop = logs.scrollHeight;
}

function showSyncPanel(company, mode = "vouchers") {
  const panel = document.getElementById("sync-panel");
  const label = document.getElementById("sync-panel-label");
  const bar = document.getElementById("sync-progress-bar");
  const pctText = document.getElementById("sync-progress-text");
  const logs = document.getElementById("sync-logs");
  const body = document.getElementById("sync-panel-body");

  const title = mode === "master" ? "Syncing Master Data" : "Syncing Vouchers";
  panel.style.display = "block";
  panel.classList.remove("done");
  body.classList.remove("collapsed");
  bar.style.width = "0%";
  pctText.textContent = "0%";
  logs.innerHTML = "";
  label.textContent = `${title} — ${company}`;

  // Toggle collapse
  document.getElementById("sync-panel-toggle").onclick = () => {
    body.classList.toggle("collapsed");
  };
}

function updateSyncProgress(current, total) {
  const pct = Math.round((current / total) * 100);
  document.getElementById("sync-progress-bar").style.width = pct + "%";
  document.getElementById("sync-progress-text").textContent = pct + "%";
}

async function syncVouchers(btn) {
  const company = btn.dataset.company;
  const orig = btn.innerHTML;
  btn.classList.add("loading");
  btn.innerHTML = `${voucherIcon} Syncing…`;
  const fy = getSelectedFY(company);

  showSyncPanel(company);
  syncLog(`Starting voucher sync for ${company} (${fy.from_date}–${fy.to_date})`, "info");

  let totalSynced = 0;
  let totalErrors = 0;

  for (let i = 0; i < VOUCHER_TYPES.length; i++) {
    const vtype = VOUCHER_TYPES[i];
    syncLog(`[${i + 1}/${VOUCHER_TYPES.length}] Syncing ${vtype}...`, "info");
    updateSyncProgress(i, VOUCHER_TYPES.length);

    try {
      let url = `${API_BASE}/tally/sync-vouchers?company=${encodeURIComponent(company)}&voucher_type=${encodeURIComponent(vtype)}`;
      if (fy.from_date) url += `&from_date=${fy.from_date}`;
      if (fy.to_date) url += `&to_date=${fy.to_date}`;

      const res = await fetch(url);
      const data = await res.json();

      if (data.ok) {
        const n = data.synced || 0;
        totalSynced += n;
        syncLog(`${vtype}: ${n} vouchers synced`, n > 0 ? "success" : "info");
      } else {
        totalErrors++;
        syncLog(`${vtype}: ${data.error || "failed"}`, "error");
      }
    } catch (e) {
      totalErrors++;
      syncLog(`${vtype}: Connection error`, "error");
    }
  }

  updateSyncProgress(VOUCHER_TYPES.length, VOUCHER_TYPES.length);
  document.getElementById("sync-panel").classList.add("done");
  document.getElementById("sync-panel-label").textContent =
    `Sync Complete — ${totalSynced} vouchers (${totalErrors} errors)`;
  syncLog(`Done! ${totalSynced} total vouchers synced, ${totalErrors} errors`, totalErrors > 0 ? "error" : "success");

  if (totalSynced > 0) {
    showToast(`✓ Synced ${totalSynced} vouchers — ${company}`, "success");
    const idx = allCompanies.findIndex(c => c.name === company);
    if (idx !== -1) { allCompanies[idx].connected = true; allCompanies[idx].lastVoucherSync = new Date().toLocaleString(); }
    renderCompanies(searchInput.value);
  } else {
    btn.classList.remove("loading");
    btn.innerHTML = orig;
  }
}

// ═══════════════════════════════════════════════════════
// HEALTH
// ═══════════════════════════════════════════════════════

async function checkTallyHealth() {
  try {
    const res  = await fetch(`${API_BASE}/health`);
    const data = await res.json();
    if (data.tally_connected) {
      tallyText.textContent = "CONNECTED";
      tallyStatusItem.className = "status-item ok";
      tallyIcon.setAttribute("stroke", "#22c55e");
    } else throw new Error();
  } catch {
    tallyText.textContent = "DISCONNECTED";
    tallyStatusItem.className = "status-item fail";
    tallyIcon.setAttribute("stroke", "#ef4444");
  }
}

// ═══════════════════════════════════════════════════════
// EVENTS
// ═══════════════════════════════════════════════════════

document.addEventListener("click", e => {
  const btn = e.target.closest("[data-action]");
  if (!btn) return;
  if (btn.dataset.action === "master")      syncMaster(btn);
  if (btn.dataset.action === "vouchers")    syncVouchers(btn);
  if (btn.dataset.action === "add-company") addCompany(btn);
});

// Persist FY dropdown selection when user changes it
document.addEventListener("change", e => {
  if (e.target.classList.contains('fy-select')) {
    const company = e.target.dataset.company;
    if (company) selectedFYMap[company] = e.target.value;
  }
});

// document.getElementById("add-company-btn").addEventListener("click", openDrawer);
// document.getElementById("drawer-close").addEventListener("click", closeDrawer);
// drawerBackdrop.addEventListener("click", closeDrawer);
document.addEventListener("keydown", e => { if (e.key === "Escape") closeDrawer(); });
// drawerSearch.addEventListener("input", () => renderDrawerList(drawerSearch.value));
searchInput.addEventListener("input",  () => renderCompanies(searchInput.value));
refreshBtn.addEventListener("click", () => {
  refreshBtn.classList.add("spinning");
  checkTallyHealth();
  loadCompanies();
  setTimeout(() => refreshBtn.classList.remove("spinning"), 700);
});
// document.querySelector(".btn-yes").addEventListener("click", () => showToast("Click 'Add Companies' to connect a company."));
// document.querySelector(".btn-no").addEventListener("click",  openDrawer);

// ═══════════════════════════════════════════════════════
// LOGOUT
// ═══════════════════════════════════════════════════════
document.querySelector(".btn-logout").addEventListener("click", async () => {
  const token = sessionStorage.getItem("sb_access_token");
  // Call Supabase logout to invalidate the token server-side
  if (token) {
    try {
      await fetch("https://yjcbbgjrxvwbdrcprbiy.supabase.co/auth/v1/logout", {
        method: "POST",
        headers: {
          "apikey": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InlqY2JiZ2pyeHZ3YmRyY3ByYml5Iiwicm9sZSI6InNlcnZpY2Vfcm9sZSIsImlhdCI6MTc2NzA4NTUyNiwiZXhwIjoyMDgyNjYxNTI2fQ.0fEUSYFaiPMBYt-SZKgbVapGtfk-8I5JjQ6fheeCdxA",
          "Authorization": `Bearer ${token}`,
        },
      });
    } catch (_) {}
  }
  sessionStorage.clear();
  window.location.href = "login.html";
});

// ═══════════════════════════════════════════════════════
// INIT
// ═══════════════════════════════════════════════════════

// Show logged-in email in topbar
(function() {
  const email = sessionStorage.getItem("sb_user_email");
  if (email) {
    const appName = document.querySelector(".app-name");
    if (appName) {
      const emailBadge = document.createElement("span");
      emailBadge.style.cssText = "font-size:11px;color:var(--text3);font-weight:400;margin-left:8px;";
      emailBadge.textContent = email;
      appName.parentElement.appendChild(emailBadge);
    }
  }
})();

checkTallyHealth();
loadCompanies();
setInterval(checkTallyHealth, 15000);