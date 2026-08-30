//! The Smeech card vocabulary — a card as a composition of independent visual
//! axes (material, size, edge, label, adornments) bundled by named
//! **recipes**, rendered to Maud markup over the real handmade fabric assets
//! (`/static/boro/...`, embedded via `assets.rs`).
//!
//! **Structural vs surface (the core rule — brief §5/§6).**
//!   - *Structural* properties are recipe/card-controlled and seed-independent:
//!     material, size, label placement, and the ordered adornment list (patches,
//!     Smeech details, and future decoration kinds).
//!   - *Surface* properties vary by the deterministic `(day, card-id)` seed within
//!     recipe bounds: which label/adornment/fray/Smeech-pose **variant**, tiny
//!     rotation/offset. So an SSE re-render on the same day is byte-identical
//!     (no decoration jitter — brief §44); the look only drifts day to day.
//!
//! Labels are always real HTML `<h2>` text laid over a transparent cloth asset
//! (never baked into the image — brief §14). Cards stay `overflow: visible`;
//! decoration overhangs while the inner `.card__surface`/`.panel` clips data.

use chrono::NaiveDate;
use maud::{html, Markup, PreEscaped};

const BORO: &str = "/static/boro";

// ── Axes ─────────────────────────────────────────────────────────────────────

/// Fabric family (brief §6/§7). The `.material--*` class maps each to its
/// `--surface/--on/--mut/--stitch` roles + the real woven-cloth texture.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Material {
    Linen,
    Charcoal,
    FadedBlue,
    Indigo,
    Olive,
    Rust,
}

impl Material {
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Material::Linen => "linen",
            Material::Charcoal => "charcoal",
            Material::FadedBlue => "faded-blue",
            Material::Indigo => "indigo",
            Material::Olive => "olive",
            Material::Rust => "rust",
        }
    }
}

/// Card footprint in the bento grid (brief §23).
#[derive(Clone, Copy)]
pub enum CardSize {
    S,
    M,
    Wide,
    Tall,
    Hero,
}

impl CardSize {
    fn slug(self) -> &'static str {
        match self {
            CardSize::S => "s",
            CardSize::M => "m",
            CardSize::Wide => "wide",
            CardSize::Tall => "tall",
            CardSize::Hero => "hero",
        }
    }
}

/// Edge construction (brief §11). Stitch is the standard scalable construction;
/// fray is a real transparent overlay on the `frayed`/`stitched-frayed` edges.
#[derive(Clone, Copy)]
pub enum Edge {
    Clean,
    Stitched,
    Frayed,
    StitchedFrayed,
}

impl Edge {
    fn slug(self) -> &'static str {
        match self {
            Edge::Clean => "clean",
            Edge::Stitched => "stitched",
            Edge::Frayed => "frayed",
            Edge::StitchedFrayed => "stitched-frayed",
        }
    }
    fn has_fray(self) -> bool {
        matches!(self, Edge::Frayed | Edge::StitchedFrayed)
    }
}

/// Where the label sits (brief §15). `Inset` and `Edge` are the canonical
/// everyday placements; `Overlap`/`Floating` remain as deliberate exceptions.
#[derive(Clone, Copy)]
pub enum LabelPlacement {
    Inset,
    Edge,
    Overlap,
    Floating,
}

impl LabelPlacement {
    fn slug(self) -> &'static str {
        match self {
            LabelPlacement::Inset => "inset",
            LabelPlacement::Edge => "edge",
            LabelPlacement::Overlap => "overlap",
            LabelPlacement::Floating => "floating",
        }
    }
}

/// Intentional label width (brief §16). `content`/`short`/`medium` are the
/// everyday sizes; `long`/`sash` are deliberate exceptions.
#[derive(Clone, Copy)]
pub enum LabelWidth {
    Content,
    Short,
    Medium,
    Long,
    Sash,
}

