//! Card Lab — the development-only sandbox for the Smeech visual language
//! (brief §45–§47). It renders the card system's axes as explorable permutations
//! so we can discover and approve recipes without touching production pages:
//!
//!   1. **Recipe gallery** — one example per approved recipe + intended use.
//!   2. **Canonical cases** — the 12 reference specimens (brief §46).
//!   3. **Permutation matrices** — every material / size / edge / label / repair /
//!      variation, laid out in grids (brief §69).
//!   4. **Single-card previewer** — a plain `<form method=get>` (no JS) that builds
//!      one card from query-param axis selections.
//!
//! Everything is placeholder content; this page depends on no domain data. Cards
//! are rendered with a *fixed* seed date so the lab is reproducible for
//! screenshots (production pages seed from today).

use crate::card::{
    Card, CardSize, CatFamily, Edge, Label, LabelPlacement, LabelWidth, Material, Ornament, Recipe,
    Repair, RepairKind, RepairPos, Variation,
};
use crate::views::{self, stat};
use axum::{extract::Query, response::Html};
use chrono::NaiveDate;
use maud::{html, Markup};
use std::collections::HashMap;

/// Fixed seed date so the Lab renders identically across reloads (production
/// pages use `render_today`). A stable specimen sheet is easier to review.
fn seed() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 8, 28).unwrap()
}

/// `GET /lab` — the sandbox. Query params drive the single-card previewer only;
/// the galleries/matrices are static.
pub async fn page(Query(q): Query<HashMap<String, String>>) -> Html<String> {
    let body = html! {
        // Lab-only layout CSS, kept out of the production `STYLES` blob.
        style { (maud::PreEscaped(LAB_CSS)) }
        header.page-head {
            h1 { "Card Lab" }
            p { "Sandbox for the Smeech visual language — recipes, canonical cases, and every axis." }
        }
        (previewer(&q))
        (recipe_gallery())
        (canonical_cases())
        (axis_matrices())
        p.lab-caption { "Dev-only. Seeded " (seed()) " for reproducibility. " a href="/" { "← Overview" } }
    };
    Html(views::layout("Card Lab", "", body).into_string())
}

// ── 4. Single-card previewer ──────────────────────────────────────────────────

const MATERIALS: [&str; 6] = ["linen", "charcoal", "faded-blue", "indigo", "olive", "rust"];
const SIZES: [&str; 5] = ["s", "m", "wide", "tall", "hero"];
const EDGES: [&str; 4] = ["clean", "stitched", "frayed", "stitched-frayed"];
const PLACEMENTS: [&str; 4] = ["inset", "edge", "overlap", "floating"];
const WIDTHS: [&str; 5] = ["content", "short", "medium", "long", "sash"];
const REPAIR_KINDS: [&str; 5] = ["none", "corner", "edge", "overlap", "fold"];
const REPAIR_POS: [&str; 6] = [
    "top-left",
    "top-right",
    "bottom-left",
    "bottom-right",
    "left",
    "right",
];
const VARIATIONS: [&str; 3] = ["none", "subtle", "playful"];

