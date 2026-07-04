//! Phase 8: peacock's own MCP surface (FR-M-1..3, ACC-5/12) against a **real**
//! escurel — `tools/call render_report` returns structuredContent + the linked
//! `ui://` resource; `resources/read` returns the iframe; a drill is a fresh
//! render with absolute params.

use std::sync::Arc;

use peacock_core::EscurelData;
use peacock_server::{AppState, serve};
use peacock_test_support::{NW_REPORT, NorthwindEscurel};
use serde_json::{Value, json};

async fn start() -> (NorthwindEscurel, String) {
    let nw = NorthwindEscurel::spawn().await;
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html>",
        flutter_dir: None,
        flutter_app_url: None,
        themes: peacock_rasterizer::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(async move { serve(addr, state).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    (nw, format!("http://{addr}"))
}

async fn rpc(base: &str, method: &str, params: Value) -> Value {
    reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn initialize_and_tools_list() {
    let (nw, base) = start().await;
    let init = rpc(&base, "initialize", json!({})).await;
    assert_eq!(init["result"]["serverInfo"]["name"], "peacock");
    assert!(init["result"]["capabilities"].get("resources").is_some());

    let tools = rpc(&base, "tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"render_report"));
    nw.shutdown().await;
}

#[tokio::test]
async fn tools_call_links_structured_content_and_ui_resource() {
    // FR-M-2: the tool result carries structuredContent AND the ui:// link.
    let (nw, base) = start().await;
    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "render_report", "arguments": { "report_id": NW_REPORT } }),
    )
    .await;
    let res = &r["result"];
    assert_eq!(res["isError"], false);
    assert_eq!(
        res["structuredContent"]["rows"].as_array().unwrap().len(),
        16
    );
    assert_eq!(
        res["_meta"]["ui"]["resourceUri"],
        format!("ui://peacock/{NW_REPORT}")
    );
    nw.shutdown().await;
}

#[tokio::test]
async fn resources_read_serves_the_iframe_runtime() {
    // FR-M-1: resources/read of the ui:// URI returns the iframe HTML, with the
    // report id injected.
    let (nw, base) = start().await;
    let r = rpc(
        &base,
        "resources/read",
        json!({ "uri": format!("ui://peacock/{NW_REPORT}") }),
    )
    .await;
    let c = &r["result"]["contents"][0];
    assert_eq!(c["mimeType"], "text/html");
    let html = c["text"].as_str().unwrap();
    assert!(html.contains("peacock report"));
    assert!(
        html.contains(NW_REPORT),
        "report id injected into the iframe"
    );
    assert!(
        html.contains("updateModelContext"),
        "iframe pushes view state"
    );
    // No flutter_app_url configured → the self-contained iframe, not the shim.
    assert!(
        !html.contains("peacock-app"),
        "without flutter_app_url, the ui:// resource is the self-contained iframe"
    );
    nw.shutdown().await;
}

#[tokio::test]
async fn resources_read_serves_the_flutter_shim_when_app_url_is_set() {
    // With a host-reachable flutter_app_url, the ui:// resource is the Flutter
    // shim that nests the hosted /app/ bundle (the direct-reachable deployment).
    let nw = NorthwindEscurel::spawn().await;
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html>",
        flutter_dir: None,
        flutter_app_url: Some("http://peacock.example/app/".into()),
        themes: peacock_rasterizer::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(async move { serve(addr, state).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let r = rpc(
        &format!("http://{addr}"),
        "resources/read",
        json!({ "uri": format!("ui://peacock/{NW_REPORT}") }),
    )
    .await;
    let html = r["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(
        html.contains("peacock-app"),
        "the shim nests the Flutter app"
    );
    assert!(
        html.contains("http://peacock.example/app/"),
        "shim points at the configured app base"
    );
    assert!(html.contains(NW_REPORT));
    nw.shutdown().await;
}

#[tokio::test]
async fn in_iframe_drill_is_a_fresh_render() {
    // FR-M-3 / FR-X-2: a callServerTool drill = tools/call with absolute params.
    let (nw, base) = start().await;
    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "render_report",
                "arguments": { "report_id": NW_REPORT, "params": { "category": "Beverages" } } }),
    )
    .await;
    let sc = &r["result"]["structuredContent"];
    assert_eq!(sc["current_params"]["category"], "Beverages");
    // Every returned row is Beverages (the drilled view).
    assert!(
        sc["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["category"] == "Beverages")
    );
    nw.shutdown().await;
}

// ── instance reports on the MCP surface ──

const ACCOUNT_SKILL: &str = "---\ntype: skill\nid: account\n\
    description: A customer account.\nrequired_frontmatter: [id, name]\n---\n# account\n";

/// The body carries hostile markup — the artifact transports it RAW (typed
/// contract); only the RENDERER escapes. The iframe test below pins the
/// escape path.
const HOSTILE_ACCOUNT: &str = "---\ntype: instance\nskill: account\nid: beverages-gmbh\n\
    name: \"Beverages <script>alert(1)</script>\"\n---\n# Beverages GmbH\n\n\
    Note: <script>alert(1)</script> rides verbatim.\n";

const CUSTOMER_REPORT: &str = "---\ntype: skill\nid: customer-report\nrender: a2ui\n\
    description: One customer account as a card.\n\
    params:\n  account: { type: string }\n\
    instances:\n  acct: \"[[account::{account}]]\"\n\
    views:\n\
      - { kind: frontmatter, instance: acct, keys: [name], label: Account }\n\
      - { kind: markdown, instance: acct }\n\
      - { kind: timeline, instance: acct, limit: 5 }\n\
    ---\n";

async fn start_with_customer_report() -> (NorthwindEscurel, String) {
    let nw = NorthwindEscurel::spawn_with(peacock_test_support::NorthwindOpts {
        extra_skills: vec![
            ("account".to_owned(), ACCOUNT_SKILL.to_owned()),
            ("customer-report".to_owned(), CUSTOMER_REPORT.to_owned()),
        ],
        extra_instances: vec![(
            "account".to_owned(),
            "beverages-gmbh".to_owned(),
            HOSTILE_ACCOUNT.to_owned(),
        )],
        ..Default::default()
    })
    .await;
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html>",
        flutter_dir: None,
        flutter_app_url: None,
        themes: peacock_rasterizer::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(async move { serve(addr, state).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    (nw, format!("http://{addr}"))
}

#[tokio::test]
async fn tools_call_instance_report_carries_the_typed_contract() {
    let (nw, base) = start_with_customer_report().await;
    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "render_report", "arguments": {
            "report_id": "customer-report",
            "params": { "account": "beverages-gmbh" },
        }}),
    )
    .await;
    let res = &r["result"];
    assert_eq!(res["isError"], false, "{r}");
    let inst = &res["structuredContent"]["instances"]["acct"];
    assert_eq!(inst["id"], "beverages-gmbh");
    assert_eq!(inst["facts"][0]["key"], "name");
    // RAW in the typed contract — the consumer (iframe/chat mapper) encodes.
    assert!(
        inst["markdown"].as_str().unwrap().contains("<script>"),
        "the artifact transports the body verbatim: {inst}"
    );
    assert!(inst["events"].as_array().is_some(), "timeline contract");
    assert_eq!(
        res["_meta"]["ui"]["resourceUri"], "ui://peacock/customer-report?account=beverages-gmbh",
        "the caller's params ride the ui link"
    );
    // The chat surface: a chartless instance report still carries a PNG —
    // the themed INSTANCE CARD (protocol adaptation on this endpoint too).
    assert!(
        res["_meta"]["png_base64"]
            .as_str()
            .is_some_and(|b| !b.is_empty()),
        "instance-card png on tools/call: {}",
        res["_meta"]
    );
    nw.shutdown().await;
}

