// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json,
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use quick_xml::de::from_str;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use axum::http::Method;
use tower_http::cors::{Any, CorsLayer};
use chrono::{Local, Datelike};
use uuid::Uuid;

const SUPABASE_URL: &str = "https://yjcbbgjrxvwbdrcprbiy.supabase.co";
const SUPABASE_KEY: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InlqY2JiZ2pyeHZ3YmRyY3ByYml5Iiwicm9sZSI6InNlcnZpY2Vfcm9sZSIsImlhdCI6MTc2NzA4NTUyNiwiZXhwIjoyMDgyNjYxNTI2fQ.0fEUSYFaiPMBYt-SZKgbVapGtfk-8I5JjQ6fheeCdxA";

// ── Company state ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct CompanyState {
    name: String,
    connected: bool,
    last_master_sync: Option<String>,
    last_voucher_sync: Option<String>,
}

type SharedState = Arc<Mutex<HashMap<String, CompanyState>>>;

// ── Tally XML structs ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
#[serde(rename = "ENVELOPE")]
struct TallyEnvelope {
    #[serde(rename = "BODY", default)]
    body: TallyBody,
}

#[derive(Debug, Deserialize, Default)]
struct TallyBody {
    #[serde(rename = "DATA", default)]
    data: TallyData,
}

#[derive(Debug, Deserialize, Default)]
struct TallyData {
    #[serde(rename = "COLLECTION", default)]
    collection: TallyCollection,
    #[serde(rename = "TALLYMESSAGE", default)]
    tallymessage: Vec<TallyMessage>,
}

#[derive(Debug, Deserialize, Default)]
struct TallyMessage {
    #[serde(rename = "VOUCHER", default)]
    vouchers: Vec<TallyVoucher>,
}

#[derive(Debug, Deserialize, Default)]
struct TallyCollection {
    #[serde(rename = "LEDGER", default)]
    ledgers: Vec<TallyLedger>,
    #[serde(rename = "VOUCHER", default)]
    vouchers: Vec<TallyVoucher>,
}

#[derive(Debug, Deserialize, Default)]
struct TallyLedger {
    #[serde(rename = "@NAME", default)]
    name_attr: String,
    #[serde(rename = "NAME", default)]
    name_elem: String,
    #[serde(rename = "PARENT", default)]
    parent: String,
    #[serde(rename = "OPENINGBALANCE", default)]
    opening_balance: String,
    #[serde(rename = "CLOSINGBALANCE", default)]
    closing_balance: String,
    #[serde(rename = "PARTYGSTIN", default)]
    party_gstin: String,
    #[serde(rename = "GSTREGISTRATIONTYPE", default)]
    gst_registration_type: String,
    #[serde(rename = "LEDSTATENAME", default)]
    state: String,
    #[serde(rename = "PINCODE", default)]
    pin_code: String,
    #[serde(rename = "EMAIL", default)]
    email: String,
    #[serde(rename = "LEDGERMOBILE", default)]
    mobile: String,
    #[serde(rename = "ADDRESS", default)]
    address: String,
    #[serde(rename = "MAILINGNAME", default)]
    mailing_name: String,
    #[serde(rename = "GUID", default)]
    guid: String,
}

impl TallyLedger {
    fn name(&self) -> &str {
        if !self.name_attr.is_empty() { &self.name_attr } else { &self.name_elem }
    }
}

#[derive(Debug, Deserialize, Default)]
struct TallyVoucher {
    #[serde(rename = "DATE", default)]
    date: String,
    #[serde(rename = "VOUCHERTYPENAME", default)]
    voucher_type: String,
    #[serde(rename = "VOUCHERNUMBER", default)]
    voucher_number: String,
    #[serde(rename = "PARTYLEDGERNAME", default)]
    party_name: String,
    #[serde(rename = "AMOUNT", default)]
    amount: String,
    #[serde(rename = "NARRATION", default)]
    narration: String,
    #[serde(rename = "GUID", default)]
    guid: String,
    #[serde(rename = "ALTERID", default)]
    alter_id: String,
}

fn extract_vouchers(envelope: &TallyEnvelope) -> Vec<&TallyVoucher> {
    let from_tallymsg: Vec<&TallyVoucher> = envelope.body.data.tallymessage
        .iter()
        .flat_map(|m| m.vouchers.iter())
        .collect();
    if !from_tallymsg.is_empty() {
        from_tallymsg
    } else {
        envelope.body.data.collection.vouchers.iter().collect()
    }
}

// ── Ledger entry XML parser ──────────────────────────────────────────────────

fn extract_xml_value(xml: &str, tag: &str) -> String {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(start) = xml.find(&open) {
        let after = &xml[start + open.len()..];
        if let Some(end) = after.find(&close) {
            return after[..end].trim().to_string();
        }
    }
    String::new()
}

fn parse_ledger_entries_from_voucher_xml(voucher_xml: &str) -> Vec<LedgerEntryRow> {
    let mut entries = Vec::new();
    for list_tag in &["ALLLEDGERENTRIES.LIST", "LEDGERENTRIES.LIST"] {
        let open_tag = format!("<{}", list_tag);
        let close_tag = format!("</{}>", list_tag);
        let mut search_from = 0;

        while let Some(rel_start) = voucher_xml[search_from..].find(&open_tag) {
            let abs_start = search_from + rel_start;
            let tag_end = match voucher_xml[abs_start..].find('>') {
                Some(p) => abs_start + p + 1,
                None => break,
            };
            let abs_end = match voucher_xml[tag_end..].find(&close_tag) {
                Some(p) => tag_end + p + close_tag.len(),
                None => break,
            };
            let entry_xml = &voucher_xml[tag_end..abs_end - close_tag.len()];
            let ledger_name = extract_xml_value(entry_xml, "LEDGERNAME");
            let amount_str  = extract_xml_value(entry_xml, "AMOUNT");
            let is_deemed   = extract_xml_value(entry_xml, "ISDEEMEDPOSITIVE");
            if !ledger_name.is_empty() {
                entries.push(LedgerEntryRow {
                    ledger_name,
                    amount:   parse_balance(&amount_str),
                    is_debit: is_deemed.to_lowercase() == "yes",
                });
            }
            search_from = abs_end;
        }
        if !entries.is_empty() { break; }
    }
    entries
}

fn parse_ledger_entries_for_guid(raw_xml: &str, guid: &str) -> Vec<LedgerEntryRow> {
    if guid.is_empty() { return Vec::new(); }
    let guid_pos = match raw_xml.find(guid) { Some(p) => p, None => return Vec::new() };
    let voucher_start = match raw_xml[..guid_pos].rfind("<VOUCHER") { Some(p) => p, None => return Vec::new() };
    let voucher_end = match raw_xml[voucher_start..].find("</VOUCHER>") {
        Some(p) => voucher_start + p + "</VOUCHER>".len(),
        None => return Vec::new(),
    };
    parse_ledger_entries_from_voucher_xml(&raw_xml[voucher_start..voucher_end])
}

// ── Inventory entry XML parser ───────────────────────────────────────────────

#[derive(Debug, Clone)]
struct InventoryEntryRow {
    stock_item_name: String,
    quantity:        Option<f64>,
    rate:            Option<f64>,
    amount:          Option<f64>,
    uom:             String,
    godown:          String,
    batch:           String,
}

fn parse_qty_uom(s: &str) -> (Option<f64>, String) {
    let s = s.trim();
    if s.is_empty() { return (None, String::new()); }
    let mut parts = s.splitn(2, ' ');
    let num_str = parts.next().unwrap_or("").replace(",", "");
    let uom     = parts.next().unwrap_or("").trim().to_string();
    (num_str.parse::<f64>().ok(), uom)
}

fn parse_rate(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() { return None; }
    let num = s.split('/').next().unwrap_or(s).replace(",", "");
    num.parse::<f64>().ok()
}

fn parse_inventory_entries_from_voucher_xml(voucher_xml: &str) -> Vec<InventoryEntryRow> {
    let mut entries = Vec::new();
    for list_tag in &["ALLINVENTORYENTRIES.LIST", "INVENTORYENTRIES.LIST", "INVENTORYALLOCATIONS.LIST"] {
        let open_tag  = format!("<{}", list_tag);
        let close_tag = format!("</{}>", list_tag);
        let mut search_from = 0;

        while let Some(rel_start) = voucher_xml[search_from..].find(&open_tag) {
            let abs_start = search_from + rel_start;
            let tag_end = match voucher_xml[abs_start..].find('>') { Some(p) => abs_start + p + 1, None => break };
            let abs_end = match voucher_xml[tag_end..].find(&close_tag) { Some(p) => tag_end + p + close_tag.len(), None => break };
            let entry_xml = &voucher_xml[tag_end..abs_end - close_tag.len()];
            let stock_item_name = extract_xml_value(entry_xml, "STOCKITEMNAME");
            if stock_item_name.is_empty() { search_from = abs_end; continue; }
            let qty_str = { let b = extract_xml_value(entry_xml, "BILLEDQTY"); if !b.is_empty() { b } else { extract_xml_value(entry_xml, "ACTUALQTY") } };
            let (quantity, uom) = parse_qty_uom(&qty_str);
            entries.push(InventoryEntryRow {
                stock_item_name,
                quantity,
                rate:   parse_rate(&extract_xml_value(entry_xml, "RATE")),
                amount: parse_balance(&extract_xml_value(entry_xml, "AMOUNT")),
                uom,
                godown: extract_xml_value(entry_xml, "GODOWNNAME"),
                batch:  extract_xml_value(entry_xml, "BATCHNAME"),
            });
            search_from = abs_end;
        }
        if !entries.is_empty() { break; }
    }
    entries
}

fn parse_inventory_entries_for_guid(raw_xml: &str, guid: &str) -> Vec<InventoryEntryRow> {
    if guid.is_empty() { return Vec::new(); }
    let guid_pos = match raw_xml.find(guid) { Some(p) => p, None => return Vec::new() };
    let voucher_start = match raw_xml[..guid_pos].rfind("<VOUCHER") { Some(p) => p, None => return Vec::new() };
    let voucher_end = match raw_xml[voucher_start..].find("</VOUCHER>") {
        Some(p) => voucher_start + p + "</VOUCHER>".len(),
        None => return Vec::new(),
    };
    parse_inventory_entries_from_voucher_xml(&raw_xml[voucher_start..voucher_end])
}

// ── JSON output structs ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct LedgerRow {
    name: String,
    parent: String,
    primary_group: String,
    opening_balance: Option<f64>,
    closing_balance: Option<f64>,
    party_gstin: String,
    gst_registration_type: String,
    state: String,
    pin_code: String,
    email: String,
    mobile: String,
    address: String,
    mailing_name: String,
    guid: String,
}

#[derive(Debug, Serialize, Clone)]
struct LedgerEntryRow {
    ledger_name: String,
    amount: Option<f64>,
    is_debit: bool,
}

#[derive(Debug, Serialize)]
struct VoucherRow {
    date: String,
    voucher_type: String,
    voucher_number: String,
    party_name: String,
    amount: Option<f64>,
    narration: String,
    guid: String,
    alter_id: String,
    ledger_entries: Vec<LedgerEntryRow>,
}

