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
  if (!sessionStorage.getItem("selectedClientId")) {
    showToast("⚠ Please select a client first", "error");
    return;
  }
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
      const clientId = sessionStorage.getItem("selectedClientId") || "";
      let url = `${API_BASE}/tally/sync-ledgers?company=${encodeURIComponent(company)}&step=ledgers&group=${encodeURIComponent(groupName)}&client_id=${encodeURIComponent(clientId)}`;
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
    const clientId = sessionStorage.getItem("selectedClientId") || "";
    let url = `${API_BASE}/tally/sync-ledgers?company=${encodeURIComponent(company)}&step=stock_items&client_id=${encodeURIComponent(clientId)}`;
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
  if (!sessionStorage.getItem("selectedClientId")) {
    showToast("⚠ Please select a client first", "error");
    return;
  }
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
loadCompanies().then(tryAutoSelectClient);
setInterval(checkTallyHealth, 15000);

// ═══════════════════════════════════════════════════════
// CLIENT SELECTOR DROPDOWN
// ═══════════════════════════════════════════════════════

const SUPABASE_URL = "https://yjcbbgjrxvwbdrcprbiy.supabase.co";
const SUPABASE_ANON_KEY = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InlqY2JiZ2pyeHZ3YmRyY3ByYml5Iiwicm9sZSI6InNlcnZpY2Vfcm9sZSIsImlhdCI6MTc2NzA4NTUyNiwiZXhwIjoyMDgyNjYxNTI2fQ.0fEUSYFaiPMBYt-SZKgbVapGtfk-8I5JjQ6fheeCdxA";

let allClients = [];
const clientBtn = document.getElementById("btn-client-select");
const clientLabel = document.getElementById("client-select-label");
const clientPanel = document.getElementById("client-dropdown-panel");
const clientListEl = document.getElementById("client-dropdown-list");
const clientSearchInput = document.getElementById("client-dropdown-search");
const clientHint = document.getElementById("client-hint");

// Restore previous selection from sessionStorage
(function restoreClientSelection() {
  const savedName = sessionStorage.getItem("selectedClientName");
  if (savedName) {
    clientLabel.textContent = savedName;
    clientBtn.classList.add("has-selection");
    if (clientHint) clientHint.classList.add("hidden");
  }
})();

async function loadClients() {
  const email = sessionStorage.getItem("sb_user_email");
  if (!email) return;

  clientListEl.innerHTML = `<div class="client-dropdown-loading">Loading…</div>`;

  try {
    // Step 1: Get firm_id from users table (cache it)
    let firmId = sessionStorage.getItem("sb_firm_id");
    if (!firmId) {
      const userRes = await fetch(
        `${SUPABASE_URL}/rest/v1/users?select=id,firm_id&email=eq.${encodeURIComponent(email)}&limit=1`, {
        headers: {
          "apikey": SUPABASE_ANON_KEY,
          "Authorization": `Bearer ${SUPABASE_ANON_KEY}`,
        },
      });
      if (!userRes.ok) throw new Error(`Users lookup HTTP ${userRes.status}`);
      const users = await userRes.json();
      console.log("[Clients] Users lookup result:", users);
      if (!users.length || !users[0].firm_id) {
        clientListEl.innerHTML = `<div class="client-dropdown-loading">No firm linked to this account</div>`;
        return;
      }
      firmId = users[0].firm_id;
      sessionStorage.setItem("sb_firm_id", firmId);
      sessionStorage.setItem("sb_user_id", users[0].id);
    }

    // Step 2: Fetch clients by firm_id
    const clientRes = await fetch(
      `${SUPABASE_URL}/rest/v1/clients?select=id,name&firm_id=eq.${encodeURIComponent(firmId)}&order=name.asc`, {
      headers: {
        "apikey": SUPABASE_ANON_KEY,
        "Authorization": `Bearer ${SUPABASE_ANON_KEY}`,
      },
    });
    if (!clientRes.ok) throw new Error(`Clients lookup HTTP ${clientRes.status}`);
    allClients = await clientRes.json();
    console.log(`[Clients] Loaded ${allClients.length} clients for firm ${firmId}:`, allClients);
    renderClientList();
  } catch (e) {
    console.error("[Clients] Failed to load:", e);
    clientListEl.innerHTML = `<div class="client-dropdown-loading">Failed to load clients</div>`;
  }
}