impl LabelWidth {
    fn slug(self) -> &'static str {
        match self {
            LabelWidth::Content => "content",
            LabelWidth::Short => "short",
            LabelWidth::Medium => "medium",
            LabelWidth::Long => "long",
            LabelWidth::Sash => "sash",
        }
    }
}

/// A cloth label carrying real text. Material/placement/width are `Option`:
/// unset means "take the recipe's default."
#[derive(Clone)]
pub struct Label {
    pub text: String,
    pub material: Option<Material>,
    pub placement: Option<LabelPlacement>,
    pub width: Option<LabelWidth>,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        Label {
            text: text.into(),
            material: None,
            placement: None,
            width: None,
        }
    }
    pub fn material(mut self, m: Material) -> Self {
        self.material = Some(m);
        self
    }
    pub fn placement(mut self, p: LabelPlacement) -> Self {
        self.placement = Some(p);
        self
    }
    pub fn width(mut self, w: LabelWidth) -> Self {
        self.width = Some(w);
        self
    }
}

/// Patch/repair shape (brief §18).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RepairKind {
    None,
    Corner,
    Edge,
    Overlap,
    Fold,
}

/// Where a repair attaches (brief §18). Deliberate, per card — not randomized.
#[derive(Clone, Copy)]
pub enum RepairPos {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Left,
    Right,
}

impl RepairPos {
    fn slug(self) -> &'static str {
        match self {
            RepairPos::TopLeft => "top-left",
            RepairPos::TopRight => "top-right",
            RepairPos::BottomLeft => "bottom-left",
            RepairPos::BottomRight => "bottom-right",
            RepairPos::Left => "left",
            RepairPos::Right => "right",
        }
    }
}

/// An explicit repair on a card. `kind` remains useful to the Card Lab and for
/// future repair-specific styling; position and material drive the current art.
#[derive(Clone)]
pub struct Repair {
    pub kind: RepairKind,
    pub material: Option<Material>,
    pub pos: RepairPos,
}

/// Smeech's semantic pose family (brief §33). The recipe *permits* a family;
/// variation picks a concrete pose within it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CatFamily {
    Rest,
    Watch,
    Active,
    Play,
    Attention,
}

impl CatFamily {
    /// Pose slugs available in this family (files `smeech-{slug}.webp`).
    fn poses(self) -> &'static [&'static str] {
        match self {
            CatFamily::Rest => &[
                "rest-curled",
                "rest-curled-lg",
                "rest-sidesleep",
                "rest-blanket",
            ],
            CatFamily::Watch => &[
                "watch-sit",
                "watch-back",
                "watch-loaf",
                "watch-peek-ledge",
                "watch-peek-pocket",
                "watch-drape",
            ],
            CatFamily::Active => &["active-walk", "active-prowl", "active-stretch"],
            CatFamily::Play => &["play-pounce", "play-back"],
            CatFamily::Attention => &["attention-paw"],
        }
    }
}

#[derive(Clone, Copy)]
pub enum Ornament {
    None,
    /// Permit Smeech from this family; the pose is chosen by the seed.
    Smeech(CatFamily),
}

/// A composable piece of card decoration. Recipes provide a starting list and
/// the builder can add, remove, or replace entries after choosing a recipe.
#[derive(Clone)]
pub enum Adornment {
    Repair(Repair),
    Smeech(CatFamily),
}

/// Categories used when changing recipe-provided adornments.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AdornmentKind {
    Repair,
    Smeech,
}

impl Adornment {
    fn kind(&self) -> AdornmentKind {
        match self {
            Adornment::Repair(_) => AdornmentKind::Repair,
            Adornment::Smeech(_) => AdornmentKind::Smeech,
        }
    }
}

/// How much decoration may vary between days/sessions (brief §29).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variation {
    None,
    Subtle,
    Playful,
}