// ── Query params ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct CompanyQuery {
    company: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VoucherQuery {
    company: Option<String>,
    from_date: Option<String>,
    to_date: Option<String>,
    voucher_type: Option<String>,
    step: Option<String>,
    group: Option<String>,
    client_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

// ── Push voucher structs ─────────────────────────────────────────────────────
// FIX: Accept both string and object for company field (handles frontend bug gracefully)

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CompanyField {
    Name(String),
    Object(serde_json::Value),
}

impl CompanyField {
    fn into_string(self) -> Option<String> {
        match self {
            CompanyField::Name(s) => if s.is_empty() { None } else { Some(s) },
            CompanyField::Object(v) => {
                v.get("name")
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty())
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct PushLedgerEntry {
    ledger_name: String,
    amount: f64,
    is_debit: bool,
    #[serde(default)]
    parent_group: Option<String>,
}

// FIX: Accept array of vouchers for batch push
#[derive(Debug, Deserialize)]
struct PushVoucherItem {
    voucher_type: String,
    date: String,
    narration: Option<String>,
    party_ledger: Option<String>,
    voucher_number: Option<String>,
    ledger_entries: Vec<PushLedgerEntry>,
    #[serde(default)]
    auto_create_ledgers: bool,
}

#[derive(Debug, Deserialize)]
struct PushVoucherRequest {
    company: CompanyField,
    // FIX: support both single voucher fields and array of vouchers
    #[serde(default)]
    vouchers: Vec<PushVoucherItem>,
    // Legacy single-voucher fields (kept for backward compat)
    voucher_type: Option<String>,
    date: Option<String>,
    narration: Option<String>,
    party_ledger: Option<String>,
    voucher_number: Option<String>,
    ledger_entries: Option<Vec<PushLedgerEntry>>,
    #[serde(default)]
    auto_create_ledgers: bool,
}

impl PushVoucherRequest {
    /// Normalise into a list of voucher items regardless of whether
    /// the caller used the batch `vouchers` array or the legacy flat fields.
    fn into_items(self) -> (String, Vec<PushVoucherItem>) {
        let company = self.company.into_string().unwrap_or_default();
        let auto_create = self.auto_create_ledgers;

        let items = if !self.vouchers.is_empty() {
            self.vouchers
        } else if let (Some(vtype), Some(date), Some(entries)) =
            (self.voucher_type, self.date, self.ledger_entries)
        {
            vec![PushVoucherItem {
                voucher_type: vtype,
                date,
                narration: self.narration,
                party_ledger: self.party_ledger,
                voucher_number: self.voucher_number,
                ledger_entries: entries,
                auto_create_ledgers: auto_create,
            }]
        } else {
            vec![]
        };

        (company, items)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn clean_xml(raw: &str) -> String {
    raw.chars().filter(|&c| {
        let code = c as u32;
        code == 0x9 || code == 0xA || code == 0xD
            || (code >= 0x20 && code <= 0xD7FF)
            || (code >= 0xE000 && code <= 0xFFFD)
    }).collect()
}

fn parse_balance(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() { return None; }
    let (num, sign) = if s.ends_with("Dr") {
        (s.trim_end_matches("Dr").trim(), -1.0)
    } else if s.ends_with("Cr") {
        (s.trim_end_matches("Cr").trim(), 1.0)
    } else {
        (s, 1.0)
    };
    num.replace(",", "").parse::<f64>().ok().map(|v| v * sign)
}

/// Parse Tally stock item numbers that include unit suffixes.
/// Handles formats like:
///   "-2026.000 K.G = 15472 Bags"  → -2026.0
///   "18.73/K.G"                   → 18.73
///   "6,52,450.000 K.G"            → 652450.0
///   "37946.98"                     → 37946.98
///   "-1222038850"                  → -1222038850.0
fn parse_tally_number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() { return None; }
    // First try parse_balance (works for plain numbers)
    if let Some(v) = parse_balance(s) { return Some(v); }
    // Strip everything after first space or '/' (unit suffix)
    let clean = s.split_whitespace().next().unwrap_or(s);
    let clean = clean.split('/').next().unwrap_or(clean);
    // Also handle '=' separator: take the part before '='
    let clean = clean.split('=').next().unwrap_or(clean).trim();
    // Handle negative sign and strip commas
    let stripped = clean.replace(",", "");
    stripped.parse::<f64>().ok()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&apos;")
}

async fn tally_request(xml: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .tcp_keepalive(None)
        .connection_verbose(true)
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|e| e.to_string())?;

    let res = client
        .post("http://127.0.0.1:9000")
        .header("Content-Type", "text/xml;charset=utf-8")
        .header("Connection", "close")
        .body(xml.to_string())
        .send()
        .await
        .map_err(|e| format!("Cannot connect to Tally on port 9000: {}", e))?;

    let raw = res.text().await.unwrap_or_default();
    let cleaned = clean_xml(&raw);
    if cleaned.contains("<STATUS>0</STATUS>") && !cleaned.contains("<STATUS>1</STATUS>") {
        return Err("Tally rejected the request (STATUS=0). Company may not be open in Tally.".to_string());
    }
    Ok(cleaned)
}

fn company_var(name: &str) -> String {
    format!("<SVCURRENTCOMPANY>{}</SVCURRENTCOMPANY>", name)
}

/// Switch Tally's active period for a multi-year company.
/// Sends a lightweight request to Tally with SVCURRENTDATE set to the target FY,
/// which primes Tally's internal period context without fetching heavy data.
async fn switch_tally_period(company: &str, from_date: &str, _to_date: &str) -> Result<(), String> {
    // Use a minimal Collection request that returns almost no data
    // but still forces Tally to acknowledge the SVCURRENTDATE context.
    let xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER>
    <VERSION>1</VERSION>
    <TALLYREQUEST>Export</TALLYREQUEST>
    <TYPE>Collection</TYPE>
    <ID>PeriodPrime</ID>
  </HEADER>
  <BODY>
    <DESC>
      <STATICVARIABLES>
        <SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>
        <SVCURRENTCOMPANY>{company}</SVCURRENTCOMPANY>
        <SVCURRENTDATE>{from_date}</SVCURRENTDATE>
        <SVFROMDATE>{from_date}</SVFROMDATE>
        <SVTODATE>{from_date}</SVTODATE>
      </STATICVARIABLES>
      <TDL>
        <TDLMESSAGE>
          <COLLECTION NAME="PeriodPrime" ISMODIFY="No">
            <TYPE>Company</TYPE>
            <NATIVEMETHOD>Name</NATIVEMETHOD>
          </COLLECTION>
        </TDLMESSAGE>
      </TDL>
    </DESC>
  </BODY>
</ENVELOPE>"#, company = company, from_date = from_date);

    eprintln!("switch_tally_period: priming Tally for {} starting {}", company, from_date);
    match tally_request(&xml).await {
        Ok(resp) => {
            eprintln!("switch_tally_period: priming OK, response len={}", resp.len());
            Ok(())
        },
        Err(e) => {
            eprintln!("switch_tally_period: priming failed (non-fatal): {}", e);
            Ok(())
        }
    }
}

fn now_str() -> String {
    Local::now().format("%d-%m-%Y %H:%M").to_string()
}

fn to_tally_date(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() { return String::new(); }
    if s.len() == 8 && s.chars().all(|c| c.is_ascii_digit()) { return s.to_string(); }

    let months = ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"];
    let parts: Vec<&str> = s.splitn(3, '-').collect();
    if parts.len() == 3 {
        let p0 = parts[0].trim();
        let p1 = parts[1].trim();
        let p2 = parts[2].trim();
        let p1_lower = p1.to_lowercase();
        if let Some(mi) = months.iter().position(|&m| m == p1_lower.as_str()) {
            if let (Ok(d), Ok(y)) = (p0.parse::<u32>(), p2.parse::<u32>()) {
                return format!("{:04}{:02}{:02}", y, mi + 1, d);
            }
        }
        if let (Ok(n0), Ok(n1), Ok(n2)) = (p0.parse::<u32>(), p1.parse::<u32>(), p2.parse::<u32>()) {
            if n0 > 31 { return format!("{:04}{:02}{:02}", n0, n1, n2); }
            if n2 > 31 { return format!("{:04}{:02}{:02}", n2, n1, n0); }
        }
    }
    String::new()
}

fn current_fy() -> (String, String) {
    let now = Local::now();
    let year = now.year();
    let fy_start_year = if now.month() < 4 { year - 1 } else { year };
    (
        format!("{:04}0401", fy_start_year),
        format!("{:04}0331", fy_start_year + 1),
    )
}

fn is_valid_company_name(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() { return false; }
    if s.starts_with('<') { return false; }
    if s.parse::<f64>().is_ok() { return false; }
    if s.len() < 2 { return false; }
    true
}

fn extract_company_names(xml: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in xml.lines() {
        let line = line.trim();
        let upper = line.to_uppercase();
        if upper.contains("<COMPANY") || upper.contains("<BASICCOMPANYNAME>") {
            if let Some(pos) = upper.find("NAME=\"") {
                let rest = &line[pos + 6..];
                if let Some(end) = rest.find('"') {
                    let name = rest[..end].trim().to_string();
                    if is_valid_company_name(&name) && !names.contains(&name) {
                        names.push(name);
                        continue;
                    }
                }
            }
            for (open_tag, close_tag) in &[
                ("<COMPANY>", "</COMPANY>"),
                ("<BASICCOMPANYNAME>", "</BASICCOMPANYNAME>"),
            ] {
                let open_u  = open_tag.to_uppercase();
                let close_u = close_tag.to_uppercase();
                if let Some(start) = upper.find(&open_u) {
                    let after = &line[start + open_tag.len()..];
                    if let Some(end) = after.to_uppercase().find(&close_u) {
                        let name = after[..end].trim().to_string();
                        if is_valid_company_name(&name) && !names.contains(&name) {
                            names.push(name);
                        }
                    }
                }
            }
        }
    }
    names
}

fn voucher_export_xml(company: &str, from_date: &str, to_date: &str, voucher_type: Option<&str>) -> String {
    let (filter_tag, system_tag) = match voucher_type {
        Some(vt) => (
            String::from("\n            <FILTER>VchTypeFilter</FILTER>"),
            format!("\n          <SYSTEM TYPE=\"Formulae\" NAME=\"VchTypeFilter\">$VoucherTypeName = \"{}\"</SYSTEM>", vt),
        ),
        None => (String::new(), String::new()),
    };
    format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER>
    <VERSION>1</VERSION>
    <TALLYREQUEST>Export</TALLYREQUEST>
    <TYPE>Collection</TYPE>
    <ID>AllVouchers</ID>
  </HEADER>
  <BODY>
    <DESC>
      <STATICVARIABLES>
        <SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>
        {cvar}
        <SVFROMDATE>{from_date}</SVFROMDATE>
        <SVTODATE>{to_date}</SVTODATE>
      </STATICVARIABLES>
      <TDL>
        <TDLMESSAGE>
          <COLLECTION NAME="AllVouchers" ISMODIFY="No">
            <TYPE>Voucher</TYPE>
            <FILTER>DateFromFilter</FILTER>
            <FILTER>DateToFilter</FILTER>{filter_tag}
            <NATIVEMETHOD>*</NATIVEMETHOD>
            <NATIVEMETHOD>AllLedgerEntries</NATIVEMETHOD>
            <NATIVEMETHOD>AllInventoryEntries</NATIVEMETHOD>
            <NATIVEMETHOD>LedgerEntries</NATIVEMETHOD>
            <NATIVEMETHOD>InventoryEntries</NATIVEMETHOD>
            <FETCH>AllLedgerEntries</FETCH>
            <FETCH>AllInventoryEntries</FETCH>
            <FETCH>LedgerEntries</FETCH>
            <FETCH>InventoryEntries</FETCH>
          </COLLECTION>
          <SYSTEM TYPE="Formulae" NAME="DateFromFilter">$Date >= $$Date:"{from_date}"</SYSTEM>
          <SYSTEM TYPE="Formulae" NAME="DateToFilter">NOT $Date > $$Date:"{to_date}"</SYSTEM>{system_tag}
        </TDLMESSAGE>
      </TDL>
    </DESC>
  </BODY>
</ENVELOPE>"#,
        cvar = company_var(company),
        from_date = from_date,
        to_date = to_date,
        filter_tag = filter_tag,
        system_tag = system_tag,
    )
}

// ── Build Tally import XML ───────────────────────────────────────────────────
// Returns (optional ledger-creation XML, voucher XML).
// When auto_create_ledgers is true, ledger XML must be sent to Tally FIRST.

fn build_ledger_create_xml(company: &str, item: &PushVoucherItem) -> Option<String> {
    if !item.auto_create_ledgers { return None; }

    let mut ledger_xml = String::new();
    for entry in &item.ledger_entries {
        let parent = entry.parent_group
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("Indirect Expenses");

        ledger_xml.push_str(&format!(
            r#"
          <LEDGER NAME="{ledger}" ACTION="Create">
            <NAME.LIST>
              <NAME>{ledger}</NAME>
            </NAME.LIST>
            <PARENT>{parent}</PARENT>
            <ISBILLWISEON>No</ISBILLWISEON>
            <ISCOSTCENTRESON>No</ISCOSTCENTRESON>
          </LEDGER>"#,
            ledger = xml_escape(&entry.ledger_name),
            parent = xml_escape(parent),
        ));
    }

    if ledger_xml.is_empty() { return None; }

    Some(format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER>
    <TALLYREQUEST>Import Data</TALLYREQUEST>
  </HEADER>
  <BODY>
    <IMPORTDATA>
      <REQUESTDESC>
        <REPORTNAME>All Masters</REPORTNAME>
        <STATICVARIABLES>
          {cvar}
        </STATICVARIABLES>
      </REQUESTDESC>
      <REQUESTDATA>
        <TALLYMESSAGE xmlns:UDF="TallyUDF">
          {ledgers}
        </TALLYMESSAGE>
      </REQUESTDATA>
    </IMPORTDATA>
  </BODY>
</ENVELOPE>"#,
        cvar = company_var(company),
        ledgers = ledger_xml,
    ))
}

fn build_voucher_xml(company: &str, item: &PushVoucherItem) -> String {
    let date = to_tally_date(&item.date);
    let narration = item.narration.as_deref().unwrap_or("");
    let vch_type = &item.voucher_type;

    let mut entries_xml = String::new();
    for entry in &item.ledger_entries {
        let tally_amount = if entry.is_debit { -entry.amount.abs() } else { entry.amount.abs() };
        let deemed = if entry.is_debit { "Yes" } else { "No" };

        entries_xml.push_str(&format!(
            r#"
            <ALLLEDGERENTRIES.LIST>
              <LEDGERNAME>{ledger}</LEDGERNAME>
              <ISDEEMEDPOSITIVE>{deemed}</ISDEEMEDPOSITIVE>
              <AMOUNT>{amount:.2}</AMOUNT>
            </ALLLEDGERENTRIES.LIST>"#,
            ledger = xml_escape(&entry.ledger_name),
            deemed = deemed,
            amount = tally_amount,
        ));
    }

    let party_xml = match &item.party_ledger {
        Some(p) if !p.is_empty() => format!("<PARTYLEDGERNAME>{}</PARTYLEDGERNAME>", xml_escape(p)),
        _ => String::new(),
    };
    let vchnum_xml = match &item.voucher_number {
        Some(n) if !n.is_empty() => format!("<VOUCHERNUMBER>{}</VOUCHERNUMBER>", xml_escape(n)),
        _ => String::new(),
    };

    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER>
    <TALLYREQUEST>Import Data</TALLYREQUEST>
  </HEADER>
  <BODY>
    <IMPORTDATA>
      <REQUESTDESC>
        <REPORTNAME>Vouchers</REPORTNAME>
        <STATICVARIABLES>
          {cvar}
        </STATICVARIABLES>
      </REQUESTDESC>
      <REQUESTDATA>
        <TALLYMESSAGE xmlns:UDF="TallyUDF">
          <VOUCHER REMOTEID="" VCHTYPE="{vtype}" ACTION="Create" DATE="{date}">
            <DATE>{date}</DATE>
            <VOUCHERTYPENAME>{vtype}</VOUCHERTYPENAME>
            {party}
            {vchnum}
            <NARRATION>{narration}</NARRATION>
            {entries}
          </VOUCHER>
        </TALLYMESSAGE>
      </REQUESTDATA>
    </IMPORTDATA>
  </BODY>
</ENVELOPE>"#,
        cvar = company_var(company),
        vtype = xml_escape(vch_type),
        date = date,
        party = party_xml,
        vchnum = vchnum_xml,
        narration = xml_escape(narration),
        entries = entries_xml,
    )
}

// ── Supabase helpers ─────────────────────────────────────────────────────────

fn make_http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .pool_max_idle_per_host(0)
        .build()
        .unwrap_or_default()
}