fn previewer(q: &HashMap<String, String>) -> Markup {
    let g = |k: &str, d: &str| q.get(k).map(|s| s.as_str()).unwrap_or(d).to_string();

    let (mat, size, edge) = (
        g("material", "linen"),
        g("size", "wide"),
        g("edge", "stitched-frayed"),
    );
    let (lmat, lplace, lwidth) = (
        g("label_material", "indigo"),
        g("label_placement", "edge"),
        g("label_width", "medium"),
    );
    let ltext = g("label_text", "System health");
    let (rkind, rmat, rpos) = (
        g("repair_kind", "corner"),
        g("repair_material", "indigo"),
        g("repair_pos", "top-right"),
    );
    let variation = g("variation", "playful");

    let mut card = Card::new("lab-preview", demo_stat())
        .material(material(&mat))
        .size(card_size(&size))
        .edge(edge_of(&edge))
        .variation(variation_of(&variation))
        .label_full(
            Label::new(ltext.clone())
                .material(material(&lmat))
                .placement(placement(&lplace))
                .width(width(&lwidth)),
        );
    let kind = repair_kind(&rkind);
    if kind != RepairKind::None {
        card = card.repair(Repair {
            kind,
            material: Some(material(&rmat)),
            pos: repair_pos(&rpos),
        });
    }

    html! {
        section.lab-section {
            h2 { "Previewer" }
            div.lab-preview-wrap {
                // Live-round-trip form: change a select, Update, server re-renders.
                form.lab-controls method="get" action="/lab" {
                    (field("material", &mat, &MATERIALS))
                    (field("size", &size, &SIZES))
                    (field("edge", &edge, &EDGES))
                    (text_field("label_text", &ltext))
                    (field("label_material", &lmat, &MATERIALS))
                    (field("label_placement", &lplace, &PLACEMENTS))
                    (field("label_width", &lwidth, &WIDTHS))
                    (field("repair_kind", &rkind, &REPAIR_KINDS))
                    (field("repair_material", &rmat, &MATERIALS))
                    (field("repair_pos", &rpos, &REPAIR_POS))
                    (field("variation", &variation, &VARIATIONS))
                    button.lab-apply type="submit" { "Update" }
                }
                div.lab-stage.bento {
                    (card.render(seed()))
                }
            }
        }
    }
}

/// A labelled `<select>` for one axis.
fn field(name: &str, current: &str, opts: &[&str]) -> Markup {
    html! {
        label.lab-field {
            span { (name) }
            select name=(name) {
                @for o in opts {
                    option value=(o) selected[*o == current] { (o) }
                }
            }
        }
    }
}

fn text_field(name: &str, current: &str) -> Markup {
    html! {
        label.lab-field {
            span { (name) }
            input type="text" name=(name) value=(current);
        }
    }
}

// ── 1. Recipe gallery ─────────────────────────────────────────────────────────

fn recipe_gallery() -> Markup {
    let entries = [
        (
            Recipe::Quiet,
            "Quiet",
            "Technical telemetry — CPU, logs, metrics.",
        ),
        (
            Recipe::Information,
            "Information",
            "Calendars, lists, general data.",
        ),
        (
            Recipe::Healthy,
            "Healthy",
            "Stable operational state — backups, services.",
        ),
        (
            Recipe::Active,
            "Active",
            "Running jobs, processes, warm accents.",
        ),
        (
            Recipe::Feature,
            "Feature",
            "Fasting, health summary, hero cards.",
        ),
    ];
    html! {
        section.lab-section {
            h2 { "Approved recipes" }
            div.bento {
                @for (recipe, name, use_) in entries {
                    (Card::new(format!("lab-recipe-{name}"), html! {
                        (demo_stat())
                        p.muted { (use_) }
                    })
                        .recipe(recipe)
                        .label(name)
                        .variation(Variation::Playful)
                        .render(seed()))
                }
            }
        }
    }
}

// ── 2. Canonical cases ────────────────────────────────────────────────────────

fn canonical_cases() -> Markup {
    html! {
        section.lab-section {
            h2 { "Canonical cases" }
            div.bento {
                (Card::new("c-hero", html! { h2.card__lead { "Smeech is purring" } p.muted { "All systems healthy." } })
                    .recipe(Recipe::Feature).size(CardSize::Wide).ornament(Ornament::Smeech(CatFamily::Rest)).variation(Variation::Playful).render(seed()))
                (Card::new("c-metric", demo_single_stat()).recipe(Recipe::Quiet).size(CardSize::S).label("CPU").render(seed()))
                (Card::new("c-status", html! { p { "● Operational" } p.muted { "3 services up" } }).recipe(Recipe::Healthy).label("Status").render(seed()))
                (Card::new("c-job", html! { (demo_single_stat()) p.muted { "ETA 4m" } }).recipe(Recipe::Active).label("Backup").ornament(Ornament::Smeech(CatFamily::Active)).render(seed()))
                (Card::new("c-trend", demo_panel()).recipe(Recipe::Information).size(CardSize::Wide).label("Trends").render(seed()))
                (Card::new("c-fasting", demo_stat()).recipe(Recipe::Feature).size(CardSize::Wide).label("Intermittent fasting").variation(Variation::Playful).render(seed()))
                (Card::new("c-calendar", demo_list()).recipe(Recipe::Information).size(CardSize::Tall).label("Calendar").render(seed()))
                (Card::new("c-log", html! { div.log { span.t { "12:01 refresh ok" } span.last { "12:02 idle" } } }).recipe(Recipe::Quiet).size(CardSize::Wide).label("Log").render(seed()))
                (Card::new("c-services", demo_list()).recipe(Recipe::Healthy).size(CardSize::Tall).label("Services").render(seed()))
                (Card::new("c-network", demo_single_stat()).recipe(Recipe::Active).label("Network").render(seed()))
                (Card::new("c-storage", demo_single_stat()).recipe(Recipe::Information).label("Storage").render(seed()))
                (Card::new("c-empty", demo_empty()).recipe(Recipe::Quiet).label("Empty state").render(seed()))
            }
        }
    }
}