/// A named family supplying coherent structural defaults (brief §26).
#[derive(Clone, Copy)]
pub enum Recipe {
    Quiet,
    Information,
    Healthy,
    Active,
    Feature,
}

/// Fully-resolved recipe defaults (structural). Builder overrides win.
struct Defaults {
    material: Material,
    edge: Edge,
    label_material: Material,
    label_placement: LabelPlacement,
    label_width: LabelWidth,
    /// Structural adornments supplied by this recipe. These are defaults, not
    /// limits: a card may remove them or add more entries.
    adornments: Vec<Adornment>,
}

impl Recipe {
    // Material pairings follow the matrix in brief §10.
    fn defaults(self) -> Defaults {
        match self {
            // Quiet — technical telemetry: minimal, inset label, no patch/Smeech.
            Recipe::Quiet => Defaults {
                material: Material::Charcoal,
                edge: Edge::Stitched,
                label_material: Material::Linen,
                label_placement: LabelPlacement::Inset,
                label_width: LabelWidth::Short,
                adornments: vec![],
            },
            // Information — calendars/lists/data: edge label, optional top-right indigo patch.
            Recipe::Information => Defaults {
                material: Material::FadedBlue,
                edge: Edge::StitchedFrayed,
                label_material: Material::Linen,
                label_placement: LabelPlacement::Edge,
                label_width: LabelWidth::Short,
                adornments: vec![Adornment::Repair(Repair {
                    kind: RepairKind::Corner,
                    material: Some(Material::Indigo),
                    pos: RepairPos::TopRight,
                })],
            },
            // Healthy — stable operational state: olive, rust patch bottom-right.
            Recipe::Healthy => Defaults {
                material: Material::Olive,
                edge: Edge::StitchedFrayed,
                label_material: Material::Linen,
                label_placement: LabelPlacement::Edge,
                label_width: LabelWidth::Medium,
                adornments: vec![Adornment::Repair(Repair {
                    kind: RepairKind::Corner,
                    material: Some(Material::Rust),
                    pos: RepairPos::BottomRight,
                })],
            },
            // Active — jobs/processes: rust, indigo label, active Smeech permitted.
            Recipe::Active => Defaults {
                material: Material::Rust,
                edge: Edge::Stitched,
                label_material: Material::Indigo,
                label_placement: LabelPlacement::Edge,
                label_width: LabelWidth::Medium,
                adornments: vec![Adornment::Smeech(CatFamily::Active)],
            },
            // Feature — hero/fasting/health: linen, indigo label, indigo patch, resting Smeech.
            Recipe::Feature => Defaults {
                material: Material::Linen,
                edge: Edge::StitchedFrayed,
                label_material: Material::Indigo,
                label_placement: LabelPlacement::Edge,
                label_width: LabelWidth::Medium,
                adornments: vec![
                    Adornment::Repair(Repair {
                        kind: RepairKind::Corner,
                        material: Some(Material::Indigo),
                        pos: RepairPos::BottomRight,
                    }),
                    Adornment::Smeech(CatFamily::Rest),
                ],
            },
        }
    }
}

// ── Card builder ─────────────────────────────────────────────────────────────

/// A composed card. Build with [`Card::new`], chain axis setters; unset axes
/// resolve from the [`Recipe`]. Render with [`Card::render`]/[`Card::render_today`].
pub struct Card {
    id: String,
    content: Markup,
    recipe: Recipe,
    material: Option<Material>,
    size: CardSize,
    edge: Option<Edge>,
    label: Option<Label>,
    /// Entries added or replaced by the card, in render order.
    adornments: Vec<Adornment>,
    /// Recipe categories explicitly removed from this card.
    removed_adornments: Vec<AdornmentKind>,
    variation: Variation,
    extra_class: Option<String>,
    /// Perch a peeking Smeech over the top of the label (brief §34 interaction).
    smeech_label: bool,
    /// Give the card a rough-cut, non-rectangular cloth silhouette (torn mask).
    jagged: bool,
}