async fn supabase_upsert(
    http: &reqwest::Client,
    table: &str,
    on_conflict: &str,
    rows: &[serde_json::Value],
) -> Result<(), String> {
    if rows.is_empty() { return Ok(()); }

    for chunk in rows.chunks(500) {
        let url = format!("{}/rest/v1/{}?on_conflict={}", SUPABASE_URL, table, on_conflict);
        let resp = http
            .post(&url)
            .header("apikey", SUPABASE_KEY)
            .header("Authorization", format!("Bearer {}", SUPABASE_KEY))
            .header("Content-Type", "application/json")
            .header("Prefer", "resolution=merge-duplicates,return=minimal")
            .json(&chunk)
            .send()
            .await
            .map_err(|e| format!("Request to {} failed: {}", table, e))?;

        let status = resp.status().as_u16();
        if status >= 300 {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Supabase {} HTTP {}: {}", table, status, &body[..body.len().min(400)]));
        }
    }
    Ok(())
}

// ── Supabase cleanup helpers ─────────────────────────────────────────────────

async fn supabase_fetch_guids(
    http: &reqwest::Client,
    company: &str,
    from_date: Option<&str>,
    to_date: Option<&str>,
) -> Result<Vec<String>, String> {
    let url = format!("{}/rest/v1/vouchers", SUPABASE_URL);
    let mut query_params: Vec<(&str, String)> = vec![
        ("select", "guid".to_string()),
        ("company_name", format!("eq.{}", company)),
    ];
    // Scope cleanup to only the synced date range so other FYs are untouched
    if let (Some(fd), Some(td)) = (from_date, to_date) {
        query_params.push(("and", format!("(date.gte.{},date.lte.{})", fd, td)));
        eprintln!("supabase_fetch_guids: scoped to date range {}..{}", fd, td);
    }
    let query_refs: Vec<(&str, &str)> = query_params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let resp = http.get(&url)
        .header("apikey", SUPABASE_KEY)
        .header("Authorization", format!("Bearer {}", SUPABASE_KEY))
        .header("Range", "0-99999")
        .query(&query_refs)
        .send().await
        .map_err(|e| format!("Failed to fetch existing GUIDs: {}", e))?;

    let text = resp.text().await.unwrap_or_default();
    let rows: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
    eprintln!("supabase_fetch_guids: found {} existing vouchers in DB for cleanup comparison", rows.len());
    Ok(rows.iter().filter_map(|r| r["guid"].as_str().map(|s| s.to_string())).collect())
}

async fn supabase_delete_removed(
    http: &reqwest::Client,
    table: &str,
    guid_column: &str,
    company: &str,
    guids_to_delete: &[String],
) -> Result<usize, String> {
    if guids_to_delete.is_empty() { return Ok(0); }
    let mut deleted = 0;
    for chunk in guids_to_delete.chunks(50) {
        let guid_list = chunk.iter()
            .map(|g| format!("\"{}\"", g))
            .collect::<Vec<_>>()
            .join(",");
        let url = format!("{}/rest/v1/{}", SUPABASE_URL, table);
        let resp = http.delete(&url)
            .header("apikey", SUPABASE_KEY)
            .header("Authorization", format!("Bearer {}", SUPABASE_KEY))
            .query(&[
                ("company_name", &format!("eq.{}", company) as &str),
                (guid_column, &format!("in.({})", guid_list) as &str),
            ])
            .send().await
            .map_err(|e| format!("Delete from {} failed: {}", table, e))?;

        let status = resp.status().as_u16();
        if status >= 300 {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("{} delete HTTP {}: {}", table, status, &body[..body.len().min(400)]));
        }
        deleted += chunk.len();
    }
    Ok(deleted)
}

// ── Routes ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let state: SharedState = Arc::new(Mutex::new(HashMap::new()));

    tokio::spawn({
        let state = state.clone();
        async move {
            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers(Any);

            let app = Router::new()
                .route("/health",                    get(health))
                .route("/tally/companies",            get(fetch_companies))
                .route("/tally/available-companies",  get(available_companies))
                .route("/tally/add-company",          get(add_company))
                .route("/tally/sync-ledgers",         get(sync_ledgers))
                .route("/tally/list-groups",           get(list_groups))
                .route("/tally/sync-vouchers",        get(sync_vouchers))
                .route("/tally/push-voucher",         post(push_voucher))
                .route("/tally/debug-companies",      get(debug_companies))
                .route("/tally/test-post",            get(test_post))
                .route("/tally/debug-ledgers",        get(debug_ledgers))
                .route("/tally/debug-vouchers",       get(debug_vouchers))
                .route("/tally/debug-stock-items",    get(debug_stock_items))
                .layer(cors)
                .with_state(state);

            let listener = tokio::net::TcpListener::bind("127.0.0.1:17890")
                .await
                .expect("Failed to bind to 127.0.0.1:17890");

            println!("✓ SuvidhaAI connector on http://127.0.0.1:17890");
            axum::serve(listener, app).await.expect("Server crashed");
        }
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ── GET /health ──────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    let tally_ok = reqwest::Client::new()
        .get("http://127.0.0.1:9000")
        .send()
        .await
        .is_ok();

    (StatusCode::OK, [("Content-Type", "application/json")],
        json!({ "ok": true, "tally_connected": tally_ok, "service": "SuvidhaAI Connector", "version": "1.3.0" }).to_string())
}

// ── GET /tally/companies ─────────────────────────────────────────────────────

async fn fetch_companies(State(state): State<SharedState>) -> impl IntoResponse {
    // ── 1. Query Tally for companies in THIS installation ────────────
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Companies</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT></STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="List of Companies" ISMODIFY="No">
    <TYPE>Company</TYPE><NATIVEMETHOD>Name</NATIVEMETHOD><NATIVEMETHOD>BasicCompanyName</NATIVEMETHOD>
    <NATIVEMETHOD>GUID</NATIVEMETHOD><NATIVEMETHOD>StartingFrom</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#;

    let tally_names: Vec<String> = match tally_request(xml).await {
        Ok(x) => extract_company_names(&x),
        Err(e) => return (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "TALLY_CONNECTION_FAILED", "details": e }).to_string()),
    };

    if tally_names.is_empty() {
        return (StatusCode::OK, [("Content-Type", "application/json")],
            json!({ "ok": true, "companies": [] }).to_string());
    }

    // ── 2. Check Supabase for which Tally companies have been synced ─
    // Query BOTH ledgers and stock_items tables (some companies may only
    // have stock items synced but no ledgers yet, like RUDRAM)
    let http = make_http();
    let mut synced_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();

    // Build filter: company_name=in.("RUDRAM INC 2024-26","OTHER CO")
    let name_filter = tally_names.iter()
        .map(|n| format!("\"{}\"", n))
        .collect::<Vec<_>>()
        .join(",");
    let filter_param = format!("in.({})", name_filter);

    // Check ledgers table
    if let Ok(r) = http.get(format!(
            "{}/rest/v1/ledgers?select=company_name,synced_at&company_name={}&order=synced_at.desc&limit=500",
            SUPABASE_URL, filter_param))
        .header("apikey", SUPABASE_KEY)
        .header("Authorization", format!("Bearer {}", SUPABASE_KEY))
        .send().await
    {
        let status = r.status().as_u16();
        let text = r.text().await.unwrap_or_default();
        if status < 300 {
            if let Ok(rows) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                for row in &rows {
                    let name = row["company_name"].as_str().unwrap_or("").to_string();
                    let ts = row["synced_at"].as_str().unwrap_or("").to_string();
                    if !name.is_empty() { synced_map.entry(name).or_insert(ts); }
                }
            }
        } else {
            eprintln!("Supabase ledgers query failed (HTTP {}): {}", status, &text[..text.len().min(200)]);
        }
    }

    // Also check stock_items table (catches companies where only master stock was synced)
    if let Ok(r) = http.get(format!(
            "{}/rest/v1/stock_items?select=company_name,synced_at&company_name={}&order=synced_at.desc&limit=500",
            SUPABASE_URL, filter_param))
        .header("apikey", SUPABASE_KEY)
        .header("Authorization", format!("Bearer {}", SUPABASE_KEY))
        .send().await
    {
        let status = r.status().as_u16();
        let text = r.text().await.unwrap_or_default();
        if status < 300 {
            if let Ok(rows) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                for row in &rows {
                    let name = row["company_name"].as_str().unwrap_or("").to_string();
                    let ts = row["synced_at"].as_str().unwrap_or("").to_string();
                    if !name.is_empty() { synced_map.entry(name).or_insert(ts); }
                }
            }
        }
    }

    // Also check vouchers table
    if let Ok(r) = http.get(format!(
            "{}/rest/v1/vouchers?select=company_name,synced_at&company_name={}&order=synced_at.desc&limit=500",
            SUPABASE_URL, filter_param))
        .header("apikey", SUPABASE_KEY)
        .header("Authorization", format!("Bearer {}", SUPABASE_KEY))
        .send().await
    {
        let status = r.status().as_u16();
        let text = r.text().await.unwrap_or_default();
        if status < 300 {
            if let Ok(rows) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                for row in &rows {
                    let name = row["company_name"].as_str().unwrap_or("").to_string();
                    let ts = row["synced_at"].as_str().unwrap_or("").to_string();
                    if !name.is_empty() { synced_map.entry(name).or_insert(ts); }
                }
            }
        }
    }

    eprintln!("Tally has {} companies, {} have been synced to DB", tally_names.len(), synced_map.len());

    // ── 3. Hydrate in-memory state from DB ──────────────────────────
    {
        let mut guard = state.lock().unwrap();
        for (name, ts) in &synced_map {
            let entry = guard.entry(name.clone()).or_insert_with(|| CompanyState {
                name: name.clone(),
                connected: false,
                last_master_sync: None,
                last_voucher_sync: None,
            });
            entry.connected = true;
            if entry.last_master_sync.is_none() && !ts.is_empty() {
                let display_ts = if ts.len() >= 16 {
                    format!("{}-{}-{} {}",
                        &ts[8..10], &ts[5..7], &ts[0..4], &ts[11..16])
                } else {
                    ts.clone()
                };
                entry.last_master_sync = Some(display_ts.clone());
                if entry.last_voucher_sync.is_none() {
                    entry.last_voucher_sync = Some(display_ts);
                }
            }
        }
    }

    // ── 4. Build response — only companies from THIS Tally ───────────
    let guard = state.lock().unwrap();
    let companies: Vec<serde_json::Value> = tally_names.iter().map(|name| {
        let is_synced = synced_map.contains_key(name);
        let cs = guard.get(name);
        json!({
            "name": name,
            "connected": is_synced || cs.map(|c| c.connected).unwrap_or(false),
            "activeInTally": true,
            "lastMasterSync": cs.and_then(|c| c.last_master_sync.clone()),
            "lastVoucherSync": cs.and_then(|c| c.last_voucher_sync.clone()),
        })
    }).collect();

    (StatusCode::OK, [("Content-Type", "application/json")],
        json!({ "ok": true, "companies": companies }).to_string())
}

