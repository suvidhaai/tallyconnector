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
    card.innerHTML = `
      <div class="card-top">
        <div class="card-icon connected-icon">${buildingIcon}</div>
        <div class="card-meta">
          <div class="card-name">${company.name}</div>
        </div>
      </div>
      <div class="card-actions">
        <div class="card-btns">
          <button class="btn-sync btn-master" data-action="master" data-company="${company.name}">${syncIcon} Sync Master</button>
          <button class="btn-sync btn-vouchers" data-action="vouchers" data-company="${company.name}">${voucherIcon} Sync Vouchers</button>
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
        statusEl.innerHTML = `${checkIcon} ${data.ledgers_synced} ledgers · ${data.vouchers_synced} vouchers`;
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

      showToast(`✓ ${company} — ${data.ledgers_synced} ledgers, ${data.vouchers_synced} vouchers`, "success");
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
  try {
    const res  = await fetch(`${API_BASE}/tally/sync-ledgers?company=${encodeURIComponent(company)}`);
    const data = await res.json();
    if (data.ok) {
      showToast(`✓ Synced ${data.synced} ledgers — ${company}`, "success");
      const idx = allCompanies.findIndex(c => c.name === company);
      if (idx !== -1) { allCompanies[idx].connected = true; allCompanies[idx].lastMasterSync = data.syncedAt; }
      renderCompanies(searchInput.value);
    } else {
      showToast(`✗ ${data.error}`, "error");
      btn.classList.remove("loading"); btn.innerHTML = orig;
    }
  } catch {
    showToast("✗ Could not reach server", "error");
    btn.classList.remove("loading"); btn.innerHTML = orig;
  }
}

async function syncVouchers(btn) {
  const company = btn.dataset.company;
  const orig    = btn.innerHTML;
  btn.classList.add("loading");
  btn.innerHTML = `${voucherIcon} Syncing…`;
  try {
    const res  = await fetch(`${API_BASE}/tally/sync-vouchers?company=${encodeURIComponent(company)}`);
    const data = await res.json();
    if (data.ok) {
      showToast(`✓ Synced ${data.synced} vouchers — ${company}`, "success");
      const idx = allCompanies.findIndex(c => c.name === company);
      if (idx !== -1) { allCompanies[idx].connected = true; allCompanies[idx].lastVoucherSync = data.syncedAt; }
      renderCompanies(searchInput.value);
    } else {
      showToast(`✗ ${data.error}`, "error");
      btn.classList.remove("loading"); btn.innerHTML = orig;
    }
  } catch {
    showToast("✗ Could not reach server", "error");
    btn.classList.remove("loading"); btn.innerHTML = orig;
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