impl Card {
    pub fn new(id: impl Into<String>, content: Markup) -> Self {
        Card {
            id: id.into(),
            content,
            recipe: Recipe::Quiet,
            material: None,
            size: CardSize::M,
            edge: None,
            label: None,
            adornments: vec![],
            removed_adornments: vec![],
            variation: Variation::Subtle,
            extra_class: None,
            smeech_label: false,
            jagged: false,
        }
    }

    pub fn recipe(mut self, r: Recipe) -> Self {
        self.recipe = r;
        self
    }
    /// Perch a peeking Smeech over the label.
    pub fn smeech_label(mut self) -> Self {
        self.smeech_label = true;
        self
    }
    /// Give the card a rough-cut, non-rectangular cloth silhouette.
    pub fn jagged(mut self) -> Self {
        self.jagged = true;
        self
    }
    pub fn material(mut self, m: Material) -> Self {
        self.material = Some(m);
        self
    }
    pub fn size(mut self, s: CardSize) -> Self {
        self.size = s;
        self
    }
    pub fn edge(mut self, e: Edge) -> Self {
        self.edge = Some(e);
        self
    }
    /// Convenience: a label from just its text (material/placement/width from the recipe).
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(Label::new(text));
        self
    }
    pub fn label_full(mut self, l: Label) -> Self {
        self.label = Some(l);
        self
    }
    /// Add an adornment. Multiple repairs and Smeech details are supported.
    pub fn adornment(mut self, a: Adornment) -> Self {
        self.adornments.push(a);
        self
    }
    /// Add several adornments in order.
    #[allow(dead_code)]
    pub fn adornments(mut self, entries: impl IntoIterator<Item = Adornment>) -> Self {
        self.adornments.extend(entries);
        self
    }
    /// Remove all recipe-provided adornments of this category.
    pub fn remove_adornment(mut self, kind: AdornmentKind) -> Self {
        if !self.removed_adornments.contains(&kind) {
            self.removed_adornments.push(kind);
        }
        self
    }
    /// Alias for [`Card::remove_adornment`].
    #[allow(dead_code)]
    pub fn remove_adornments(self, kind: AdornmentKind) -> Self {
        self.remove_adornment(kind)
    }
    /// Remove all recipe-provided repairs (for example, keep Information but
    /// omit its default patch).
    #[allow(dead_code)]
    pub fn without_repairs(self) -> Self {
        self.remove_adornment(AdornmentKind::Repair)
    }
    /// Remove all recipe-provided Smeech entries.
    #[allow(dead_code)]
    pub fn without_smeech(self) -> Self {
        self.remove_adornment(AdornmentKind::Smeech)
    }
    /// Replace recipe adornments of this category with one explicit entry.
    pub fn replace_adornment(mut self, a: Adornment) -> Self {
        let kind = a.kind();
        self = self.remove_adornment(kind);
        self.adornments.retain(|existing| existing.kind() != kind);
        self.adornments.push(a);
        self
    }
    /// Backwards-compatible repair override: replace recipe repairs with this one.
    pub fn repair(self, r: Repair) -> Self {
        self.replace_adornment(Adornment::Repair(r))
    }
    /// Set the Smeech adornment explicitly (including `Ornament::None` to force it off).
    pub fn ornament(mut self, o: Ornament) -> Self {
        self = self.remove_adornment(AdornmentKind::Smeech);
        match o {
            Ornament::None => self,
            Ornament::Smeech(f) => self.adornment(Adornment::Smeech(f)),
        }
    }
    pub fn variation(mut self, v: Variation) -> Self {
        self.variation = v;
        self
    }
    pub fn extra_class(mut self, c: impl Into<String>) -> Self {
        self.extra_class = Some(c.into());
        self
    }

    pub fn render_today(&self) -> Markup {
        self.render(chrono::Utc::now().date_naive())
    }

    pub fn render(&self, day: NaiveDate) -> Markup {
        let d = self.recipe.defaults();
        let material = self.material.unwrap_or(d.material);
        let edge = self.edge.unwrap_or(d.edge);
        let seed = hash(&format!("{day}:{}", self.id));

        // ── Structural resolution (seed-independent) ──
        let label = self.label.as_ref().map(|l| {
            let want = l.material.unwrap_or(d.label_material);
            let placement = l.placement.unwrap_or(d.label_placement);
            let width = l.width.unwrap_or(d.label_width);
            // Surface: pick a cloth variant; fall back to a material we actually
            // have label art for (keeps the label real rather than a CSS pill).
            let (cloth, url) = label_asset(want, seed);
            ResolvedLabel {
                text: &l.text,
                cloth,
                placement,
                width,
                url,
            }
        });

        let mut resolved_adornments = Vec::new();
        for a in d
            .adornments
            .iter()
            .filter(|a| !self.removed_adornments.contains(&a.kind()))
        {
            resolved_adornments.push((a.clone(), false));
        }
        resolved_adornments.extend(self.adornments.iter().cloned().map(|a| (a, true)));

        // Recipe Smeech entries are defaults and remain suppressed by the plain
        // variation, while explicit builder entries still render when requested.
        let resolved_adornments = resolved_adornments
            .into_iter()
            .enumerate()
            .filter_map(|(i, (a, explicit))| match a {
                Adornment::Repair(r) if r.kind != RepairKind::None => {
                    Some(ResolvedAdornment::Repair(ResolvedRepair {
                        material: r.material.unwrap_or(d.material),
                        pos: r.pos.slug(),
                        url: repair_asset(
                            r.material.unwrap_or(d.material),
                            seed.wrapping_add(i as u64),
                        ),
                        rotation: repair_rotation(seed, i as u64, self.variation),
                    }))
                }
                Adornment::Repair(_) => None,
                Adornment::Smeech(f) if self.variation != Variation::None || explicit => {
                    let poses = f.poses();
                    Some(ResolvedAdornment::Smeech(
                        poses[pick(seed, 0x50 + i as u64, poses.len() as u64) as usize],
                    ))
                }
                Adornment::Smeech(_) => None,
            })
            .collect::<Vec<_>>();

        // ── Surface variation (bounded, seeded) ──
        let var = variation_style(seed, self.variation);

        // Assemble the article class list + the dynamic asset/variation style vars.
        let mut classes = format!(
            "card material--{} card--{} edge--{}",
            material.slug(),
            self.size.slug(),
            edge.slug()
        );
        let label_escapes = label
            .as_ref()
            .is_some_and(|l| !matches!(l.placement, LabelPlacement::Inset));
        if label_escapes {
            classes.push_str(" card--label-escapes");
        }
        if let Some(c) = &self.extra_class {
            classes.push(' ');
            classes.push_str(c);
        }
        if self.jagged {
            classes.push_str(" card--jagged");
        }

        // A peeking Smeech perched over the label (opt-in, brief §34).
        let label_smeech = if self.smeech_label && label.is_some() {
            let peeks = ["watch-peek-ledge", "watch-peek-pocket"];
            Some(peeks[pick(seed, 0x60, peeks.len() as u64) as usize])
        } else {
            None
        };

        let mut style = var.style;
        if edge.has_fray() {
            style.push_str(&format!(
                ";--fray-asset:url('{}')",
                fray_asset(material, seed)
            ));
        }
        if let Some(l) = &label {
            style.push_str(&format!(";--label-asset:url('{}')", l.url));
        }
        if self.jagged {
            let n = pick(seed, 0x70, 3) + 1;
            style.push_str(&format!(";--torn-mask:url('{BORO}/masks/torn-{n:02}.png')"));
        }

        html! {
            article class=(classes) style=(PreEscaped(style)) {
                @if let Some(l) = &label {
                    (render_label(l, label_smeech))
                }
                div.card__surface {
                    (self.content.clone())
                }
                @for a in &resolved_adornments {
                    @match a {
                        ResolvedAdornment::Repair(r) => {
                            span class=(format!("repair repair--{} repair-pos--{}", r.material.slug(), r.pos)) style=(format!("--repair-asset:url('{}');--repair-rot:{}deg", r.url, r.rotation)) {}
                        }
                        ResolvedAdornment::Smeech(pose) => {
                            img class=(format!("card__ornament card__ornament--{}", pose))
                                src=(format!("{BORO}/smeech/smeech-{pose}.webp")) alt="" aria-hidden="true";
                        }
                    }
                }
            }
        }
    }
}