// ── GET /tally/list-groups ───────────────────────────────────────────────────

async fn list_groups(
    Query(params): Query<CompanyQuery>,
) -> impl IntoResponse {
    let company = match params.company {
        Some(n) if !n.is_empty() => n,
        _ => return (StatusCode::BAD_REQUEST, [(("Content-Type", "application/json"))],
            json!({ "ok": false, "error": "Missing ?company= param" }).to_string()),
    };

    // Fetch group names from Tally — lightweight query, no balances
    let xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>ListOfGroups</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>{cvar}</STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="ListOfGroups" ISMODIFY="No">
    <TYPE>Group</TYPE><NATIVEMETHOD>Name</NATIVEMETHOD><NATIVEMETHOD>Parent</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#, cvar = company_var(&company));

    match tally_request(&xml).await {
        Ok(response) => {
            let mut groups = Vec::new();
            let mut pos = 0;
            // Parse <GROUP NAME="..."> or <GROUP>...<NAME>...</NAME>
            while pos < response.len() {
                let group_start = if let Some(p) = response[pos..].find("<GROUP ") {
                    let p2 = response[pos..].find("<GROUP>").unwrap_or(usize::MAX);
                    pos + p.min(p2)
                } else if let Some(p) = response[pos..].find("<GROUP>") {
                    pos + p
                } else {
                    break;
                };
                let end = match response[group_start..].find("</GROUP>") {
                    Some(p) => group_start + p + 8,
                    None => break,
                };
                let chunk = &response[group_start..end];

                // Extract NAME
                let gname = {
                    let mut n = String::new();
                    if let Some(nstart) = chunk.find("NAME=\"") {
                        let after = &chunk[nstart + 6..];
                        if let Some(nend) = after.find('"') {
                            n = after[..nend].to_string();
                        }
                    }
                    if n.is_empty() {
                        n = extract_xml_value(chunk, "NAME");
                    }
                    n
                };

                // Extract PARENT
                let parent = extract_xml_value(chunk, "PARENT");

                if !gname.is_empty() {
                    groups.push(json!({ "name": gname, "parent": parent }));
                }
                pos = end;
            }

            eprintln!("list_groups: found {} groups for {}", groups.len(), company);
            (StatusCode::OK, [(("Content-Type", "application/json"))],
                json!({ "ok": true, "groups": groups, "count": groups.len() }).to_string())
        },
        Err(e) => {
            (StatusCode::BAD_GATEWAY, [(("Content-Type", "application/json"))],
                json!({ "ok": false, "error": e }).to_string())
        },
    }
}

// ── GET /tally/sync-ledgers ──────────────────────────────────────────────────

