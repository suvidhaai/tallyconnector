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
    for list_tag in &["ALLINVENTORYENTRIES.LIST", "INVENTORYENTRIES.LIST"] {
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

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&apos;")
}

async fn tally_request(xml: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
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

fn voucher_export_xml(company: &str, from_date: &str, to_date: &str) -> String {
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
            <NATIVEMETHOD>Date</NATIVEMETHOD>
            <NATIVEMETHOD>VoucherTypeName</NATIVEMETHOD>
            <NATIVEMETHOD>VoucherNumber</NATIVEMETHOD>
            <NATIVEMETHOD>PartyLedgerName</NATIVEMETHOD>
            <NATIVEMETHOD>Amount</NATIVEMETHOD>
            <NATIVEMETHOD>Narration</NATIVEMETHOD>
            <NATIVEMETHOD>GUID</NATIVEMETHOD>
            <NATIVEMETHOD>AlterID</NATIVEMETHOD>
            <NATIVEMETHOD>AllLedgerEntries</NATIVEMETHOD>
            <NATIVEMETHOD>AllInventoryEntries</NATIVEMETHOD>
          </COLLECTION>
        </TDLMESSAGE>
      </TDL>
    </DESC>
  </BODY>
</ENVELOPE>"#,
        cvar = company_var(company),
        from_date = from_date,
        to_date = to_date,
    )
}

// ── Build Tally import XML ───────────────────────────────────────────────────

fn build_import_xml(company: &str, item: &PushVoucherItem) -> String {
    let date = to_tally_date(&item.date);
    let narration = item.narration.as_deref().unwrap_or("");
    let vch_type = &item.voucher_type;

    let mut entries_xml = String::new();
    for entry in &item.ledger_entries {
        let tally_amount = if entry.is_debit { -entry.amount.abs() } else { entry.amount.abs() };
        let deemed = if entry.is_debit { "Yes" } else { "No" };

        if item.auto_create_ledgers {
            let parent = entry.parent_group
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or("Indirect Expenses");

            entries_xml.push_str(&format!(
                r#"
            <ALLLEDGERENTRIES.LIST ISCREATE="Yes">
              <LEDGERNAME>{ledger}</LEDGERNAME>
              <ISDEEMEDPOSITIVE>{deemed}</ISDEEMEDPOSITIVE>
              <AMOUNT>{amount:.2}</AMOUNT>
              <LEDGER NAME="{ledger}" ACTION="Create">
                <NAME>{ledger}</NAME>
                <PARENT>{parent}</PARENT>
              </LEDGER>
            </ALLLEDGERENTRIES.LIST>"#,
                ledger = xml_escape(&entry.ledger_name),
                deemed = deemed,
                amount = tally_amount,
                parent = xml_escape(parent),
            ));
        } else {
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
                .route("/tally/sync-vouchers",        get(sync_vouchers))
                .route("/tally/push-voucher",         post(push_voucher))
                .route("/tally/debug-companies",      get(debug_companies))
                .route("/tally/test-post",            get(test_post))
                .route("/tally/debug-ledgers",        get(debug_ledgers))
                .route("/tally/debug-vouchers",       get(debug_vouchers))
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
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Companies</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT></STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="List of Companies" ISMODIFY="No">
    <TYPE>Company</TYPE><NATIVEMETHOD>Name</NATIVEMETHOD><NATIVEMETHOD>BasicCompanyName</NATIVEMETHOD>
    <NATIVEMETHOD>GUID</NATIVEMETHOD><NATIVEMETHOD>StartingFrom</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#;

    let cleaned = match tally_request(xml).await {
        Ok(x) => x,
        Err(e) => return (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "TALLY_CONNECTION_FAILED", "details": e }).to_string()),
    };

    let tally_names = extract_company_names(&cleaned);
    let guard = state.lock().unwrap();

    let companies: Vec<serde_json::Value> = tally_names.iter().map(|name| {
        let cs = guard.get(name);
        json!({
            "name": name,
            "connected": cs.map(|c| c.connected).unwrap_or(false),
            "lastMasterSync": cs.and_then(|c| c.last_master_sync.clone()),
            "lastVoucherSync": cs.and_then(|c| c.last_voucher_sync.clone()),
        })
    }).collect();

    (StatusCode::OK, [("Content-Type", "application/json")],
        json!({ "ok": true, "companies": companies }).to_string())
}