// ── 3. Axis permutation matrices ──────────────────────────────────────────────

fn axis_matrices() -> Markup {
    html! {
        section.lab-section {
            h2 { "Materials" }
            div.bento {
                @for m in MATERIALS {
                    (Card::new(format!("m-{m}"), demo_single_stat())
                        .material(material(m)).edge(Edge::Stitched).label(m).render(seed()))
                }
            }
        }
        section.lab-section {
            h2 { "Sizes" }
            div.bento {
                @for s in SIZES {
                    (Card::new(format!("s-{s}"), demo_single_stat())
                        .recipe(Recipe::Information).size(card_size(s)).label(s).render(seed()))
                }
            }
        }
        section.lab-section {
            h2 { "Edges" }
            div.bento {
                @for e in EDGES {
                    (Card::new(format!("e-{e}"), demo_single_stat())
                        .material(Material::Linen).edge(edge_of(e)).label(e).render(seed()))
                }
            }
        }
        section.lab-section {
            h2 { "Label placements" }
            div.bento {
                @for p in PLACEMENTS {
                    (Card::new(format!("lp-{p}"), demo_single_stat())
                        .recipe(Recipe::Feature)
                        .label_full(Label::new(p).material(Material::Indigo).placement(placement(p)).width(LabelWidth::Medium))
                        .render(seed()))
                }
            }
        }
        section.lab-section {
            h2 { "Label widths" }
            div.bento {
                @for w in WIDTHS {
                    (Card::new(format!("lw-{w}"), demo_single_stat())
                        .recipe(Recipe::Feature)
                        .label_full(Label::new(w).material(Material::Indigo).placement(LabelPlacement::Edge).width(width(w)))
                        .render(seed()))
                }
            }
        }
        section.lab-section {
            h2 { "Repairs (kind · position)" }
            div.bento {
                @for k in REPAIR_KINDS {
                    @if k != "none" {
                        (Card::new(format!("rk-{k}"), demo_single_stat())
                            .material(Material::Charcoal).edge(Edge::Stitched).label(k)
                            .repair(Repair { kind: repair_kind(k), material: Some(Material::Indigo), pos: RepairPos::TopRight })
                            .render(seed()))
                    }
                }
                @for pos in REPAIR_POS {
                    (Card::new(format!("rp-{pos}"), demo_single_stat())
                        .material(Material::Charcoal).edge(Edge::Stitched).label(pos)
                        .repair(Repair { kind: RepairKind::Corner, material: Some(Material::Rust), pos: repair_pos(pos) })
                        .render(seed()))
                }
            }
        }
        section.lab-section {
            h2 { "Variation levels" }
            p.lab-caption { "Same recipe, different seeds — decoration drifts within recipe bounds." }
            @for v in VARIATIONS {
                h3.lab-sub { (v) }
                div.bento {
                    @for i in 0..4 {
                        (Card::new(format!("var-{v}-{i}"), demo_single_stat())
                            .recipe(Recipe::Feature).label(format!("seed {i}")).variation(variation_of(v))
                            .render(seed()))
                    }
                }
            }
        }
        section.lab-section {
            h2 { "Smeech pose families" }
            p.lab-caption { "The recipe permits a family; variation picks the pose. Force one with .ornament()." }
            div.bento {
                @for (fam, name) in [
                    (CatFamily::Rest, "Rest"), (CatFamily::Watch, "Watch"),
                    (CatFamily::Active, "Active"), (CatFamily::Play, "Play"),
                    (CatFamily::Attention, "Attention"),
                ] {
                    (Card::new(format!("smeech-{name}"), html! { p.muted { (name) } br; p.empty { "quiet · healthy · active · done · warn" } })
                        .recipe(Recipe::Feature).size(CardSize::M).material(Material::Charcoal)
                        .label(name).ornament(Ornament::Smeech(fam)).variation(Variation::Playful)
                        .render(seed()))
                }
                // escape hatch: force decoration off on a Feature card
                (Card::new("smeech-none", demo_single_stat())
                    .recipe(Recipe::Feature).label("no Smeech").ornament(Ornament::None).render(seed()))
            }
        }
        section.lab-section {
            h2 { "Smeech over the label" }
            p.lab-caption { ".smeech_label() perches a peeking Smeech over the cloth tag." }
            div.bento {
                @for (i, (mat, name)) in [
                    (Material::FadedBlue, "Calendar"), (Material::Charcoal, "System"),
                    (Material::Olive, "Backups"), (Material::Linen, "Fasting"),
                ].into_iter().enumerate() {
                    (Card::new(format!("sl-{i}"), demo_single_stat())
                        .recipe(Recipe::Information).material(mat).ornament(Ornament::None)
                        .label(name).smeech_label().variation(Variation::Playful).render(seed()))
                }
            }
        }
        section.lab-section {
            h2 { "Jagged silhouette (prototype)" }
            p.lab-caption { ".jagged() gives a rough-cut cloth-scrap edge (torn mask on a fabric layer; content stays rectangular)." }
            div.bento {
                @for (i, mat) in [Material::Linen, Material::FadedBlue, Material::Olive, Material::Rust, Material::Indigo, Material::Charcoal].into_iter().enumerate() {
                    (Card::new(format!("jag-{i}"), demo_single_stat())
                        .recipe(Recipe::Feature).material(mat).edge(Edge::Stitched)
                        .label("Jagged").jagged().ornament(Ornament::None).variation(Variation::Playful)
                        .render(seed()))
                }
            }
        }
    }
}