async fn sync_ledgers(
    Query(params): Query<VoucherQuery>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let company = match params.company {
        Some(n) if !n.is_empty() => n,
        _ => return (StatusCode::BAD_REQUEST, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "Missing ?company= param" }).to_string()),
    };

    // Date range for period-specific balances (Tally computes OB/CB for this period)
    let (default_from, default_to) = current_fy();
    let from_date = to_tally_date(&params.from_date.unwrap_or(default_from));
    let to_date   = to_tally_date(&params.to_date.unwrap_or(default_to));
    let step = params.step.unwrap_or_default();
    eprintln!("sync_ledgers: company={}, from_date={}, to_date={}, step={}", company, from_date, to_date, step);
    let sync_start = std::time::Instant::now();

    // Compute FY label from from_date, e.g. "20250401" → "2025-26"
    let fy_period = {
        // from_date is in Tally format like "1-Apr-2025" or we stored it as YYYYMMDD then converted
        // We need to extract the year from the from_date parameter (before to_tally_date conversion)
        // Actually from_date is already in tally format: "1-Apr-2025"
        // Let's extract year from the tally-format date
        let fy_year = if from_date.contains('-') {
            // Tally format: "1-Apr-2025" → extract last 4 chars
            from_date.split('-').last().unwrap_or("2026").trim().to_string()
        } else {
            // YYYYMMDD format: take first 4
            from_date[..4.min(from_date.len())].to_string()
        };
        let y: i32 = fy_year.parse().unwrap_or(2026);
        let short_next = format!("{:02}", (y + 1) % 100);
        format!("{}-{}", y, short_next)
    };
    eprintln!("sync_ledgers: fy_period={}", fy_period);

    let sync_time = now_str();

    // When step=stock_items, skip groups + ledgers entirely
    let has_group_filter = params.group.as_ref().map_or(false, |g| !g.is_empty());
    let (count, group_count) = if step != "stock_items" {

    // ── Step 1: Fetch group hierarchy from Tally ─────────────────────
    // Skip when per-group mode — the parent is used directly
    let group_primary: HashMap<String, String> = if has_group_filter {
        eprintln!("sync_ledgers: skipping group hierarchy (per-group mode)");
        HashMap::new()
    } else {
    let group_xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Groups</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>{cvar}</STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="List of Groups" ISMODIFY="No">
    <TYPE>Group</TYPE><NATIVEMETHOD>Name</NATIVEMETHOD><NATIVEMETHOD>Parent</NATIVEMETHOD><NATIVEMETHOD>PrimaryGroup</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#, cvar = company_var(&company));

    // Build group_name -> primary_group map using Tally's PRIMARYGRPPARENT
    // Tally XML format: <GROUP NAME="X"><PARENT TYPE="String">Y</PARENT>
    //   <PRIMARYGRPPARENT TYPE="String">Z</PRIMARYGRPPARENT></GROUP>
    eprintln!("sync_ledgers: [1/5] Fetching groups from Tally...");
    match tally_request(&group_xml).await {
        Ok(xml) => {
            // Debug: show what Tally returns
            eprintln!("  Groups XML preview ({} bytes): {}", xml.len(), &xml[..xml.len().min(800)]);
            let mut map = HashMap::new();
            let mut pos = 0;
            // Match both <GROUP NAME="..."> and <GROUP> (no attributes)
            while pos < xml.len() {
                // Find next <GROUP tag (with or without attributes)
                let group_start = if let Some(p) = xml[pos..].find("<GROUP ") {
                    let p2 = xml[pos..].find("<GROUP>").unwrap_or(usize::MAX);
                    pos + p.min(p2)
                } else if let Some(p) = xml[pos..].find("<GROUP>") {
                    pos + p
                } else {
                    break;
                };
                let end = match xml[group_start..].find("</GROUP>") {
                    Some(p) => group_start + p + 8,
                    None => break,
                };
                let chunk = &xml[group_start..end];

                // Extract group name: try NAME="..." attribute first, then <NAME> element
                let gname = {
                    let mut n = String::new();
                    if let Some(nstart) = chunk.find("NAME=\"") {
                        let after = &chunk[nstart + 6..];
                        if let Some(nend) = after.find('"') {
                            n = after[..nend].to_string();
                        }
                    }
                    if n.is_empty() {
                        n = extract_xml_value(chunk, "NAME");
                    }
                    n
                };

                // Extract primary group: try PRIMARYGRPPARENT, PRIMARYGROUP, PARENT
                let primary = {
                    let mut p = String::new();
                    if let Some(pstart) = chunk.find("<PRIMARYGRPPARENT") {
                        let after = &chunk[pstart..];
                        if let Some(gt) = after.find('>') {
                            let content = &after[gt + 1..];
                            if let Some(close) = content.find("</PRIMARYGRPPARENT>") {
                                p = content[..close].trim().to_string();
                            }
                        }
                    }
                    if p.is_empty() {
                        p = extract_xml_value(chunk, "PRIMARYGROUP");
                    }
                    if p.is_empty() {
                        let parent_val = extract_xml_value(chunk, "PARENT");
                        if !parent_val.is_empty() {
                            p = parent_val;
                        }
                    }
                    p
                };

                if !gname.is_empty() && !primary.is_empty() {
                    map.insert(gname, primary);
                }
                pos = end;
            }
            eprintln!("Parsed {} group->primary_group mappings from Tally", map.len());
            map
        },
        Err(e) => {
            eprintln!("Warning: failed to fetch groups from Tally: {}", e);
            HashMap::new()
        },
    }
    }; // end of if has_group_filter / else

    // Resolve primary group for a ledger's parent
    let resolve_primary = |parent: &str| -> String {
        // Look up the ledger's immediate parent in the group map
        if let Some(pg) = group_primary.get(parent) {
            return pg.clone();
        }
        // If the parent itself is a primary group (not in map), return it as-is
        parent.to_string()
    };

    // ── Step 2: Fetch ledgers from Tally ─────────────────────────────
    // SVFROMDATE/SVTODATE tell Tally to compute OB/CB for this period
    // When group param is provided, use CHILDOF filter to fetch only that group's ledgers
    let group_filter = match &params.group {
        Some(g) if !g.is_empty() => format!("<CHILDOF>{}</CHILDOF>", g),
        _ => String::new(),
    };
    let group_label = params.group.as_deref().unwrap_or("ALL");
    let xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Ledgers</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>{cvar}
    <SVFROMDATE>{from_date}</SVFROMDATE><SVTODATE>{to_date}</SVTODATE>
  </STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="List of Ledgers" ISMODIFY="No">
    <TYPE>Ledger</TYPE>{group_filter}<NATIVEMETHOD>Name</NATIVEMETHOD><NATIVEMETHOD>Parent</NATIVEMETHOD>
    <NATIVEMETHOD>OpeningBalance</NATIVEMETHOD><NATIVEMETHOD>ClosingBalance</NATIVEMETHOD>
    <NATIVEMETHOD>PartyGSTIN</NATIVEMETHOD><NATIVEMETHOD>GSTRegistrationType</NATIVEMETHOD>
    <NATIVEMETHOD>LedStateName</NATIVEMETHOD><NATIVEMETHOD>PinCode</NATIVEMETHOD>
    <NATIVEMETHOD>Email</NATIVEMETHOD><NATIVEMETHOD>LedgerMobile</NATIVEMETHOD>
    <NATIVEMETHOD>Address</NATIVEMETHOD><NATIVEMETHOD>MailingName</NATIVEMETHOD>
    <NATIVEMETHOD>GUID</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#, cvar = company_var(&company), from_date = from_date, to_date = to_date, group_filter = group_filter);

    eprintln!("sync_ledgers: [2/5] Fetching ledgers [group={}] from Tally... ({:.1}s elapsed)", group_label, sync_start.elapsed().as_secs_f64());
    let cleaned = match tally_request(&xml).await {
        Ok(x) => x,
        Err(e) => return (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "TALLY_CONNECTION_FAILED", "details": e }).to_string()),
    };

    let envelope: TallyEnvelope = match from_str(&cleaned) {
        Ok(e) => e,
        Err(e) => return (StatusCode::UNPROCESSABLE_ENTITY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "XML_PARSE_FAILED", "details": format!("{}", e) }).to_string()),
    };

    let rows: Vec<LedgerRow> = envelope.body.data.collection.ledgers.iter().map(|l| {
        let parent = l.parent.clone();
        let primary = resolve_primary(&parent);
        LedgerRow {
            name:                  l.name().to_string(),
            parent:                parent,
            primary_group:         primary,
            opening_balance:       parse_balance(&l.opening_balance),
            closing_balance:       parse_balance(&l.closing_balance),
            party_gstin:           l.party_gstin.clone(),
            gst_registration_type: l.gst_registration_type.clone(),
            state:                 l.state.clone(),
            pin_code:              l.pin_code.clone(),
            email:                 l.email.clone(),
            mobile:                l.mobile.clone(),
            address:               l.address.clone(),
            mailing_name:          l.mailing_name.clone(),
            guid:                  l.guid.clone(),
        }
    }).collect();

    let _count = rows.len();
    let _group_count = group_primary.len();

    {
        let mut guard = state.lock().unwrap();
        let entry = guard.entry(company.clone()).or_insert(CompanyState {
            name: company.clone(), connected: false,
            last_master_sync: None, last_voucher_sync: None,
        });
        entry.connected = true;
        entry.last_master_sync = Some(sync_time.clone());
    }

    if !rows.is_empty() {
        let http = make_http();
        let client_id = params.client_id.as_deref().unwrap_or("");
        let upsert_rows: Vec<serde_json::Value> = rows.iter().map(|r| json!({
            "company_name":           company.trim(),
            "name":                   r.name.trim().replace("\r\n", "").replace("\r", "").replace("\n", ""),
            "parent":                 r.parent.trim(),
            "primary_group":          r.primary_group.trim(),
            "opening_balance":        r.opening_balance,
            "closing_balance":        r.closing_balance,
            "party_gstin":            r.party_gstin.trim(),
            "gst_registration_type":  r.gst_registration_type.trim(),
            "state":                  r.state.trim(),
            "pin_code":               r.pin_code.trim(),
            "email":                  r.email.trim(),
            "mobile":                 r.mobile.trim(),
            "address":                r.address.trim(),
            "mailing_name":           r.mailing_name.trim().replace("\r\n", "").replace("\r", "").replace("\n", ""),
            "fy_period":              fy_period,
            "guid":                   r.guid.trim(),
            "client_id":              client_id,
        })).collect();

        // FIX: surface Supabase errors instead of silently ignoring
        eprintln!("sync_ledgers: [3/5] Upserting {} ledgers to Supabase... ({:.1}s elapsed)", upsert_rows.len(), sync_start.elapsed().as_secs_f64());
        if let Err(e) = supabase_upsert(&http, "ledgers", "company_name,name,fy_period", &upsert_rows).await {
            return (StatusCode::OK, [("Content-Type", "application/json")],
                json!({ "ok": false, "error": "SUPABASE_UPSERT_FAILED", "details": e }).to_string());
        }

        // Skip cleanup in per-group mode (we only have a subset of ledgers)
        if !has_group_filter {
        eprintln!("sync_ledgers: [4/5] Cleaning up stale ledgers... ({:.1}s elapsed)", sync_start.elapsed().as_secs_f64());
        // ── Clean up ledgers deleted from Tally ──────────────────────────
        let synced_names: std::collections::HashSet<String> = rows.iter()
            .map(|r| r.name.clone())
            .collect();

        let existing_url = format!("{}/rest/v1/ledgers", SUPABASE_URL);
        if let Ok(resp) = http.get(&existing_url)
            .header("apikey", SUPABASE_KEY)
            .header("Authorization", format!("Bearer {}", SUPABASE_KEY))
            .header("Range", "0-99999")
            .query(&[("select", "name"), ("company_name", &format!("eq.{}", company) as &str)])
            .send().await
        {
            let text = resp.text().await.unwrap_or_default();
            let existing: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap_or_default();
            let deleted_names: Vec<String> = existing.iter()
                .filter_map(|r| r["name"].as_str().map(|s| s.to_string()))
                .filter(|n| !synced_names.contains(n))
                .collect();

            if !deleted_names.is_empty() {
                let del_count = deleted_names.len();
                for chunk in deleted_names.chunks(50) {
                    let name_list = chunk.iter()
                        .map(|n| format!("\"{}\"", n))
                        .collect::<Vec<_>>()
                        .join(",");
                    let del_url = format!("{}/rest/v1/ledgers", SUPABASE_URL);
                    if let Err(e) = http.delete(&del_url)
                        .header("apikey", SUPABASE_KEY)
                        .header("Authorization", format!("Bearer {}", SUPABASE_KEY))
                        .query(&[
                            ("company_name", &format!("eq.{}", company) as &str),
                            ("name", &format!("in.({})", name_list) as &str),
                        ])
                        .send().await
                    {
                        eprintln!("Warning: failed to delete removed ledgers: {}", e);
                    }
                }
                eprintln!("Cleaned up {} ledgers no longer in Tally for {}", del_count, company);
            }
        }
        } // end if !has_group_filter
    }

    (_count, _group_count)

    } else {
        eprintln!("sync_ledgers: skipping ledger steps (step=stock_items)");
        (0, 0)
    };

    // ── Sync Stock Items from Tally ──────────────────────────────────
    // Skip if step=ledgers (frontend will call again with step=stock_items)
    let stock_count = if step == "ledgers" {
        eprintln!("sync_ledgers: skipping stock items (step=ledgers)");
        0
    } else {
        // Give Tally a breather between heavy requests to prevent crash
        if step.is_empty() {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        eprintln!("sync_ledgers: [5/5] Fetching stock items from Tally... ({:.1}s elapsed)", sync_start.elapsed().as_secs_f64());
        let stock_xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>StockItemSync</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>{cvar}</STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="StockItemSync" ISMODIFY="No">
    <TYPE>Stock Item</TYPE>
    <NATIVEMETHOD>Name</NATIVEMETHOD>
    <NATIVEMETHOD>Parent</NATIVEMETHOD>
    <NATIVEMETHOD>Category</NATIVEMETHOD>
    <NATIVEMETHOD>BaseUnits</NATIVEMETHOD>
    <NATIVEMETHOD>OpeningBalance</NATIVEMETHOD>
    <NATIVEMETHOD>OpeningRate</NATIVEMETHOD>
    <NATIVEMETHOD>OpeningValue</NATIVEMETHOD>
    <NATIVEMETHOD>Description</NATIVEMETHOD>
    <NATIVEMETHOD>GSTDetails</NATIVEMETHOD>
    <NATIVEMETHOD>HSNCode</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#, cvar = company_var(&company));

        match tally_request(&stock_xml).await {
            Ok(xml) => {
                // Log first stock item XML for debugging
                if let Some(first_start) = xml.find("<STOCKITEM") {
                    if let Some(first_end) = xml[first_start..].find("</STOCKITEM>") {
                        let sample = &xml[first_start..first_start + first_end + "</STOCKITEM>".len()];
                        eprintln!("=== SAMPLE STOCK ITEM XML ===\n{}\n=== END SAMPLE ===", &sample[..sample.len().min(2000)]);
                    }
                }

                let mut items = Vec::new();
                let mut pos = 0;
                while let Some(start) = xml[pos..].find("<STOCKITEM") {
                    let abs_start = pos + start;
                    let end = match xml[abs_start..].find("</STOCKITEM>") {
                        Some(p) => abs_start + p + "</STOCKITEM>".len(),
                        None => break,
                    };
                    let chunk = &xml[abs_start..end];

                    let name = {
                        let mut n = String::new();
                        if let Some(nstart) = chunk.find("NAME=\"") {
                            let after = &chunk[nstart + 6..];
                            if let Some(nend) = after.find('"') {
                                n = after[..nend].to_string();
                            }
                        }
                        if n.is_empty() {
                            n = extract_xml_value(chunk, "NAME");
                        }
                        n
                    };

                    if !name.is_empty() {
                        let parent = extract_xml_value(chunk, "PARENT");
                        let category = extract_xml_value(chunk, "CATEGORY");
                        let uom = extract_xml_value(chunk, "BASEUNITS");
                        let opening_qty = parse_tally_number(&extract_xml_value(chunk, "OPENINGBALANCE"));
                        let opening_rate = parse_tally_number(&extract_xml_value(chunk, "OPENINGRATE"));
                        let opening_value = parse_tally_number(&extract_xml_value(chunk, "OPENINGVALUE"));
                        let description = extract_xml_value(chunk, "DESCRIPTION");

                        let (hsn_code, gst_rate) = {
                            let mut hsn = String::new();
                            let mut rate = 0.0_f64;

                            if let Some(gst_start) = chunk.find("<GSTDETAILS.LIST") {
                                let gst_end_tag = "</GSTDETAILS.LIST>";
                                if let Some(gst_end) = chunk[gst_start..].find(gst_end_tag) {
                                    let gst_chunk = &chunk[gst_start..gst_start + gst_end + gst_end_tag.len()];
                                    let h = extract_xml_value(gst_chunk, "HSNCODE");
                                    if !h.is_empty() { hsn = h; }
                                    if hsn.is_empty() {
                                        let h2 = extract_xml_value(gst_chunk, "HSN");
                                        if !h2.is_empty() { hsn = h2; }
                                    }
                                    if hsn.is_empty() {
                                        let h3 = extract_xml_value(gst_chunk, "HSNMASTERNAME");
                                        if !h3.is_empty() { hsn = h3; }
                                    }
                                    {
                                        let mut cgst = 0.0_f64;
                                        let mut sgst = 0.0_f64;
                                        let mut igst = 0.0_f64;
                                        let mut rd_pos = 0usize;
                                        while let Some(rd_start) = gst_chunk[rd_pos..].find("<RATEDETAILS.LIST>") {
                                            let rd_abs = rd_pos + rd_start;
                                            let rd_end_tag = "</RATEDETAILS.LIST>";
                                            if let Some(rd_end) = gst_chunk[rd_abs..].find(rd_end_tag) {
                                                let rd_block = &gst_chunk[rd_abs..rd_abs + rd_end + rd_end_tag.len()];
                                                let duty_head = extract_xml_value(rd_block, "GSTRATEDUTYHEAD");
                                                let rd_rate = parse_balance(&extract_xml_value(rd_block, "GSTRATE")).unwrap_or(0.0);
                                                match duty_head.trim() {
                                                    "IGST" => { igst = rd_rate; },
                                                    "CGST" => { cgst = rd_rate; },
                                                    "SGST/UTGST" | "SGST" => { sgst = rd_rate; },
                                                    _ => {},
                                                }
                                                rd_pos = rd_abs + rd_end + rd_end_tag.len();
                                            } else { break; }
                                        }
                                        if igst > 0.0 {
                                            rate = igst;
                                        } else if cgst > 0.0 || sgst > 0.0 {
                                            rate = cgst + sgst;
                                        }
                                    }
                                }
                            }

                            if hsn.is_empty() {
                                hsn = extract_xml_value(chunk, "HSNCODE");
                            }
                            if hsn.is_empty() {
                                let h = extract_xml_value(chunk, "HSN");
                                if !h.is_empty() { hsn = h; }
                            }
                            if rate == 0.0 {
                                let r = extract_xml_value(chunk, "GSTRATE");
                                if !r.is_empty() { rate = parse_balance(&r).unwrap_or(0.0); }
                            }

                            (hsn, rate)
                        };

                        items.push(json!({
                            "company_name": company.trim(),
                            "name": name.trim(),
                            "parent": parent.trim(),
                            "category": category.trim(),
                            "uom": uom.trim(),
                            "opening_balance_qty": opening_qty,
                            "opening_balance_rate": opening_rate,
                            "opening_balance_value": opening_value,
                            "hsn_code": hsn_code.trim(),
                            "gst_rate": gst_rate,
                            "description": description.trim(),
                        }));
                    }
                    pos = end;
                }

                eprintln!("Parsed {} stock items from Tally for {}", items.len(), company);
                let hsn_count = items.iter().filter(|i| !i["hsn_code"].as_str().unwrap_or("").is_empty()).count();
                let gst_count = items.iter().filter(|i| i["gst_rate"].as_f64().unwrap_or(0.0) > 0.0).count();
                eprintln!("  -> {} items with HSN code, {} items with GST rate", hsn_count, gst_count);

                if !items.is_empty() {
                    let http2 = make_http();
                    if let Err(e) = supabase_upsert(&http2, "stock_items", "company_name,name", &items).await {
                        eprintln!("Warning: stock_items upsert failed: {}", e);
                    } else {
                        eprintln!("Successfully upserted {} stock items to Supabase", items.len());
                    }
                }
                items.len()
            },
            Err(e) => {
                eprintln!("Warning: failed to fetch stock items from Tally: {}", e);
                0
            },
        }
    };

    eprintln!("sync_ledgers: DONE in {:.1}s - {} ledgers, {} stock items", sync_start.elapsed().as_secs_f64(), count, stock_count);
    (StatusCode::OK, [("Content-Type", "application/json")],
        json!({ "ok": true, "company": company, "synced": count, "groups": group_count, "stock_items_synced": stock_count, "syncedAt": sync_time, "from_date": from_date, "to_date": to_date }).to_string())
}

// ── GET /tally/sync-vouchers ─────────────────────────────────────────────────