function renderClientList(filter = "") {
  const q = filter.toLowerCase();
  const filtered = allClients.filter(c => (c.name || "").toLowerCase().includes(q));
  const selectedId = sessionStorage.getItem("selectedClientId");

  if (filtered.length === 0) {
    clientListEl.innerHTML = `<div class="client-dropdown-empty">No clients found</div>`;
    return;
  }

  clientListEl.innerHTML = "";
  filtered.forEach(client => {
    const isSelected = client.id === selectedId;
    const initials = (client.name || "?").slice(0, 2).toUpperCase();
    const item = document.createElement("div");
    item.className = "client-dropdown-item" + (isSelected ? " selected" : "");
    item.dataset.clientId = client.id;
    item.dataset.clientName = client.name || "";
    item.innerHTML = `
      <div class="client-item-icon">${initials}</div>
      <span class="client-item-name">${client.name || "Unnamed"}</span>
      <svg class="client-item-check" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><polyline points="20 6 9 17 4 12"/></svg>
    `;
    item.addEventListener("click", () => selectClient(client));
    clientListEl.appendChild(item);
  });
}

function selectClient(client) {
  sessionStorage.setItem("selectedClientId", client.id);
  sessionStorage.setItem("selectedClientName", client.name || "");
  clientLabel.textContent = client.name || "Unnamed";
  clientBtn.classList.add("has-selection");
  if (clientHint) clientHint.classList.add("hidden");
  closeClientDropdown();
  showToast(`✓ Switched to ${client.name}`, "success");
}

function toggleClientDropdown() {
  const isOpen = clientPanel.classList.contains("open");
  if (isOpen) {
    closeClientDropdown();
  } else {
    openClientDropdown();
  }
}

function openClientDropdown() {
  clientPanel.classList.add("open");
  clientBtn.classList.add("active");
  clientSearchInput.value = "";
  clientSearchInput.focus();
  loadClients();
}

function closeClientDropdown() {
  clientPanel.classList.remove("open");
  clientBtn.classList.remove("active");
}

clientBtn.addEventListener("click", (e) => {
  e.stopPropagation();
  toggleClientDropdown();
});

clientPanel.addEventListener("click", (e) => e.stopPropagation());

clientSearchInput.addEventListener("input", () => {
  renderClientList(clientSearchInput.value);
});

document.addEventListener("click", () => closeClientDropdown());
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeClientDropdown();
});

// ── Auto-select client matching a Tally company name ────
async function tryAutoSelectClient() {
  // Skip if user already picked a client this session
  if (sessionStorage.getItem("selectedClientId")) return;
  // Need companies loaded from Tally
  if (!allCompanies.length) return;

  // Preload clients if not yet loaded
  if (!allClients.length) {
    const email = sessionStorage.getItem("sb_user_email");
    if (!email) return;
    try {
      let firmId = sessionStorage.getItem("sb_firm_id");
      if (!firmId) {
        const uRes = await fetch(
          `${SUPABASE_URL}/rest/v1/users?select=id,firm_id&email=eq.${encodeURIComponent(email)}&limit=1`, {
          headers: { "apikey": SUPABASE_ANON_KEY, "Authorization": `Bearer ${SUPABASE_ANON_KEY}` },
        });
        const users = await uRes.json();
        if (!users.length || !users[0].firm_id) return;
        firmId = users[0].firm_id;
        sessionStorage.setItem("sb_firm_id", firmId);
        sessionStorage.setItem("sb_user_id", users[0].id);
      }
      const cRes = await fetch(
        `${SUPABASE_URL}/rest/v1/clients?select=id,name&firm_id=eq.${encodeURIComponent(firmId)}&order=name.asc`, {
        headers: { "apikey": SUPABASE_ANON_KEY, "Authorization": `Bearer ${SUPABASE_ANON_KEY}` },
      });
      allClients = await cRes.json();
    } catch { return; }
  }

  // Only auto-select on exact name match
  const tallyNames = allCompanies.map(c => c.name.toLowerCase());
  let bestMatch = null;
  for (const client of allClients) {
    const cn = (client.name || "").toLowerCase();
    if (!cn) continue;
    if (tallyNames.includes(cn)) { bestMatch = client; break; }
  }

  if (bestMatch) {
    selectClient(bestMatch);
    console.log(`[Clients] Auto-selected "${bestMatch.name}" based on Tally company match`);
  }
}


// ═══════════════════════════════════════════════════════
// AUTO-UPDATE CHECK (runs on every launch)
// ═══════════════════════════════════════════════════════