#[tokio::test]
async fn resources_read_is_themed_and_escapes_before_formatting() {
    // The served runtime carries the RESOLVED theme css (brand ⊕ host — the
    // "apply styling" contract on the MCP endpoint) and its renderer only
    // ever writes escaped content (escape-then-format; the browser-level
    // gold check drives the semantics tree per CLAUDE.md).
    let (nw, base) = start_with_customer_report().await;
    let r = rpc(
        &base,
        "resources/read",
        json!({ "uri": "ui://peacock/customer-report" }),
    )
    .await;
    let html = r["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(
        !html.contains("__THEME_CSS__"),
        "the theme placeholder is substituted"
    );
    assert!(
        html.contains("peacock theme — host flavor"),
        "the resolved theme css is embedded: {}",
        &html[..600]
    );
    // The escape path: dynamic content goes through esc() before innerHTML,
    // and the markdown renderer formats the ESCAPED line.
    assert!(html.contains("const esc ="));
    assert!(html.contains("esc(raw.trim())"), "escape-then-format");
    nw.shutdown().await;
}

#[tokio::test]
async fn caller_params_ride_the_resource_uri_and_seed_the_iframe() {
    // A param-REQUIRED report (nba/customer style) is unrenderable from a
    // bare ui:// URI — the CALLER's params must ride the resource link and
    // seed the served runtime's first render (the drill loop then owns
    // them). A caller that sent none keeps the bare URI (byte-compat).
    let (nw, base) = start_with_customer_report().await;

    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "render_report", "arguments": {
            "report_id": "customer-report",
            "params": { "account": "beverages-gmbh" },
        }}),
    )
    .await;
    assert_eq!(
        r["result"]["_meta"]["ui"]["resourceUri"],
        "ui://peacock/customer-report?account=beverages-gmbh",
        "the caller's params ride the resource link: {}",
        r["result"]["_meta"]
    );

    // The served runtime carries those params as its initial render vector.
    let res = rpc(
        &base,
        "resources/read",
        json!({ "uri": "ui://peacock/customer-report?account=beverages-gmbh" }),
    )
    .await;
    let html = res["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(
        html.contains(r#""account":"beverages-gmbh""#),
        "the iframe seeds the initial params: {}",
        &html[..500]
    );
    assert!(
        !html.contains("__INITIAL_PARAMS__"),
        "placeholder substituted"
    );
    assert!(!html.contains("__THEME_CSS__"));

    // No caller params → the bare URI, exactly as before.
    let bare = rpc(
        &base,
        "tools/call",
        json!({ "name": "render_report", "arguments": { "report_id": NW_REPORT } }),
    )
    .await;
    assert_eq!(
        bare["result"]["_meta"]["ui"]["resourceUri"],
        format!("ui://peacock/{NW_REPORT}"),
    );

    nw.shutdown().await;
}