async fn sync_vouchers(
    Query(params): Query<VoucherQuery>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let company = match params.company {
        Some(n) if !n.is_empty() => n,
        _ => return (StatusCode::BAD_REQUEST, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "Missing ?company= param" }).to_string()),
    };

    let (_, fy_to) = current_fy();
    let from_date = to_tally_date(&params.from_date.unwrap_or("20000401".to_string()));
    let to_date   = to_tally_date(&params.to_date.unwrap_or(fy_to));
    eprintln!("sync_vouchers: company={}, from_date={}, to_date={}", company, from_date, to_date);
    eprintln!("sync_vouchers: Using voucher-type-by-type pagination");

    // If a specific voucher_type is requested, sync only that one (frontend step mode)
    // Otherwise sync all types in sequence
    let voucher_types: Vec<String> = if let Some(ref vt) = params.voucher_type {
        vec![vt.clone()]
    } else {
        vec![
            "Sales".into(), "Purchase".into(), "Receipt".into(),
            "Payment".into(), "Contra".into(), "Journal".into(),
            "Credit Note".into(), "Debit Note".into(),
            "Sales - Automatic".into(), "Purchase - Automatic".into(),
        ]
    };

    // ── Query each voucher type separately ───────────────────────────────
    let mut all_rows: Vec<VoucherRow> = Vec::new();
    let mut total_fetched = 0usize;
    let mut errors = 0usize;

    let start_date = chrono::NaiveDate::parse_from_str(&from_date, "%Y%m%d")
        .unwrap_or_else(|_| chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap());
    let end_date = chrono::NaiveDate::parse_from_str(&to_date, "%Y%m%d")
        .unwrap_or_else(|_| chrono::NaiveDate::from_ymd_opt(2027, 3, 31).unwrap());

    for (i, vtype) in voucher_types.iter().enumerate() {
        eprintln!("  [{}/{}] Syncing {} vouchers...", i+1, voucher_types.len(), vtype);
        let mut current_start = start_date;
        while current_start <= end_date {
            let mut current_end = current_start + chrono::Duration::days(30);
            if current_end > end_date {
                current_end = end_date;
            }
            
            let chunk_from = current_start.format("%Y%m%d").to_string();
            let chunk_to = current_end.format("%Y%m%d").to_string();
            
            eprintln!("    Chunk {} to {}: Fetching {}...", chunk_from, chunk_to, vtype);
            
            let xml = voucher_export_xml(&company, &chunk_from, &chunk_to, Some(vtype));
            let cleaned = match tally_request(&xml).await {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("    Error in chunk {}..{}: {}", chunk_from, chunk_to, e);
                    current_start = current_end + chrono::Duration::days(1);
                    continue;
                }
            };

            let envelope: TallyEnvelope = match from_str(&cleaned) {
                Ok(e) => e,
                Err(_) => { 
                    current_start = current_end + chrono::Duration::days(1);
                    continue; 
                }
            };

            let vouchers = extract_vouchers(&envelope);
            if !vouchers.is_empty() {
                let chunk_rows: Vec<VoucherRow> = vouchers.iter().map(|v| {
                    VoucherRow {
                        date:           v.date.clone(),
                        voucher_type:   v.voucher_type.clone(),
                        voucher_number: v.voucher_number.clone(),
                        party_name:     v.party_name.clone(),
                        amount:         parse_balance(&v.amount),
                        narration:      v.narration.clone(),
                        guid:           v.guid.clone(),
                        alter_id:       v.alter_id.clone(),
                        ledger_entries: vec![],
                    }
                }).collect();
                
                let n = chunk_rows.len();
                eprintln!("    {} vouchers found in chunk", n);
                
                // Upsert vouchers
                let http = make_http();
                let sb_rows: Vec<serde_json::Value> = chunk_rows.iter().map(|v| {
                    let guid = if v.guid.is_empty() {
                        format!("{}-{}-{}-{}", company, v.date, v.voucher_type, v.voucher_number)
                    } else { v.guid.clone() };
                    json!({
                        "company_name":   company,
                        "date":           v.date,
                        "voucher_type":   v.voucher_type,
                        "voucher_number": v.voucher_number,
                        "party_name":     v.party_name,
                        "amount":         v.amount,
                        "narration":      v.narration,
                        "guid":           guid,
                        "alter_id":       v.alter_id,
                    })
                }).collect();

                for chunk in sb_rows.chunks(500) {
                    let _ = supabase_upsert(&http, "vouchers", "guid", &chunk.to_vec()).await;
                }

                // Parse and upsert ledger entries for this chunk
                let mut all_ledger_entries = Vec::new();
                let mut all_inventory_entries = Vec::new();
                
                for v in &chunk_rows {
                    if v.guid.is_empty() { continue; }
                    
                    // Parse ledger entries
                    let entries = parse_ledger_entries_for_guid(&cleaned, &v.guid);
                    for (i, entry) in entries.iter().enumerate() {
                        all_ledger_entries.push(json!({
                            "company_name": company,
                            "voucher_guid": v.guid,
                            "entry_index":  i as i64,
                            "voucher_date": v.date,
                            "voucher_type": v.voucher_type,
                            "ledger_name":  entry.ledger_name,
                            "amount":       entry.amount,
                            "is_debit":     entry.is_debit,
                        }));
                    }
                    
                    // Parse inventory entries
                    let inv_entries = parse_inventory_entries_for_guid(&cleaned, &v.guid);
                    for (i, entry) in inv_entries.iter().enumerate() {
                        all_inventory_entries.push(json!({
                            "company_name": company,
                            "voucher_guid": v.guid,
                            "voucher_date": v.date,
                            "voucher_type": v.voucher_type,
                            "stock_item_name": entry.stock_item_name,
                            "quantity":     entry.quantity,
                            "rate":         entry.rate,
                            "amount":       entry.amount,
                            "uom":          entry.uom,
                            "godown":       entry.godown,
                            "batch":        entry.batch,
                        }));
                    }
                }

                if !all_ledger_entries.is_empty() {
                    eprintln!("    -> Upserting {} ledger entries", all_ledger_entries.len());
                    let _ = supabase_upsert(&http, "voucher_entries", "company_name,voucher_guid,entry_index", &all_ledger_entries).await;
                }
                
                if !all_inventory_entries.is_empty() {
                    eprintln!("    -> Upserting {} inventory entries", all_inventory_entries.len());
                    if let Err(e) = supabase_upsert(&http, "voucher_inventory_entries", "company_name,voucher_guid,stock_item_name,quantity,amount", &all_inventory_entries).await {
                        eprintln!("    !! Inventory upsert FAILED: {}", e);
                    }
                } else {
                    eprintln!("    -> No inventory entries found in this chunk");
                }

                total_fetched += n;
                all_rows.extend(chunk_rows);
            }
            
            current_start = current_end + chrono::Duration::days(1);
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }

        // Pause between types to avoid overwhelming Tally
        if i < voucher_types.len() - 1 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    eprintln!("sync_vouchers: Total {} vouchers across {} types ({} errors)",
        total_fetched, voucher_types.len(), errors);

    let count = all_rows.len();
    let sync_time = now_str();



    // ── Clean up vouchers deleted from Tally ─────────────────────────────
    if count > 0 {
        let synced_guids: std::collections::HashSet<String> = all_rows.iter()
            .map(|v| {
                if v.guid.is_empty() {
                    format!("{}-{}-{}-{}", company, v.date, v.voucher_type, v.voucher_number)
                } else { v.guid.clone() }
            })
            .collect();

        let http = make_http();
        eprintln!("Cleanup: checking vouchers in range {}..{}", from_date, to_date);
        match supabase_fetch_guids(&http, &company, Some(&from_date), Some(&to_date)).await {
            Ok(existing_guids) => {
                let deleted_guids: Vec<String> = existing_guids.into_iter()
                    .filter(|g| !synced_guids.contains(g))
                    .collect();
                if !deleted_guids.is_empty() {
                    let del_count = deleted_guids.len();
                    let _ = supabase_delete_removed(&http, "voucher_entries", "voucher_guid", &company, &deleted_guids).await;
                    let _ = supabase_delete_removed(&http, "voucher_inventory_entries", "voucher_guid", &company, &deleted_guids).await;
                    let _ = supabase_delete_removed(&http, "vouchers", "guid", &company, &deleted_guids).await;
                    eprintln!("Cleaned up {} stale vouchers for {}", del_count, company);
                }
            },
            Err(e) => eprintln!("Warning: could not check for deleted vouchers: {}", e),
        }
    }

    {
        let mut guard = state.lock().unwrap();
        let entry = guard.entry(company.clone()).or_insert(CompanyState {
            name: company.clone(), connected: false,
            last_master_sync: None, last_voucher_sync: None,
        });
        entry.connected = true;
        entry.last_voucher_sync = Some(sync_time.clone());
    }

    (StatusCode::OK, [("Content-Type", "application/json")],
        json!({
            "ok": true,
            "company": company,
            "fromDate": from_date,
            "toDate": to_date,
            "synced": count,
            "errors": errors,
            "syncedAt": sync_time,
            "data": all_rows
        }).to_string())
}

// ── POST /tally/push-voucher ─────────────────────────────────────────────────
// FIX: Accepts both single voucher (legacy) and batch array
// FIX: Accepts company as string OR object
// FIX: Saves successfully pushed vouchers to Supabase

async fn push_voucher(Json(req): Json<PushVoucherRequest>) -> impl IntoResponse {
    let (company, items) = req.into_items();

    // Validate company
    if company.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "Missing or invalid company name. Send company as a plain string." }).to_string());
    }
    if items.is_empty() {
        return (StatusCode::BAD_REQUEST, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "No vouchers provided. Send either a 'vouchers' array or flat voucher fields." }).to_string());
    }

    let http = make_http();
    let mut results = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        // Validate voucher
        if item.voucher_type.trim().is_empty() {
            results.push(json!({ "index": idx, "ok": false, "error": "Missing voucher_type" }));
            continue;
        }

        let tally_date = to_tally_date(&item.date);
        if tally_date.is_empty() {
            results.push(json!({ "index": idx, "ok": false,
                "error": format!("Invalid date '{}'. Use YYYYMMDD, DD-MM-YYYY or YYYY-MM-DD.", item.date) }));
            continue;
        }

        if item.ledger_entries.len() < 2 {
            results.push(json!({ "index": idx, "ok": false,
                "error": "Need at least 2 ledger entries (debit + credit)" }));
            continue;
        }

        // Check balanced
        let total: f64 = item.ledger_entries.iter().map(|e| {
            if e.is_debit { -e.amount.abs() } else { e.amount.abs() }
        }).sum();
        if total.abs() > 0.01 {
            results.push(json!({ "index": idx, "ok": false,
                "error": format!("Voucher not balanced. Difference = {:.2}", total) }));
            continue;
        }

        // Step A: Auto-create ledgers first (if enabled)
        if let Some(ledger_xml) = build_ledger_create_xml(&company, item) {
            match tally_request(&ledger_xml).await {
                Ok(resp) => {
                    let created_count = resp.matches("<CREATED>").count();
                    eprintln!("Auto-created {} ledger(s) in Tally", created_count);
                },
                Err(e) => {
                    eprintln!("Warning: ledger auto-create failed: {}", e);
                    // Continue anyway — ledgers might already exist
                }
            }
        }

        // Step B: Push the voucher
        let xml = build_voucher_xml(&company, item);
        match tally_request(&xml).await {
            Ok(response) => {
                let created = response.contains("<CREATED>1</CREATED>")
                    || response.contains("<CREATED> 1 </CREATED>");
                let altered = response.contains("<ALTERED>1</ALTERED>");
                let has_line_error = response.contains("<LINEERROR>");

                if (created || altered) && !has_line_error {
                    // FIX: Save to Supabase after successful Tally push
                    let guid = format!("push-{}", Uuid::new_v4());

                    let voucher_row = vec![json!({
                        "company_name":   company,
                        "date":           tally_date,
                        "voucher_type":   item.voucher_type,
                        "voucher_number": item.voucher_number.as_deref().unwrap_or(""),
                        "party_name":     item.party_ledger.as_deref().unwrap_or(""),
                        "narration":      item.narration.as_deref().unwrap_or(""),
                        "guid":           guid.clone(),
                        "alter_id":       "",
                    })];

                    if let Err(e) = supabase_upsert(&http, "vouchers", "guid", &voucher_row).await {
                        eprintln!("Warning: failed to save voucher to Supabase: {}", e);
                    }

                    let entry_rows: Vec<serde_json::Value> = item.ledger_entries.iter().enumerate().map(|(i, e)| json!({
                        "company_name": company,
                        "voucher_guid": guid,
                        "entry_index":  i as i64,
                        "voucher_date": tally_date,
                        "voucher_type": item.voucher_type,
                        "ledger_name":  e.ledger_name,
                        "amount":       e.amount,
                        "is_debit":     e.is_debit,
                    })).collect();

                    if let Err(e) = supabase_upsert(&http, "voucher_entries", "company_name,voucher_guid,entry_index", &entry_rows).await {
                        eprintln!("Warning: failed to save voucher entries to Supabase: {}", e);
                    }

                    results.push(json!({
                        "index": idx,
                        "ok": true,
                        "guid": guid,
                        "message": "Voucher created in Tally and saved to database",
                        "voucher_type": item.voucher_type,
                        "date": item.date,
                    }));
                } else {
                    let detail = if let Some(start) = response.find("<LINEERROR>") {
                        let rest = &response[start + 11..];
                        rest.split("</LINEERROR>").next().unwrap_or("Unknown error").trim().to_string()
                    } else {
                        format!("Tally did not confirm creation. Response snippet: {}", &response[..response.len().min(300)])
                    };
                    results.push(json!({
                        "index": idx,
                        "ok": false,
                        "error": "TALLY_IMPORT_FAILED",
                        "details": detail,
                        "voucher_type": item.voucher_type,
                        "date": item.date,
                    }));
                }
            },
            Err(e) => {
                results.push(json!({
                    "index": idx,
                    "ok": false,
                    "error": "TALLY_CONNECTION_FAILED",
                    "details": e,
                }));
            }
        }
    }

    let success_count = results.iter().filter(|r| r["ok"].as_bool().unwrap_or(false)).count();
    let fail_count = results.len() - success_count;
    let overall_ok = fail_count == 0;
    let status = if overall_ok { StatusCode::OK } else if success_count == 0 { StatusCode::UNPROCESSABLE_ENTITY } else { StatusCode::MULTI_STATUS };

    (status, [("Content-Type", "application/json")],
        json!({
            "ok": overall_ok,
            "company": company,
            "total": results.len(),
            "succeeded": success_count,
            "failed": fail_count,
            "results": results,
        }).to_string())
}