struct ResolvedLabel<'a> {
    text: &'a str,
    cloth: Material,
    placement: LabelPlacement,
    width: LabelWidth,
    url: String,
}

struct ResolvedRepair {
    material: Material,
    pos: &'static str,
    url: String,
    rotation: f32,
}

enum ResolvedAdornment {
    Repair(ResolvedRepair),
    Smeech(&'static str),
}

fn render_label(l: &ResolvedLabel, smeech_peek: Option<&str>) -> Markup {
    // `label--{cloth}` sets the text color to suit the actual cloth shown.
    let class = format!(
        "card-label label--{} label-placement--{} label-width--{}",
        l.cloth.slug(),
        l.placement.slug(),
        l.width.slug(),
    );
    html! {
        h2 class=(class) {
            (l.text)
            @if let Some(pose) = smeech_peek {
                img.label-smeech src=(format!("{BORO}/smeech/smeech-{pose}.webp")) alt="" aria-hidden="true";
            }
        }
    }
}

// ── Asset selection ──────────────────────────────────────────────────────────

/// Number of label cloth variants we actually have art for, per material. Bump
/// as new masters land in `assets/boro-src/labels/`. Linen (the light tag for
/// dark cards) is still 0, so linen-label requests fall back to indigo.
fn label_variant_count(m: Material) -> u64 {
    match m {
        Material::Indigo => 4,
        Material::Linen => 2,
        Material::Olive | Material::FadedBlue | Material::Charcoal | Material::Rust => 1,
    }
}

/// Pick a label cloth: honor the requested material if we have art for it, else
/// fall back to indigo (the signature cloth we always have). Returns the cloth
/// material actually shown (for text color) + the asset URL.
fn label_asset(want: Material, seed: u64) -> (Material, String) {
    let (cloth, count) = if label_variant_count(want) > 0 {
        (want, label_variant_count(want))
    } else {
        (Material::Indigo, label_variant_count(Material::Indigo))
    };
    let n = pick(seed, 0x11, count) + 1;
    (
        cloth,
        format!("{BORO}/labels/label-{}-{:02}.webp", cloth.slug(), n),
    )
}

fn repair_variant_count(m: Material) -> u64 {
    match m {
        Material::Indigo => 6,
        _ => 1,
    }
}

fn repair_asset(m: Material, seed: u64) -> String {
    let n = pick(seed, 0x22, repair_variant_count(m)) + 1;
    format!("{BORO}/repairs/repair-{}-{:02}.webp", m.slug(), n)
}

/// Three distress variants per material; the seed picks one so edges vary per
/// card and day (some more distressed, some tidier).
fn fray_asset(m: Material, seed: u64) -> String {
    let n = pick(seed, 0x33, 3) + 1;
    format!("{BORO}/frays/fray-{}-{:02}.webp", m.slug(), n)
}

// ── Deterministic surface variation (brief §28) ──────────────────────────────

struct VarValues {
    style: String,
}

fn variation_style(seed: u64, level: Variation) -> VarValues {
    if level == Variation::None {
        return VarValues {
            style: "--label-rot:0deg;--label-dx:0px;--label-dy:0px;--repair-rot:0deg".to_string(),
        };
    }
    let (rot_max, dx_max) = match level {
        Variation::Playful => (1.5_f32, 4_i32),
        _ => (0.7_f32, 2_i32),
    };
    let label_rot = signed_bucket(seed, 0xA1, rot_max);
    let repair_rot = signed_bucket(seed, 0xB2, rot_max * 3.0);
    let label_dx = signed_bucket(seed, 0xC3, dx_max as f32).round() as i32;
    let label_dy = signed_bucket(seed, 0xD4, (dx_max / 2).max(1) as f32).round() as i32;
    VarValues {
        style: format!(
            "--label-rot:{label_rot:.2}deg;--label-dx:{label_dx}px;--label-dy:{label_dy}px;--repair-rot:{repair_rot:.2}deg"
        ),
    }
}

fn repair_rotation(seed: u64, index: u64, level: Variation) -> f32 {
    if level == Variation::None {
        return 0.0;
    }
    let max = if level == Variation::Playful {
        4.5
    } else {
        2.1
    };
    signed_bucket(seed, 0xB2 + index.wrapping_mul(0x17), max)
}

/// Tiny non-crypto hash (djb2). Deterministic; turns a seed string into stable
/// pseudo-random surface choices (brief §28).
fn hash(s: &str) -> u64 {
    let mut h: u64 = 5381;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    h
}

/// A stable index in `0..n` from a seed + per-axis salt.
fn pick(seed: u64, salt: u64, n: u64) -> u64 {
    let mixed =
        (seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0x2545_F491_4F6C_DD1D);
    mixed % n.max(1)
}

/// A value in `[-max, +max]`, quantized to buckets so nudges read deliberate.
fn signed_bucket(seed: u64, salt: u64, max: f32) -> f32 {
    const BUCKETS: u64 = 9;
    let b = pick(seed, salt, BUCKETS) as f32;
    let t = b / (BUCKETS as f32 - 1.0);
    (t * 2.0 - 1.0) * max
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 8, 29).unwrap()
    }

    #[test]
    fn recipe_defaults_can_be_removed() {
        let markup = Card::new("information", html! { p { "data" } })
            .recipe(Recipe::Information)
            .without_repairs()
            .render(day())
            .into_string();

        assert!(!markup.contains("class=\"repair"));
    }

    #[test]
    fn cards_can_have_multiple_repairs_and_smeech() {
        let markup = Card::new("many-details", html! { p { "data" } })
            .recipe(Recipe::Quiet)
            .adornments([
                Adornment::Repair(Repair {
                    kind: RepairKind::Edge,
                    material: Some(Material::Indigo),
                    pos: RepairPos::BottomLeft,
                }),
                Adornment::Repair(Repair {
                    kind: RepairKind::Corner,
                    material: Some(Material::Rust),
                    pos: RepairPos::BottomRight,
                }),
                Adornment::Smeech(CatFamily::Watch),
                Adornment::Smeech(CatFamily::Attention),
            ])
            .render(day())
            .into_string();

        assert_eq!(markup.matches("class=\"repair ").count(), 2);
        assert_eq!(markup.matches("class=\"card__ornament ").count(), 2);
    }

    #[test]
    fn legacy_repair_and_ornament_methods_replace_recipe_defaults() {
        let markup = Card::new("override", html! { p { "data" } })
            .recipe(Recipe::Feature)
            .repair(Repair {
                kind: RepairKind::Edge,
                material: Some(Material::Rust),
                pos: RepairPos::TopLeft,
            })
            .ornament(Ornament::Smeech(CatFamily::Play))
            .render(day())
            .into_string();

        assert_eq!(markup.matches("class=\"repair ").count(), 1);
        assert!(markup.contains("repair-pos--top-left"));
        assert_eq!(markup.matches("class=\"card__ornament ").count(), 1);
        assert!(markup.contains("card__ornament--play-"));
    }
}