async function checkForAppUpdates() {
  // Only runs inside Tauri — skip if opened in regular browser
  if (!window.__TAURI__) return;

  try {
    const { check } = window.__TAURI__.updater;
    const { relaunch } = window.__TAURI__.process;

    console.log('[Updater] Checking for updates...');
    const update = await check();

    if (!update) {
      console.log('[Updater] App is up to date');
      return;
    }

    console.log(`[Updater] Update available: v${update.version}`);

    // Build the update modal
    showUpdateModal({
      currentVersion: update.currentVersion,
      newVersion: update.version,
      notes: update.body || '',
      date: update.date || '',
      onUpdate: async () => {
        const modal = document.getElementById('update-modal');
        const body = modal.querySelector('.update-modal-body');
        const actions = modal.querySelector('.update-modal-actions');

        // Switch to download progress view
        body.innerHTML = `
          <div style="text-align:center;padding:1.5rem 0">
            <div style="font-size:2rem;margin-bottom:.75rem">⬇️</div>
            <div style="font-weight:700;color:#0f172a;margin-bottom:.5rem">Downloading update…</div>
            <div style="background:#f1f5f9;border-radius:8px;height:8px;overflow:hidden;margin:.75rem 0">
              <div id="update-progress-bar" style="height:100%;background:linear-gradient(90deg,#3b82f6,#1e40af);border-radius:8px;width:0%;transition:width .3s"></div>
            </div>
            <div id="update-progress-text" style="font-size:.75rem;color:#64748b">Starting download…</div>
          </div>`;
        actions.innerHTML = '';

        try {
          let downloaded = 0;
          let contentLength = 0;
          await update.downloadAndInstall((event) => {
            switch (event.event) {
              case 'Started':
                contentLength = event.data.contentLength || 0;
                break;
              case 'Progress':
                downloaded += event.data.chunkLength || 0;
                const pct = contentLength > 0 ? Math.round((downloaded / contentLength) * 100) : 0;
                const bar = document.getElementById('update-progress-bar');
                const text = document.getElementById('update-progress-text');
                if (bar) bar.style.width = pct + '%';
                if (text) {
                  const mb = (downloaded / 1048576).toFixed(1);
                  const totalMb = contentLength > 0 ? (contentLength / 1048576).toFixed(1) : '?';
                  text.textContent = `${mb} MB / ${totalMb} MB (${pct}%)`;
                }
                break;
              case 'Finished':
                break;
            }
          });

          // Download complete — show restart prompt
          body.innerHTML = `
            <div style="text-align:center;padding:1.5rem 0">
              <div style="font-size:2.5rem;margin-bottom:.75rem">✅</div>
              <div style="font-weight:800;color:#059669;font-size:1.05rem;margin-bottom:.35rem">Update Installed!</div>
              <div style="font-size:.82rem;color:#64748b">Restart the app to use the new version.</div>
            </div>`;
          actions.innerHTML = `
            <button id="update-restart-btn" style="padding:.65rem 2rem;background:linear-gradient(135deg,#059669,#047857);color:#fff;border:none;border-radius:10px;font-weight:700;font-size:.85rem;cursor:pointer;font-family:inherit;transition:all .15s">
              🔄 Restart Now
            </button>`;
          document.getElementById('update-restart-btn').onclick = async () => {
            await relaunch();
          };

        } catch (err) {
          console.error('[Updater] Download failed:', err);
          body.innerHTML = `
            <div style="text-align:center;padding:1.5rem 0">
              <div style="font-size:2rem;margin-bottom:.75rem">❌</div>
              <div style="font-weight:700;color:#dc2626;margin-bottom:.35rem">Update Failed</div>
              <div style="font-size:.82rem;color:#64748b">${err.message || 'Download error occurred'}</div>
            </div>`;
          actions.innerHTML = `
            <button onclick="document.getElementById('update-modal-overlay').remove()" style="padding:.5rem 1.5rem;background:#f1f5f9;border:1px solid #e2e8f0;border-radius:8px;font-weight:600;font-size:.82rem;cursor:pointer;font-family:inherit;color:#64748b">Close</button>`;
        }
      },
      onSkip: () => {
        // User chose to skip — just close modal
        const overlay = document.getElementById('update-modal-overlay');
        if (overlay) overlay.remove();
      }
    });

  } catch (err) {
    // Silently fail — don't block the app if update check fails
    console.warn('[Updater] Update check failed (non-fatal):', err);
  }
}