// ── GET /tally/test-post ─────────────────────────────────────────────────────

async fn test_post() -> impl IntoResponse {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Companies</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT></STATICVARIABLES></DESC></BODY>
</ENVELOPE>"#;

    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(60)).build().unwrap();
    match client.post("http://127.0.0.1:9000")
        .header("Content-Type", "text/xml;charset=utf-8")
        .body(xml.to_string()).send().await
    {
        Ok(r) => {
            let status = r.status().as_u16();
            let text = r.text().await.unwrap_or_default();
            (StatusCode::OK, [("Content-Type", "application/json")],
                json!({ "ok": true, "http_status": status, "response_length": text.len(), "preview": &text[..text.len().min(500)] }).to_string())
        },
        Err(e) => (StatusCode::OK, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": format!("{}", e) }).to_string()),
    }
}

// ── GET /tally/debug-companies ───────────────────────────────────────────────

async fn debug_companies() -> impl IntoResponse {
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Companies</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT></STATICVARIABLES></DESC></BODY>
</ENVELOPE>"#;
    match tally_request(xml).await {
        Ok(body) => (StatusCode::OK,          [("Content-Type", "text/xml")], body),
        Err(e)   => (StatusCode::BAD_GATEWAY, [("Content-Type", "text/xml")], format!("<error>{}</error>", e)),
    }
}

// ── GET /tally/debug-ledgers ─────────────────────────────────────────────────

async fn debug_ledgers() -> impl IntoResponse {
    // FIX: was hardcoded to wrong company — now reads from Tally's open company
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Ledgers</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT></STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="List of Ledgers" ISMODIFY="No">
    <TYPE>Ledger</TYPE><NATIVEMETHOD>Name</NATIVEMETHOD><NATIVEMETHOD>Parent</NATIVEMETHOD>
    <NATIVEMETHOD>OpeningBalance</NATIVEMETHOD><NATIVEMETHOD>ClosingBalance</NATIVEMETHOD>
    <NATIVEMETHOD>PartyGSTIN</NATIVEMETHOD><NATIVEMETHOD>GSTRegistrationType</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#;
    match tally_request(xml).await {
        Ok(r)  => (StatusCode::OK, [("Content-Type", "text/xml")], r),
        Err(e) => (StatusCode::BAD_GATEWAY, [("Content-Type", "text/xml")], e),
    }
}

// ── GET /tally/debug-stock-items ─────────────────────────────────────────────
// Returns raw XML from Tally for stock items — helps debug HSN/GST extraction

async fn debug_stock_items() -> impl IntoResponse {
    // Use NATIVEMETHOD * to fetch ALL stock item fields
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>StockItemFull</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT></STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="StockItemFull" ISMODIFY="No">
    <TYPE>Stock Item</TYPE>
    <NATIVEMETHOD>*</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#;
    match tally_request(xml).await {
        Ok(r)  => (StatusCode::OK, [("Content-Type", "text/plain")], r),
        Err(e) => (StatusCode::BAD_GATEWAY, [("Content-Type", "text/plain")], e),
    }
}

// ── GET /tally/debug-vouchers ────────────────────────────────────────────────
// FIX: was hardcoded to wrong company — now uses currently open company in Tally

async fn debug_vouchers(
    Query(params): Query<VoucherQuery>,
) -> impl IntoResponse {
    // Step 1: Get company from params or auto-detect
    let company = match params.company {
        Some(c) if !c.is_empty() => c,
        _ => {
            let company_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Companies</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT></STATICVARIABLES></DESC></BODY>
</ENVELOPE>"#;

            match tally_request(company_xml).await {
                Ok(xml) => {
                    let names = extract_company_names(&xml);
                    names.into_iter().next().unwrap_or_default()
                },
                Err(e) => return (StatusCode::BAD_GATEWAY, [("Content-Type", "text/xml")], format!("<error>Could not detect open company: {}</error>", e)),
            }
        }
    };

    if company.is_empty() {
        return (StatusCode::BAD_GATEWAY, [("Content-Type", "text/xml")], "<error>No company open in Tally</error>".to_string());
    }

    // Step 2: Fetch vouchers for requested period
    let today = Local::now().format("%Y%m%d").to_string();
    let from_date = params.from_date.unwrap_or(today.clone());
    let to_date = params.to_date.unwrap_or(today);
    
    let xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER>
    <VERSION>1</VERSION>
    <TALLYREQUEST>Export</TALLYREQUEST>
    <TYPE>Collection</TYPE>
    <ID>DebugVouchers</ID>
  </HEADER>
  <BODY>
    <DESC>
      <STATICVARIABLES>
        <SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>
        {cvar}
        <SVFROMDATE>{from_date}</SVFROMDATE>
        <SVTODATE>{to_date}</SVTODATE>
      </STATICVARIABLES>
      <TDL>
        <TDLMESSAGE>
          <COLLECTION NAME="DebugVouchers" ISMODIFY="No">
            <TYPE>Voucher</TYPE>
            <FILTER>DateFromFilter</FILTER>
            <FILTER>DateToFilter</FILTER>
            <NATIVEMETHOD>*</NATIVEMETHOD>
            <NATIVEMETHOD>AllLedgerEntries</NATIVEMETHOD>
            <NATIVEMETHOD>AllInventoryEntries</NATIVEMETHOD>
            <NATIVEMETHOD>LedgerEntries</NATIVEMETHOD>
            <NATIVEMETHOD>InventoryEntries</NATIVEMETHOD>
            <FETCH>AllLedgerEntries</FETCH>
            <FETCH>AllInventoryEntries</FETCH>
            <FETCH>LedgerEntries</FETCH>
            <FETCH>InventoryEntries</FETCH>
          </COLLECTION>
          <SYSTEM TYPE="Formulae" NAME="DateFromFilter">$Date >= $$Date:"{from_date}"</SYSTEM>
          <SYSTEM TYPE="Formulae" NAME="DateToFilter">NOT $Date > $$Date:"{to_date}"</SYSTEM>
        </TDLMESSAGE>
      </TDL>
    </DESC>
  </BODY>
</ENVELOPE>"#, cvar = company_var(&company), from_date = from_date, to_date = to_date);

    match tally_request(&xml).await {
        Ok(r) => (StatusCode::OK, [("Content-Type", "text/plain")], r),
        Err(e) => (StatusCode::BAD_GATEWAY, [("Content-Type", "text/plain")], format!("<error>{}</error>", e)),
    }
}

// ── GET /tally/available-companies ──────────────────────────────────────────

async fn available_companies(
    Query(params): Query<TokenQuery>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let token = match params.token {
        Some(t) if !t.is_empty() => t,
        _ => return (StatusCode::UNAUTHORIZED, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "Missing auth token. Please log in again." }).to_string()),
    };

    let http = make_http();

    // 1. Get user details from production backend auth/me directly
    let backend_url = "https://ca-copilot-mrwj.onrender.com/api/v1/auth/me".to_string();
    let response = http.get(&backend_url)
        .header("Authorization", format!("Bearer {}", token))
        .send().await;

    let user_data = match response {
        Ok(r) => {
            let status = r.status();
            let text = r.text().await.unwrap_or_default();
            if !status.is_success() {
                return (StatusCode::UNAUTHORIZED, [("Content-Type", "application/json")],
                    json!({ "ok": false, "error": "Invalid or expired session. Please log in again." }).to_string());
            }
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => v,
                Err(_) => return (StatusCode::UNAUTHORIZED, [("Content-Type", "application/json")],
                    json!({ "ok": false, "error": "Failed to parse session response from backend." }).to_string()),
            }
        },
        Err(e) => return (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": format!("Auth service unreachable: {}", e) }).to_string()),
    };

    let user_id = match user_data["id"].as_str() {
        Some(id) => id.to_string(),
        None => return (StatusCode::UNAUTHORIZED, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "User ID missing from session response." }).to_string()),
    };

    let firm_id = match user_data["firm_id"].as_str() {
        Some(fid) => fid.to_string(),
        None => match user_data["firm_id"].as_i64() {
            Some(fid) => fid.to_string(),
            None => "".to_string(),
        }
    };

    let db_clients: Vec<(String, String)> = match http
        .get(format!("{}/rest/v1/clients?select=id,name&firm_id=eq.{}&order=name.asc", SUPABASE_URL, firm_id))
        .header("apikey", SUPABASE_KEY)
        .header("Authorization", format!("Bearer {}", SUPABASE_KEY))
        .header("Content-Type", "application/json")
        .send().await
    {
        Ok(r) => {
            let text = r.text().await.unwrap_or_default();
            match serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                Ok(rows) => rows.iter().filter_map(|r| {
                    let id   = r["id"].as_str().map(|s| s.to_string())
                        .or_else(|| r["id"].as_i64().map(|n| n.to_string()))
                        .unwrap_or_default();
                    let name = r["name"].as_str().map(|s| s.to_string()).unwrap_or_default();
                    if name.is_empty() { None } else { Some((id, name)) }
                }).collect(),
                Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, [("Content-Type", "application/json")],
                    json!({ "ok": false, "error": "Failed to parse clients from database." }).to_string()),
            }
        },
        Err(e) => return (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": format!("Database unreachable: {}", e) }).to_string()),
    };

    let tally_names: std::collections::HashSet<String> = {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Companies</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT></STATICVARIABLES></DESC></BODY>
</ENVELOPE>"#;
        match tally_request(xml).await {
            Ok(cleaned) => extract_company_names(&cleaned).into_iter().collect(),
            Err(_) => std::collections::HashSet::new(),
        }
    };

    let guard = state.lock().unwrap();
    let companies: Vec<serde_json::Value> = db_clients.iter().map(|(id, name)| {
        json!({
            "id":            id,
            "name":          name,
            "already_added": guard.get(name).map(|c| c.connected).unwrap_or(false),
            "in_tally":      tally_names.contains(name),
        })
    }).collect();

    (StatusCode::OK, [("Content-Type", "application/json")],
        json!({ "ok": true, "firm_id": firm_id, "companies": companies, "total": companies.len() }).to_string())
}

// ── GET /tally/add-company ───────────────────────────────────────────────────