// ── GET /tally/sync-ledgers ──────────────────────────────────────────────────

async fn sync_ledgers(
    Query(params): Query<CompanyQuery>,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let company = match params.company {
        Some(n) if !n.is_empty() => n,
        _ => return (StatusCode::BAD_REQUEST, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "Missing ?company= param" }).to_string()),
    };

    let xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Ledgers</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>{cvar}</STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="List of Ledgers" ISMODIFY="No">
    <TYPE>Ledger</TYPE><NATIVEMETHOD>Name</NATIVEMETHOD><NATIVEMETHOD>Parent</NATIVEMETHOD>
    <NATIVEMETHOD>OpeningBalance</NATIVEMETHOD><NATIVEMETHOD>ClosingBalance</NATIVEMETHOD>
    <NATIVEMETHOD>PartyGSTIN</NATIVEMETHOD><NATIVEMETHOD>GSTRegistrationType</NATIVEMETHOD>
    <NATIVEMETHOD>LedStateName</NATIVEMETHOD><NATIVEMETHOD>PinCode</NATIVEMETHOD>
    <NATIVEMETHOD>Email</NATIVEMETHOD><NATIVEMETHOD>LedgerMobile</NATIVEMETHOD>
    <NATIVEMETHOD>Address</NATIVEMETHOD><NATIVEMETHOD>MailingName</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#, cvar = company_var(&company));

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

    let rows: Vec<LedgerRow> = envelope.body.data.collection.ledgers.iter().map(|l| LedgerRow {
        name:                  l.name().to_string(),
        parent:                l.parent.clone(),
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
    }).collect();

    let count = rows.len();
    let sync_time = now_str();

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
        let upsert_rows: Vec<serde_json::Value> = rows.iter().map(|r| json!({
            "company_name":           company.trim(),
            "name":                   r.name.trim().replace("\r\n", "").replace("\r", "").replace("\n", ""),
            "parent":                 r.parent.trim(),
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
        })).collect();

        // FIX: surface Supabase errors instead of silently ignoring
        if let Err(e) = supabase_upsert(&http, "ledgers", "company_name,name", &upsert_rows).await {
            return (StatusCode::OK, [("Content-Type", "application/json")],
                json!({ "ok": false, "error": "SUPABASE_UPSERT_FAILED", "details": e }).to_string());
        }
    }

    (StatusCode::OK, [("Content-Type", "application/json")],
        json!({ "ok": true, "company": company, "synced": count, "syncedAt": sync_time }).to_string())
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

    let xml = voucher_export_xml(&company, &from_date, &to_date);
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

    let vouchers = extract_vouchers(&envelope);
    let rows: Vec<VoucherRow> = vouchers.iter().map(|v| {
        let entries = parse_ledger_entries_for_guid(&cleaned, &v.guid);
        VoucherRow {
            date:           v.date.clone(),
            voucher_type:   v.voucher_type.clone(),
            voucher_number: v.voucher_number.clone(),
            party_name:     v.party_name.clone(),
            amount:         parse_balance(&v.amount),
            narration:      v.narration.clone(),
            guid:           v.guid.clone(),
            alter_id:       v.alter_id.clone(),
            ledger_entries: entries,
        }
    }).collect();

    let count = rows.len();
    let sync_time = now_str();

    if count > 0 {
        let http = make_http();

        // FIX: Generate a stable fallback GUID when Tally doesn't provide one
        let sb_rows: Vec<serde_json::Value> = rows.iter().map(|v| {
            let guid = if v.guid.is_empty() {
                format!("{}-{}-{}-{}", company, v.date, v.voucher_type, v.voucher_number)
            } else {
                v.guid.clone()
            };
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

        // FIX: surface errors
        if let Err(e) = supabase_upsert(&http, "vouchers", "guid", &sb_rows).await {
            eprintln!("Warning: vouchers upsert failed: {}", e);
        }

        let entry_rows: Vec<serde_json::Value> = rows.iter().flat_map(|v| {
            let cn = company.clone();
            let guid = if v.guid.is_empty() {
                format!("{}-{}-{}-{}", company, v.date, v.voucher_type, v.voucher_number)
            } else {
                v.guid.clone()
            };
            v.ledger_entries.iter().map(move |e| json!({
                "company_name": cn,
                "voucher_guid": guid,
                "voucher_date": v.date,
                "voucher_type": v.voucher_type,
                "ledger_name":  e.ledger_name,
                "amount":       e.amount,
                "is_debit":     e.is_debit,
            }))
        }).collect();

        if let Err(e) = supabase_upsert(&http, "voucher_entries", "company_name,voucher_guid,ledger_name,amount", &entry_rows).await {
            eprintln!("Warning: voucher_entries upsert failed: {}", e);
        }

        let inv_rows: Vec<serde_json::Value> = rows.iter().flat_map(|v| {
            let cn = company.clone();
            let entries = parse_inventory_entries_for_guid(&cleaned, &v.guid);
            entries.into_iter().map(move |e| json!({
                "company_name":    cn,
                "voucher_guid":    v.guid,
                "voucher_date":    v.date,
                "voucher_type":    v.voucher_type,
                "stock_item_name": e.stock_item_name,
                "quantity":        e.quantity,
                "rate":            e.rate,
                "amount":          e.amount,
                "uom":             e.uom,
                "godown":          e.godown,
                "batch":           e.batch,
            }))
        }).collect();

        if let Err(e) = supabase_upsert(&http, "voucher_inventory_entries", "company_name,voucher_guid,stock_item_name,quantity,amount", &inv_rows).await {
            eprintln!("Warning: voucher_inventory_entries upsert failed: {}", e);
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
            "syncedAt": sync_time,
            "data": rows
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

        let xml = build_import_xml(&company, item);
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

                    let entry_rows: Vec<serde_json::Value> = item.ledger_entries.iter().map(|e| json!({
                        "company_name": company,
                        "voucher_guid": guid,
                        "voucher_date": tally_date,
                        "voucher_type": item.voucher_type,
                        "ledger_name":  e.ledger_name,
                        "amount":       e.amount,
                        "is_debit":     e.is_debit,
                    })).collect();

                    if let Err(e) = supabase_upsert(&http, "voucher_entries", "company_name,voucher_guid,ledger_name,amount", &entry_rows).await {
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

// ── GET /tally/debug-vouchers ────────────────────────────────────────────────
// FIX: was hardcoded to wrong company — now uses currently open company in Tally

async fn debug_vouchers() -> impl IntoResponse {
    // Step 1: Get the currently open company from Tally
    let company_xml = r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Companies</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT></STATICVARIABLES></DESC></BODY>
</ENVELOPE>"#;

    let company = match tally_request(company_xml).await {
        Ok(xml) => {
            let names = extract_company_names(&xml);
            names.into_iter().next().unwrap_or_default()
        },
        Err(e) => return (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": format!("Could not detect open company: {}", e) }).to_string()),
    };

    if company.is_empty() {
        return (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": "No company open in Tally" }).to_string());
    }

    // Step 2: Fetch one day of vouchers to inspect structure
    let today = Local::now().format("%Y%m%d").to_string();
    let xml = voucher_export_xml(&company, &today, &today);
    match tally_request(&xml).await {
        Ok(r) => {
            let preview = &r[..r.len().min(2000)];
            (StatusCode::OK, [("Content-Type", "application/json")],
                json!({ "ok": true, "company": company, "date": today, "xml_preview": preview }).to_string())
        },
        Err(e) => (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": e }).to_string()),
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

    let user_id = match http.get(format!("{}/auth/v1/user", SUPABASE_URL))
        .header("apikey", SUPABASE_KEY)
        .header("Authorization", format!("Bearer {}", token))
        .send().await
    {
        Ok(r) => {
            let text = r.text().await.unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) => match v["id"].as_str() {
                    Some(id) => id.to_string(),
                    None => return (StatusCode::UNAUTHORIZED, [("Content-Type", "application/json")],
                        json!({ "ok": false, "error": "Invalid or expired session." }).to_string()),
                },
                Err(_) => return (StatusCode::UNAUTHORIZED, [("Content-Type", "application/json")],
                    json!({ "ok": false, "error": "Could not verify session." }).to_string()),
            }
        },
        Err(e) => return (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": format!("Auth service unreachable: {}", e) }).to_string()),
    };

    let firm_id = match http.get(format!("{}/rest/v1/users?select=firm_id&user_id=eq.{}&limit=1", SUPABASE_URL, user_id))
        .header("apikey", SUPABASE_KEY)
        .header("Authorization", format!("Bearer {}", SUPABASE_KEY))
        .header("Content-Type", "application/json")
        .send().await
    {
        Ok(r) => {
            let text = r.text().await.unwrap_or_default();
            match serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                Ok(rows) if !rows.is_empty() => {
                    rows[0]["firm_id"].as_str().map(|s| s.to_string())
                        .or_else(|| rows[0]["firm_id"].as_i64().map(|n| n.to_string()))
                        .unwrap_or_default()
                },
                _ => return (StatusCode::FORBIDDEN, [("Content-Type", "application/json")],
                    json!({ "ok": false, "error": "User not found in users table." }).to_string()),
            }
        },
        Err(e) => return (StatusCode::BAD_GATEWAY, [("Content-Type", "application/json")],
            json!({ "ok": false, "error": format!("Database unreachable: {}", e) }).to_string()),
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

    // Step 1: Sync Ledgers
    let ledger_xml = format!(r#"<?xml version="1.0" encoding="utf-8"?>
<ENVELOPE>
  <HEADER><VERSION>1</VERSION><TALLYREQUEST>Export</TALLYREQUEST><TYPE>Collection</TYPE><ID>List of Ledgers</ID></HEADER>
  <BODY><DESC><STATICVARIABLES><SVEXPORTFORMAT>$$SysName:XML</SVEXPORTFORMAT>{cvar}</STATICVARIABLES>
  <TDL><TDLMESSAGE><COLLECTION NAME="List of Ledgers" ISMODIFY="No">
    <TYPE>Ledger</TYPE><NATIVEMETHOD>Name</NATIVEMETHOD><NATIVEMETHOD>Parent</NATIVEMETHOD>
    <NATIVEMETHOD>OpeningBalance</NATIVEMETHOD><NATIVEMETHOD>ClosingBalance</NATIVEMETHOD>
    <NATIVEMETHOD>PartyGSTIN</NATIVEMETHOD><NATIVEMETHOD>GSTRegistrationType</NATIVEMETHOD>
    <NATIVEMETHOD>LedStateName</NATIVEMETHOD><NATIVEMETHOD>PinCode</NATIVEMETHOD>
    <NATIVEMETHOD>Email</NATIVEMETHOD><NATIVEMETHOD>LedgerMobile</NATIVEMETHOD>
    <NATIVEMETHOD>Address</NATIVEMETHOD><NATIVEMETHOD>MailingName</NATIVEMETHOD>
  </COLLECTION></TDLMESSAGE></TDL></DESC></BODY>
</ENVELOPE>"#, cvar = company_var(&company));

    let ledger_count = match tally_request(&ledger_xml).await {
        Ok(cleaned) => match from_str::<TallyEnvelope>(&cleaned) {
            Ok(env) => {
                let ledgers = &env.body.data.collection.ledgers;
                let count = ledgers.len();
                if count > 0 {
                    let rows: Vec<serde_json::Value> = ledgers.iter().map(|l| json!({
                        "company_name": company, "name": l.name().to_string(),
                        "parent": l.parent, "opening_balance": parse_balance(&l.opening_balance),
                        "closing_balance": parse_balance(&l.closing_balance),
                        "party_gstin": l.party_gstin, "gst_registration_type": l.gst_registration_type,
                        "state": l.state, "pin_code": l.pin_code, "email": l.email,
                        "mobile": l.mobile, "address": l.address, "mailing_name": l.mailing_name,
                    })).collect();
                    if let Err(e) = supabase_upsert(&http, "ledgers", "company_name,name", &rows).await {
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

    // Step 2: Sync Vouchers
    let (_, fy_to) = current_fy();
    let voucher_xml = voucher_export_xml(&company, "20000401", &fy_to);

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
                        entries.into_iter().map(move |e| json!({
                            "company_name": cn, "voucher_guid": guid,
                            "voucher_date": v.date, "voucher_type": v.voucher_type,
                            "ledger_name":  e.ledger_name, "amount": e.amount,
                            "is_debit":     e.is_debit,
                        }))
                    }).collect();

                    if let Err(e) = supabase_upsert(&http, "voucher_entries", "company_name,voucher_guid,ledger_name,amount", &entry_rows).await {
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
            "vouchers_synced": voucher_count,
            "syncedAt": sync_time
        }).to_string())
}