function showUpdateModal({ currentVersion, newVersion, notes, date, onUpdate, onSkip }) {
  // Remove existing overlay if any
  const existing = document.getElementById('update-modal-overlay');
  if (existing) existing.remove();

  // Parse markdown-ish release notes to simple HTML
  const notesHtml = notes
    ? notes.split('\n').map(line => {
        line = line.trim();
        if (!line) return '';
        if (line.startsWith('### ')) return `<div style="font-weight:700;margin-top:.5rem;color:#0f172a">${line.slice(4)}</div>`;
        if (line.startsWith('## '))  return `<div style="font-weight:700;margin-top:.5rem;color:#0f172a">${line.slice(3)}</div>`;
        if (line.startsWith('- '))   return `<div style="padding-left:.75rem;position:relative"><span style="position:absolute;left:0">•</span>${line.slice(2)}</div>`;
        if (line.startsWith('* '))   return `<div style="padding-left:.75rem;position:relative"><span style="position:absolute;left:0">•</span>${line.slice(2)}</div>`;
        return `<div>${line}</div>`;
      }).join('')
    : '<div style="color:#94a3b8">No release notes available</div>';

  const overlay = document.createElement('div');
  overlay.id = 'update-modal-overlay';
  overlay.style.cssText = 'position:fixed;inset:0;background:rgba(15,23,42,.55);backdrop-filter:blur(4px);z-index:99999;display:flex;align-items:center;justify-content:center;animation:fadeIn .2s ease';

  overlay.innerHTML = `
    <div id="update-modal" style="background:#fff;border-radius:16px;box-shadow:0 25px 50px rgba(0,0,0,.25);max-width:440px;width:90%;overflow:hidden;animation:slideUp .3s ease;font-family:'Inter','Segoe UI',sans-serif">
      <!-- Header -->
      <div style="padding:1.25rem 1.5rem;background:linear-gradient(135deg,#1e3a8a,#3b82f6);color:#fff">
        <div style="display:flex;align-items:center;gap:.65rem">
          <div style="width:38px;height:38px;border-radius:10px;background:rgba(255,255,255,.15);display:flex;align-items:center;justify-content:center;font-size:1.25rem;flex-shrink:0">🚀</div>
          <div>
            <div style="font-weight:800;font-size:1rem;letter-spacing:-.01em">Update Available</div>
            <div style="font-size:.72rem;opacity:.8;margin-top:2px">A new version of SuvidhaAI Connector is ready</div>
          </div>
        </div>
      </div>

      <!-- Version badges -->
      <div style="padding:.85rem 1.5rem;display:flex;align-items:center;gap:.5rem;border-bottom:1px solid #f1f5f9">
        <span style="font-size:.7rem;font-weight:700;padding:.2rem .55rem;border-radius:6px;background:#fef2f2;color:#dc2626">v${currentVersion || '?'}</span>
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#94a3b8" stroke-width="2"><path d="M5 12h14"/><path d="M12 5l7 7-7 7"/></svg>
        <span style="font-size:.7rem;font-weight:700;padding:.2rem .55rem;border-radius:6px;background:#ecfdf5;color:#059669">v${newVersion}</span>
        ${date ? `<span style="font-size:.65rem;color:#94a3b8;margin-left:auto">${new Date(date).toLocaleDateString('en-IN', {day:'numeric',month:'short',year:'numeric'})}</span>` : ''}
      </div>

      <!-- Body (notes) -->
      <div class="update-modal-body" style="padding:1rem 1.5rem;max-height:200px;overflow-y:auto">
        <div style="font-size:.68rem;font-weight:700;text-transform:uppercase;letter-spacing:.05em;color:#9ca3af;margin-bottom:.4rem">What's New</div>
        <div style="font-size:.82rem;color:#334155;line-height:1.6">${notesHtml}</div>
      </div>

      <!-- Actions -->
      <div class="update-modal-actions" style="padding:1rem 1.5rem;border-top:1px solid #f1f5f9;display:flex;align-items:center;justify-content:flex-end;gap:.5rem">
        <button id="update-skip-btn" style="padding:.55rem 1.25rem;background:#f8fafc;border:1px solid #e2e8f0;border-radius:9px;font-weight:600;font-size:.82rem;cursor:pointer;font-family:inherit;color:#64748b;transition:all .15s">
          Later
        </button>
        <button id="update-now-btn" style="padding:.55rem 1.5rem;background:linear-gradient(135deg,#3b82f6,#1e40af);color:#fff;border:none;border-radius:9px;font-weight:700;font-size:.82rem;cursor:pointer;font-family:inherit;transition:all .15s;box-shadow:0 2px 8px rgba(59,130,246,.3)">
          ✨ Update Now
        </button>
      </div>
    </div>`;

  document.body.appendChild(overlay);

  // Wire up buttons
  document.getElementById('update-skip-btn').onclick = onSkip;
  document.getElementById('update-now-btn').onclick = onUpdate;

  // Close on backdrop click
  overlay.addEventListener('click', (e) => {
    if (e.target === overlay) onSkip();
  });
}

// Add animation keyframes
(function() {
  const style = document.createElement('style');
  style.textContent = `
    @keyframes fadeIn { from { opacity: 0 } to { opacity: 1 } }
    @keyframes slideUp { from { opacity: 0; transform: translateY(20px) } to { opacity: 1; transform: translateY(0) } }
  `;
  document.head.appendChild(style);
})();

// Check for updates 2 seconds after load (non-blocking)
setTimeout(checkForAppUpdates, 2000);