async fn add_company(
    Query(params): Query<CompanyQuery>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let company = match params.company {
        Some(n) if !n.is_empty() => n,
        _ => return (StatusCode::BAD_REQUEST, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "Missing ?company= param" }).to_string()),
    };

    let sync_time = now_str();
    let http = make_http();

    // Step 0: Fetch group hierarchy for primary_group resolution (same as sync_ledgers)
    let group_xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Groups</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>{cvar}</STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="List of Groups" ISMODIFY="No">
    <TYPE>Group</TYPE><NATIVEMETHOD>Name</NATIVEMETHOD><NATIVEMETHOD>Parent</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#, cvar = company_var(&company));

    let group_primary: HashMap<String, String> = match tally_request(&group_xml).await {
        Ok(xml) => {
            let mut map = HashMap::new();
            let mut pos = 0;
            while let Some(start) = xml[pos..].find("<GROUP ") {
                let abs_start = pos + start;
                let end = match xml[abs_start..].find("</GROUP>") {
                    Some(p) => abs_start + p + 8,
                    None => break,
                };
                let chunk = &xml[abs_start..end];
                let gname = {
                    let mut n = String::new();
                    if let Some(nstart) = chunk.find("NAME=\"") {
                        let after = &chunk[nstart + 6..];
                        if let Some(nend) = after.find('"') {
                            n = after[..nend].to_string();
                        }
                    }
                    n
                };
                let primary = {
                    let mut p = String::new();
                    if let Some(pstart) = chunk.find("<PRIMARYGRPPARENT") {
                        let after = &chunk[pstart..];
                        if let Some(gt) = after.find('>') {
                            let content = &after[gt + 1..];
                            if let Some(close) = content.find("</PRIMARYGRPPARENT>") {
                                p = content[..close].trim().to_string();
                            }
                        }
                    }
                    p
                };
                if !gname.is_empty() && !primary.is_empty() {
                    map.insert(gname, primary);
                }
                pos = end;
            }
            eprintln!("add-company: parsed {} group→primary_group mappings", map.len());
            map
        },
        Err(e) => {
            eprintln!("Warning: failed to fetch groups in add-company: {}", e);
            HashMap::new()
        },
    };

    let resolve_primary = |parent: &str| -> String {
        if let Some(pg) = group_primary.get(parent) { pg.clone() } else { parent.to_string() }
    };

    // Step 1: Sync Ledgers (with current FY date range for period-specific balances)
    let (fy_from, fy_to) = current_fy();
    // Compute FY label from fy_from (YYYYMMDD format), e.g. "20260401" → "2026-27"
    let add_fy_period = {
        let y: i32 = fy_from[..4].parse().unwrap_or(2026);
        let short_next = format!("{:02}", (y + 1) % 100);
        format!("{}-{}", y, short_next)
    };
    let ledger_xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Ledgers</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>{cvar}
    <SVFROMDATE>{fy_from}</SVFROMDATE><SVTODATE>{fy_to}</SVTODATE>
  </STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="List of Ledgers" ISMODIFY="No">
    <TYPE>Ledger</TYPE><NATIVEMETHOD>Name</NATIVEMETHOD><NATIVEMETHOD>Parent</NATIVEMETHOD>
    <NATIVEMETHOD>OpeningBalance</NATIVEMETHOD><NATIVEMETHOD>ClosingBalance</NATIVEMETHOD>
    <NATIVEMETHOD>PartyGSTIN</NATIVEMETHOD><NATIVEMETHOD>GSTRegistrationType</NATIVEMETHOD>
    <NATIVEMETHOD>LedStateName</NATIVEMETHOD><NATIVEMETHOD>PinCode</NATIVEMETHOD>
    <NATIVEMETHOD>Email</NATIVEMETHOD><NATIVEMETHOD>LedgerMobile</NATIVEMETHOD>
    <NATIVEMETHOD>Address</NATIVEMETHOD><NATIVEMETHOD>MailingName</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#, cvar = company_var(&company), fy_from = fy_from, fy_to = fy_to);

    let ledger_count = match tally_request(&ledger_xml).await {
        Ok(cleaned) => match from_str::<TallyEnvelope>(&cleaned) {
            Ok(env) => {
                let ledgers = &env.body.data.collection.ledgers;
                let count = ledgers.len();
                if count > 0 {
                    let rows: Vec<serde_json::Value> = ledgers.iter().map(|l| {
                        let parent = l.parent.clone();
                        let primary = resolve_primary(&parent);
                        json!({
                            "company_name": company, "name": l.name().to_string(),
                            "parent": parent, "primary_group": primary,
                            "opening_balance": parse_balance(&l.opening_balance),
                            "closing_balance": parse_balance(&l.closing_balance),
                            "party_gstin": l.party_gstin, "gst_registration_type": l.gst_registration_type,
                            "state": l.state, "pin_code": l.pin_code, "email": l.email,
                            "mobile": l.mobile, "address": l.address, "mailing_name": l.mailing_name,
                            "fy_period": add_fy_period,
                        })
                    }).collect();
                    if let Err(e) = supabase_upsert(&http, "ledgers", "company_name,name,fy_period", &rows).await {
                        eprintln!("Warning: ledger upsert in add-company failed: {}", e);
                    }
                }
                count
            },
            Err(_) => 0,
        },
        Err(e) => return (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "step": "master", "error": e }).to_string()),
    };

    // Step 1b: Sync Stock Items (NATIVEMETHOD * for full data)
    let stock_item_xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>StockItemSync</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>{cvar}</STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="StockItemSync" ISMODIFY="No">
    <TYPE>Stock Item</TYPE>
    <NATIVEMETHOD>*</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#, cvar = company_var(&company));

    let stock_items_count = match tally_request(&stock_item_xml).await {
        Ok(xml) => {
            let mut items = Vec::new();
            let mut pos = 0;
            while let Some(start) = xml[pos..].find("<STOCKITEM") {
                let abs_start = pos + start;
                let end = match xml[abs_start..].find("</STOCKITEM>") {
                    Some(p) => abs_start + p + "</STOCKITEM>".len(),
                    None => break,
                };
                let chunk = &xml[abs_start..end];
                let name = {
                    let mut n = String::new();
                    if let Some(nstart) = chunk.find("NAME=\"") {
                        let after = &chunk[nstart + 6..];
                        if let Some(nend) = after.find('"') {
                            n = after[..nend].to_string();
                        }
                    }
                    if n.is_empty() { n = extract_xml_value(chunk, "NAME"); }
                    n
                };
                if !name.is_empty() {
                    // Extract HSN and GST from nested GSTDETAILS.LIST
                    let (hsn_code, gst_rate) = {
                        let mut hsn = String::new();
                        let mut rate = 0.0_f64;
                        if let Some(gst_start) = chunk.find("<GSTDETAILS.LIST") {
                            let gst_end_tag = "</GSTDETAILS.LIST>";
                            if let Some(gst_end) = chunk[gst_start..].find(gst_end_tag) {
                                let gst_chunk = &chunk[gst_start..gst_start + gst_end + gst_end_tag.len()];
                                let h = extract_xml_value(gst_chunk, "HSNCODE");
                                if !h.is_empty() { hsn = h; }
                                if hsn.is_empty() {
                                    let h2 = extract_xml_value(gst_chunk, "HSN");
                                    if !h2.is_empty() { hsn = h2; }
                                }
                                if hsn.is_empty() {
                                    let h3 = extract_xml_value(gst_chunk, "HSNMASTERNAME");
                                    if !h3.is_empty() { hsn = h3; }
                                }
                                // GST rate: iterate RATEDETAILS.LIST to find IGST (= total rate)
                                {
                                    let mut cgst = 0.0_f64;
                                    let mut sgst = 0.0_f64;
                                    let mut igst = 0.0_f64;
                                    let mut rd_pos = 0usize;
                                    while let Some(rd_start) = gst_chunk[rd_pos..].find("<RATEDETAILS.LIST>") {
                                        let rd_abs = rd_pos + rd_start;
                                        let rd_end_tag = "</RATEDETAILS.LIST>";
                                        if let Some(rd_end) = gst_chunk[rd_abs..].find(rd_end_tag) {
                                            let rd_block = &gst_chunk[rd_abs..rd_abs + rd_end + rd_end_tag.len()];
                                            let duty_head = extract_xml_value(rd_block, "GSTRATEDUTYHEAD");
                                            let rd_rate = parse_balance(&extract_xml_value(rd_block, "GSTRATE")).unwrap_or(0.0);
                                            match duty_head.trim() {
                                                "IGST" => { igst = rd_rate; },
                                                "CGST" => { cgst = rd_rate; },
                                                "SGST/UTGST" | "SGST" => { sgst = rd_rate; },
                                                _ => {},
                                            }
                                            rd_pos = rd_abs + rd_end + rd_end_tag.len();
                                        } else { break; }
                                    }
                                    if igst > 0.0 {
                                        rate = igst;
                                    } else if cgst > 0.0 || sgst > 0.0 {
                                        rate = cgst + sgst;
                                    }
                                }
                            }
                        }
                        if hsn.is_empty() { hsn = extract_xml_value(chunk, "HSNCODE"); }
                        if hsn.is_empty() {
                            let h = extract_xml_value(chunk, "HSN");
                            if !h.is_empty() { hsn = h; }
                        }
                        if rate == 0.0 {
                            let r = extract_xml_value(chunk, "GSTRATE");
                            if !r.is_empty() { rate = parse_balance(&r).unwrap_or(0.0); }
                        }
                        (hsn, rate)
                    };
                    items.push(json!({
                        "company_name": company.trim(), "name": name.trim(),
                        "parent": extract_xml_value(chunk, "PARENT").trim().to_string(),
                        "category": extract_xml_value(chunk, "CATEGORY").trim().to_string(),
                        "uom": extract_xml_value(chunk, "BASEUNITS").trim().to_string(),
                        "opening_balance_qty": parse_tally_number(&extract_xml_value(chunk, "OPENINGBALANCE")),
                        "opening_balance_rate": parse_tally_number(&extract_xml_value(chunk, "OPENINGRATE")),
                        "opening_balance_value": parse_tally_number(&extract_xml_value(chunk, "OPENINGVALUE")),
                        "hsn_code": hsn_code.trim(),
                        "gst_rate": gst_rate,
                        "description": extract_xml_value(chunk, "DESCRIPTION").trim().to_string(),
                    }));
                }
                pos = end;
            }
            eprintln!("add-company: parsed {} stock items for {}", items.len(), company);
            let hsn_count = items.iter().filter(|i| !i["hsn_code"].as_str().unwrap_or("").is_empty()).count();
            eprintln!("  → {} items with HSN code", hsn_count);
            if !items.is_empty() {
                if let Err(e) = supabase_upsert(&http, "stock_items", "company_name,name", &items).await {
                    eprintln!("Warning: stock_items upsert in add-company failed: {}", e);
                }
            }
            items.len()
        },
        Err(e) => { eprintln!("Warning: stock items fetch failed in add-company: {}", e); 0 },
    };

    // Step 2: Sync Vouchers
    let (_, fy_to) = current_fy();
    let voucher_xml = voucher_export_xml(&company, "20000401", &fy_to, None);

    let voucher_count = match tally_request(&voucher_xml).await {
        Ok(cleaned) => match from_str::<TallyEnvelope>(&cleaned) {
            Ok(env) => {
                let vouchers = extract_vouchers(&env);
                let count = vouchers.len();
                if count > 0 {
                    // FIX: Generate fallback GUIDs when Tally doesn't provide them
                    let rows: Vec<serde_json::Value> = vouchers.iter().map(|v| {
                        let guid = if v.guid.is_empty() {
                            format!("{}-{}-{}-{}", company, v.date, v.voucher_type, v.voucher_number)
                        } else {
                            v.guid.clone()
                        };
                        json!({
                            "company_name":   company, "date": v.date, "voucher_type": v.voucher_type,
                            "voucher_number": v.voucher_number, "party_name": v.party_name,
                            "amount":         parse_balance(&v.amount), "narration": v.narration,
                            "guid":           guid, "alter_id": v.alter_id,
                        })
                    }).collect();

                    if let Err(e) = supabase_upsert(&http, "vouchers", "guid", &rows).await {
                        eprintln!("Warning: vouchers upsert in add-company failed: {}", e);
                    }

                    let entry_rows: Vec<serde_json::Value> = vouchers.iter().flat_map(|v| {
                        let cn = company.clone();
                        let guid = if v.guid.is_empty() {
                            format!("{}-{}-{}-{}", company, v.date, v.voucher_type, v.voucher_number)
                        } else {
                            v.guid.clone()
                        };
                        let entries = parse_ledger_entries_for_guid(&cleaned, &v.guid);
                        entries.into_iter().enumerate().map(move |(i, e)| json!({
                            "company_name": cn, "voucher_guid": guid,
                            "entry_index":  i as i64,
                            "voucher_date": v.date, "voucher_type": v.voucher_type,
                            "ledger_name":  e.ledger_name, "amount": e.amount,
                            "is_debit":     e.is_debit,
                        }))
                    }).collect();

                    if let Err(e) = supabase_upsert(&http, "voucher_entries", "company_name,voucher_guid,entry_index", &entry_rows).await {
                        eprintln!("Warning: voucher_entries upsert in add-company failed: {}", e);
                    }

                    let inv_rows: Vec<serde_json::Value> = vouchers.iter().flat_map(|v| {
                        let cn = company.clone();
                        let entries = parse_inventory_entries_for_guid(&cleaned, &v.guid);
                        entries.into_iter().map(move |e| json!({
                            "company_name":    cn, "voucher_guid": v.guid,
                            "voucher_date":    v.date, "voucher_type": v.voucher_type,
                            "stock_item_name": e.stock_item_name, "quantity": e.quantity,
                            "rate":            e.rate, "amount": e.amount,
                            "uom":             e.uom, "godown": e.godown, "batch": e.batch,
                        }))
                    }).collect();

                    if let Err(e) = supabase_upsert(&http, "voucher_inventory_entries", "company_name,voucher_guid,stock_item_name,quantity,amount", &inv_rows).await {
                        eprintln!("Warning: inventory_entries upsert in add-company failed: {}", e);
                    }
                }
                count
            },
            Err(_) => 0,
        },
        Err(_) => 0,
    };

    {
        let mut guard = state.lock().unwrap();
        let entry = guard.entry(company.clone()).or_insert(CompanyState {
            name: company.clone(), connected: false,
            last_master_sync: None, last_voucher_sync: None,
        });
        entry.connected = true;
        entry.last_master_sync  = Some(sync_time.clone());
        entry.last_voucher_sync = Some(sync_time.clone());
    }

    (StatusCode::OK, [("Content-Type", "application/json")],
        json!({
            "ok": true,
            "company": company,
            "ledgers_synced": ledger_count,
            "stock_items_synced": stock_items_count,
            "vouchers_synced": voucher_count,
            "syncedAt": sync_time
        }).to_string())
}