// ── Placeholder content ───────────────────────────────────────────────────────

fn demo_stat() -> Markup {
    html! { div.stat-row { (stat("18.5", "hrs fasting")) (stat("172.3", "lbs")) } }
}
fn demo_single_stat() -> Markup {
    html! { (stat("42%", "load")) }
}
fn demo_list() -> Markup {
    html! {
        ul.list {
            li.row { span.when { "Mon 09:00" } span.title { "Standup" } }
            li.row { span.when { "Tue 18:30" } span.title { "Gig at Bear's" } span.meta { "Shreveport" } }
            li.row { span.when { "Fri 12:00" } span.title { "Backup window" } }
        }
    }
}
fn demo_panel() -> Markup {
    html! { div.panel { p.muted { "calm content surface — charts/grids live here" } } }
}
fn demo_empty() -> Markup {
    html! { p.empty { "Nothing here yet." } }
}

// ── Slug → enum mapping (previewer + matrices) ────────────────────────────────

fn material(s: &str) -> Material {
    match s {
        "charcoal" => Material::Charcoal,
        "faded-blue" => Material::FadedBlue,
        "indigo" => Material::Indigo,
        "olive" => Material::Olive,
        "rust" => Material::Rust,
        _ => Material::Linen,
    }
}
fn card_size(s: &str) -> CardSize {
    match s {
        "s" => CardSize::S,
        "wide" => CardSize::Wide,
        "tall" => CardSize::Tall,
        "hero" => CardSize::Hero,
        _ => CardSize::M,
    }
}
fn edge_of(s: &str) -> Edge {
    match s {
        "clean" => Edge::Clean,
        "frayed" => Edge::Frayed,
        "stitched-frayed" => Edge::StitchedFrayed,
        _ => Edge::Stitched,
    }
}
fn placement(s: &str) -> LabelPlacement {
    match s {
        "inset" => LabelPlacement::Inset,
        "overlap" => LabelPlacement::Overlap,
        "floating" => LabelPlacement::Floating,
        _ => LabelPlacement::Edge,
    }
}
fn width(s: &str) -> LabelWidth {
    match s {
        "content" => LabelWidth::Content,
        "short" => LabelWidth::Short,
        "long" => LabelWidth::Long,
        "sash" => LabelWidth::Sash,
        _ => LabelWidth::Medium,
    }
}
fn repair_kind(s: &str) -> RepairKind {
    match s {
        "corner" => RepairKind::Corner,
        "edge" => RepairKind::Edge,
        "overlap" => RepairKind::Overlap,
        "fold" => RepairKind::Fold,
        _ => RepairKind::None,
    }
}
fn repair_pos(s: &str) -> RepairPos {
    match s {
        "top-left" => RepairPos::TopLeft,
        "bottom-left" => RepairPos::BottomLeft,
        "bottom-right" => RepairPos::BottomRight,
        "left" => RepairPos::Left,
        "right" => RepairPos::Right,
        _ => RepairPos::TopRight,
    }
}
fn variation_of(s: &str) -> Variation {
    match s {
        "none" => Variation::None,
        "playful" => Variation::Playful,
        _ => Variation::Subtle,
    }
}