#[tokio::test]
async fn the_iframe_surfaces_render_errors_and_derives_drill_chips() {
    // The runtime must SHOW a failed render (an invalid params error), never
    // the empty state; and its drill chips exist only for reports that
    // DECLARE the drill dimension (param_schema-driven, nothing hardcoded).
    let (nw, base) = start_with_customer_report().await;
    let res = rpc(
        &base,
        "resources/read",
        json!({ "uri": "ui://peacock/customer-report" }),
    )
    .await;
    let html = res["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(
        html.contains("renderError") || html.contains("data.error"),
        "the runtime has an error-surfacing path"
    );
    assert!(
        html.contains("param_schema") && !html.contains("allCats"),
        "drill chips derive from the declared schema, not a hardcoded list"
    );
    nw.shutdown().await;
}

// ── The `document` pseudo-report + skill-page actions (Sources targets) ──

/// An account skill with document affordances: a prompt action and an
/// event action (`actions:` frontmatter — the skill page is the contract).
const ACTIONS_ACCOUNT_SKILL: &str = "---\ntype: skill\nid: account\n\
    description: A customer account.\nrequired_frontmatter: [id, name]\n\
    optional_frontmatter: [status]\n\
    actions:\n\
    \x20 - name: propose-nba\n\
    \x20   kind: prompt\n\
    \x20   label: Propose next best action\n\
    \x20   prompt: \"whats the next best action for {id}?\"\n\
    \x20 - name: renewal-at-risk\n\
    \x20   kind: event\n\
    \x20   label: Flag renewal at risk\n\
    \x20   event: follow_up\n\
    \x20   title: \"{frontmatter.name}\"\n\
    \x20   body: \"renewal at risk (flagged from the document)\"\n\
    ---\n# account\n";

const PLAIN_ACCOUNT: &str = "---\ntype: instance\nskill: account\nid: beverages-gmbh\n\
    name: Beverages GmbH\nstatus: follow_up\n---\n# Beverages GmbH\n\n\
    See [[email::mail-1]] for the renewal thread.\n";

async fn start_with_actions() -> (NorthwindEscurel, String) {
    let nw = NorthwindEscurel::spawn_with(peacock_test_support::NorthwindOpts {
        extra_skills: vec![("account".to_owned(), ACTIONS_ACCOUNT_SKILL.to_owned())],
        extra_instances: vec![(
            "account".to_owned(),
            "beverages-gmbh".to_owned(),
            PLAIN_ACCOUNT.to_owned(),
        )],
        ..Default::default()
    })
    .await;
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html>",
        flutter_dir: None,
        flutter_app_url: None,
        themes: peacock_rasterizer::ThemeRegistry::builtin(),
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(async move { serve(addr, state).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    (nw, format!("http://{addr}"))
}

#[tokio::test]
async fn tools_list_advertises_emit_document_event() {
    let (nw, base) = start().await;
    let tools = rpc(&base, "tools/list", json!({})).await;
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"emit_document_event"), "{names:?}");
    nw.shutdown().await;
}