const LAB_CSS: &str = r#"
.lab-section { margin: 0 0 var(--s-6); }
.lab-section > h2 {
  font-family: var(--font-mono); font-size: 0.8rem; letter-spacing: 0.16em;
  text-transform: uppercase; color: var(--accent); margin: 0 0 var(--s-4);
  border-bottom: 1px dashed var(--stitch-charcoal); padding-bottom: var(--s-2);
}
.lab-sub { font-family: var(--font-mono); font-size: 0.72rem; letter-spacing: 0.1em;
  text-transform: uppercase; color: var(--mut-charcoal); margin: var(--s-4) 0 var(--s-2); }
.lab-caption { color: var(--mut-charcoal); font-size: 0.85rem; }
.lab-caption a { color: var(--accent); }
.lab-preview-wrap { display: grid; grid-template-columns: 260px 1fr; gap: var(--s-5); align-items: start; }
.lab-controls { display: grid; gap: var(--s-2); padding: var(--s-4); background: var(--bg-2);
  border-radius: var(--radius); border: 1px solid var(--stitch-charcoal); }
.lab-field { display: grid; gap: 2px; font-size: 0.72rem; letter-spacing: 0.06em;
  text-transform: uppercase; color: var(--mut-charcoal); }
.lab-field select, .lab-field input {
  font: inherit; font-size: 0.85rem; text-transform: none; letter-spacing: 0;
  padding: 0.3rem 0.4rem; border-radius: 6px; background: var(--bg); color: var(--on-charcoal);
  border: 1px solid var(--stitch-charcoal); min-height: 34px;
}
.lab-apply { margin-top: var(--s-2); padding: 0.5rem; min-height: 40px; border-radius: 8px;
  background: linear-gradient(145deg, #5a3a22, #3f2716); color: #f3e3c4; border: 0;
  font: inherit; font-weight: 700; letter-spacing: 0.04em; cursor: pointer; }
.lab-stage {
  /* Keep the single-card preview visually diagnostic: auto-fit can collapse a
     one-item grid to one usable column, hiding the size-axis spans. */
  display: grid; grid-template-columns: repeat(2, minmax(0, 1fr));
  grid-auto-rows: minmax(120px, auto); grid-auto-flow: dense; gap: var(--s-4);
  align-content: start; padding: var(--s-6) var(--s-5); background: var(--bg-2);
  border-radius: var(--radius); min-height: 240px;
}
@media (max-width: 720px) {
  .lab-preview-wrap { grid-template-columns: 1fr; }
  .lab-stage { grid-template-columns: 1fr; }
}
"#;