#[tokio::test]
async fn document_render_carries_the_action_contract_and_the_uri_round_trips() {
    let (nw, base) = start_with_actions().await;

    // tools/call the pseudo-report: the document contract rides the result.
    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "render_report", "arguments": {
            "report_id": "document",
            "params": { "skill": "account", "id": "beverages-gmbh" },
        }}),
    )
    .await;
    let res = &r["result"];
    assert_eq!(res["isError"], false, "{r}");
    let doc = &res["structuredContent"]["document"];
    assert_eq!(doc["skill"], "account");
    assert_eq!(doc["id"], "beverages-gmbh");
    assert_eq!(doc["actions"][0]["kind"], "prompt");
    assert_eq!(
        doc["actions"][0]["prompt"],
        "whats the next best action for beverages-gmbh?"
    );
    // Event actions never ship their captured title/body.
    assert_eq!(doc["actions"][1]["kind"], "event");
    assert!(doc["actions"][1].get("title").is_none());
    // The caller params ride the minted URI (FR-M-2 + the params contract).
    assert_eq!(
        res["_meta"]["ui"]["resourceUri"],
        "ui://peacock/document?skill=account&id=beverages-gmbh"
    );

    // …and resources/read of that URI seeds the runtime: params island +
    // the action/wikilink/prompt machinery in the served HTML.
    let read = rpc(
        &base,
        "resources/read",
        json!({ "uri": "ui://peacock/document?skill=account&id=beverages-gmbh" }),
    )
    .await;
    let html = read["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(html.contains("\"skill\":\"account\"") && html.contains("\"id\":\"beverages-gmbh\""));
    assert!(html.contains("mcp:prompt"), "the prompt verb is wired");
    assert!(
        html.contains("emit_document_event"),
        "the event tool is wired"
    );
    assert!(html.contains("data-skill"), "wikilinks are navigable");

    nw.shutdown().await;
}

#[tokio::test]
async fn emit_document_event_captures_as_the_caller() {
    let (nw, base) = start_with_actions().await;

    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "emit_document_event", "arguments": {
            "skill": "account", "id": "beverages-gmbh", "action": "renewal-at-risk",
        }}),
    )
    .await;
    assert_eq!(r["result"]["ok"], true, "{r}");
    let event_id = r["result"]["event_id"].as_str().unwrap();
    assert!(!event_id.is_empty());

    // The event landed in escurel with the SERVER-substituted templates.
    let client = nw.sales_client().await;
    let inbox = client
        .list_inbox(escurel_client::ListInboxRequest { limit: 50 })
        .await
        .expect("list inbox");
    let ev = inbox
        .events
        .iter()
        .find(|e| e.event_id == event_id)
        .expect("the captured event is in the inbox");
    assert_eq!(ev.label_skill, "follow_up");
    assert_eq!(
        ev.title, "Beverages GmbH",
        "{{frontmatter.name}} substituted"
    );
    assert_eq!(ev.source, "peacock");

    nw.shutdown().await;
}

#[tokio::test]
async fn emit_document_event_fails_closed() {
    let (nw, base) = start_with_actions().await;

    // A prompt action is NOT emittable.
    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "emit_document_event", "arguments": {
            "skill": "account", "id": "beverages-gmbh", "action": "propose-nba",
        }}),
    )
    .await;
    assert!(
        r["error"]["message"]
            .as_str()
            .unwrap()
            .contains("propose-nba"),
        "{r}"
    );

    // An undeclared action name.
    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "emit_document_event", "arguments": {
            "skill": "account", "id": "beverages-gmbh", "action": "made-up",
        }}),
    )
    .await;
    assert!(r.get("error").is_some(), "{r}");

    // Smuggled identifiers are rejected before any escurel read.
    let r = rpc(
        &base,
        "tools/call",
        json!({ "name": "emit_document_event", "arguments": {
            "skill": "../etc", "id": "beverages-gmbh", "action": "renewal-at-risk",
        }}),
    )
    .await;
    assert!(r.get("error").is_some(), "{r}");

    // Nothing was captured by any of the refusals.
    let client = nw.sales_client().await;
    let inbox = client
        .list_inbox(escurel_client::ListInboxRequest { limit: 50 })
        .await
        .expect("list inbox");
    assert!(
        inbox.events.iter().all(|e| e.source != "peacock"),
        "no peacock event captured: {:?}",
        inbox.events
    );

    nw.shutdown().await;
}

// ── get_theme: peacock owns ALL theming; adapters consume it as data ──

#[tokio::test]
async fn get_theme_returns_the_resolved_brand_as_data() {
    // In-proc state with a registered brand (what PEACOCK_BRAND_CSS does at
    // boot): the tool returns the RESOLVED chrome + css for tenant ⊕ host.
    let nw = NorthwindEscurel::spawn().await;
    let mut themes = peacock_rasterizer::ThemeRegistry::builtin();
    themes.register_brand(
        "acme",
        ":root { --pk-name: \"DataZoo\"; --pk-brand: #1a73e8; \
         --pk-logo: https://brand.example/logo.png; --pk-logo-style: banner; }",
    );
    let state = Arc::new(AppState {
        escurel: EscurelData::new(nw.endpoint()),
        principal: nw.sales_principal(),
        png_scale: 2.0,
        demo_html: "<!doctype html>",
        flutter_dir: None,
        flutter_app_url: None,
        themes,
        triton_url: None,
        upstream_capture: Default::default(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    tokio::spawn(async move { serve(addr, state).await.unwrap() });
    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/healthz")).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let base = format!("http://{addr}");

    let tools = rpc(&base, "tools/list", json!({})).await;
    assert!(
        tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["name"] == "get_theme"),
        "{tools}"
    );

    let r = rpc(&base, "tools/call", json!({ "name": "get_theme" })).await;
    let theme = &r["result"];
    assert_eq!(theme["brand"], "acme", "{r}");
    assert_eq!(theme["title"], "DataZoo");
    assert_eq!(theme["logo_url"], "https://brand.example/logo.png");
    assert_eq!(theme["logo_style"], "banner");
    assert_eq!(theme["brand_color"], "#1a73e8");
    assert!(
        theme["css"].as_str().unwrap().contains("--pk-name"),
        "the composed css rides along for web chrome"
    );

    nw.shutdown().await;
}

#[tokio::test]
async fn get_theme_falls_back_to_stock_for_unknown_brands() {
    // An unconfigured deployment gets the stock look — data, never an error.
    let (nw, base) = start().await;
    let r = rpc(&base, "tools/call", json!({ "name": "get_theme" })).await;
    let theme = &r["result"];
    assert!(r.get("error").is_none(), "{r}");
    assert_eq!(theme["logo_style"], "avatar");
    assert_eq!(theme["brand_color"], "#0f6cbd", "stock brand colour: {r}");
    assert!(theme["title"].is_null() && theme["logo_url"].is_null());
    nw.shutdown().await;
}

#[tokio::test]
async fn peacock_brand_css_boots_the_deployment_brand() {
    // The REAL binary path: PEACOCK_BRAND_CSS registers the file under the
    // deployment tenant at boot; get_theme serves it.
    let dir = tempfile::tempdir().expect("tmpdir");
    let css_path = dir.path().join("brand.css");
    std::fs::write(
        &css_path,
        ":root { --pk-name: \"Initech\"; --pk-brand: #b71c1c; --pk-logo-style: banner; }",
    )
    .expect("write brand css");

    let peacock = peacock_test_support::PeacockProcess::spawn(std::collections::HashMap::from([
        ("PEACOCK_ESCUREL_URL".into(), "http://127.0.0.1:1".into()),
        ("PEACOCK_TENANT".into(), "initech".into()),
        (
            "PEACOCK_BRAND_CSS".into(),
            css_path.to_str().unwrap().to_string(),
        ),
    ]))
    .await;

    let r = rpc(
        &peacock.base_url().to_string(),
        "tools/call",
        json!({ "name": "get_theme" }),
    )
    .await;
    let theme = &r["result"];
    assert_eq!(theme["brand"], "initech", "{r}");
    assert_eq!(theme["title"], "Initech");
    assert_eq!(theme["brand_color"], "#b71c1c");
    assert_eq!(theme["logo_style"], "banner");

    peacock.terminate();
}
