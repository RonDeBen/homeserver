# Frontend plan: web now, server-driven iOS later

Planning doc for the homeserver's UI layer. Written as a handoff — self-contained
enough for a fresh agent (or future-you) to pick up. The near-term work is the
**web dashboard**; the longer-term goal is a **server-driven iOS app**. The whole
point of this doc is the seam *between* them: build the website so its visual
language and layout vocabulary can be shared with iOS, not thrown away.

## Where we are today (already built)

`crates/gateway` is the HTTP backend-for-frontend — a bus participant like every
job, that also serves HTTP. It reads Postgres for queries and subscribes to
`*.updated` bus events to push live updates. Current surfaces:

- `GET /` — the **Overview hub**: a bento grid of cards fed by real reads
  (calendar, health fasting/weight, orchestrator schedules); `hub.rs`
- `GET /api/calendar` — JSON list (the eventual iOS/SDUI data source)
- `GET /calendar` — server-rendered HTML page (maud templates in `views.rs` +
  `calendar.rs`)
- `GET /calendar/events` — Datastar SSE; re-queries Postgres and pushes a
  `PatchElements` fragment on every `calendar.updated`
- `GET /static/{file}` — vendored assets (Datastar, boro SVGs), unauthenticated;
  served from `assets.rs` via `include_*!` so deploy stays a single binary
- `GET /healthz` unauthenticated

Supporting pieces:
- **Auth**: `common::auth` (`AuthProvider` trait + `Identity`). `TrustedNetwork`
  default (deploy behind Tailscale); `BearerToken` for public exposure. Selected
  by `AUTH_MODE` env. Handlers never know the scheme.
- **Styling**: `views::layout(title, body)` wraps every page; base CSS lives in
  `views.rs` as a deliberately-plain placeholder (`BASE_CSS`). Datastar loads
  from CDN (`@v1.0.2`) — vendor it under a `/static` route before relying on it
  offline.
- **XSS posture**: maud auto-escapes all interpolated (scraped, untrusted) text;
  `views::safe_url` http(s)-allowlists any URL used as an `href`.

Stack: `axum` 0.8, `datastar` 0.3, `maud`. No build step, no JS framework, no WASM.

## Design language: Smeech (boro / wabi-sabi)

The look is **patched fabric**: an asymmetric bento grid of cloth cards, each with
a sashiko running-stitch border and the odd corner patch, in a muted indigo/earth
palette, watched over by a black cat named **Smeech**. It lives entirely in
`views.rs` (`STYLES` + partials) + hand-authored SVGs in `crates/gateway/assets/`.
Three layers, each a portable artifact:

- **Tokens** (`:root` in `views.rs`) — the names the future SwiftUI theme mirrors:
  - *Surfaces* come as variant sets, each pairing a fabric color with text/muted/
    stitch: `linen` (cream), `denim` (indigo), `olive`, `rust`, `charcoal`. In CSS
    they're the `.card--{variant}` classes; in SDUI they become a `variant` field
    on the `card` component (a token, **not** a new component type).
  - *Accents / threads*: `--accent` (amber, the active-tab + link color),
    `--thread-sage/-rust/-blue` (the restrained chart-line palette).
  - *Type ramp*: `--font-display` (serif headlines), `--font-mono` (data/stats/logs,
    tabular-nums), `--font-body` (humanist sans). System stacks for now — no
    vendored webfonts yet.
  - *Space / radius / stitch* scales, so the sashiko border is one styleable thing.
- **Components** — one class per vocabulary entry: `.card` (+ variants, `.card__title`,
  `.card__lead`), `.stat`, `.list`/`.row` (+ `.badge`), `.panel`/`.chart`, `.log`,
  `.quote`, and the `.bento` grid (`card--wide/-tall/-big` spans, `grid-auto-flow: dense`).
- **Decoration** — textures (`noise.svg` card grain), corner patches
  (`card--patch-sashiko/-calico`, pure-CSS `::before`), and cats (`cat-peek/-sleeping/
  -walk.svg`). All gated by `body.plain`, so calm/low-distraction mode is one switch.

**The rule that keeps charts & calendars precise:** *textured chrome wraps a calm
content surface.* Fabric is only the frame + patch; put any chart or dense grid
inside a `.panel` (quiet dark ground, crisp gridlines, tabular type) and the boro
never touches the data. Charts stay inline SVG using the `--thread-*` tokens.

**iOS seam:** tokens port 1:1 to a SwiftUI theme; the SVG patches/cats become a
shared Asset Catalog of the *same* art; card `variant`/`patch`/`decor` ride the SDUI
JSON as tokens while each client owns the pixels — so the fabric look is shared
without shipping markup or bytes through the API. Decoration is skin, never content.

**Assets** (`crates/gateway/assets/`, embedded via `include_*!`): `datastar.js`
(vendored, offline), `noise.svg`, `patch-sashiko.svg`, `patch-calico.svg`,
`cat-peek.svg`, `cat-sleeping.svg`, `cat-walk.svg`. All SVG/text — theme and scale
for free; cats carry a CSS rim-light so the near-black silhouettes read on dark nav.

### Multi-axis card system + Card Lab (2026-08-28)

The single-axis `.card--{variant}` system was promoted to a **composable multi-axis**
one per the Card & Visual Language brief. A card is now a composition of independent
axes — **material · size · edge · label · repair · ornament · variation** — bundled
by named **recipes**. It lives in `crates/gateway/src/card.rs` (a small flat builder,
no big type hierarchy yet — brief §68) rendering to Maud; the CSS vocabulary is in
`views.rs` (`STYLES`).

```rust
Card::new("fasting", content).recipe(Recipe::Feature).size(CardSize::Wide).label("Health")
```

- **Six materials**: linen · charcoal · faded-blue · indigo · olive · rust (`--denim`
  kept as a back-compat alias of faded-blue). Class token `material--{name}`.
- **Recipes** (`Quiet`/`Information`/`Healthy`/`Active`/`Feature`) supply coherent
  defaults; any axis is an explicit override. Unset → recipe default → global default.
- **Labels are real HTML text** in an `<h2 class="card-label …">` (never baked into an
  image); placement `inset/edge/overlap/floating` × width `content/short/medium/long/sash`.
- **Decoration is CSS/SVG placeholder** (real transparent label/fray/repair raster
  assets are Phase 2). `.card` stays `overflow: visible`; the inner `.card__surface`
  is the clip container so charts/grids in a `.panel` never get clipped.
- **Variation is deterministic per `(day, card-id)`** (djb2 seed, no deps) → decoration
  is stable across SSE re-renders and interaction, and only drifts day-to-day. This is
  why the calendar card (seeded `(today, "calendar")`) never jitters on a live patch.
- **`body.plain`** silences textures/frays/repairs/ornaments but keeps label *text*.
- **Card Lab** (`GET /lab`, dev-only, `card_lab.rs`): recipe gallery + the 12 canonical
  cases + axis permutation matrices + a no-JS query-param single-card previewer. This is
  the sandbox for discovering/approving recipes (brief §45–§47). The Overview and
  Calendar pages were migrated onto the builder as the first real-world proof.

**Real assets integrated (2026-08-28).** The placeholders are gone. `scripts/boro-assets.sh`
processes the handmade masters in `assets/boro-src/` (gitignored) into 55 transparent WebP
(~2.6 MB) under `crates/gateway/assets/boro/`, embedded via `include_dir` and served at
`/static/boro/**` (`assets.rs`). Wired in: real woven-cloth **materials** (all 6), transparent
**cloth labels** (indigo; other materials hot-swap when generated) with live HTML text over them,
real **repair patches** in recipe-declared **slots** (deliberate position+material, never random),
real **fray** overlays on frayed edges, and a full **Smeech** pose-family system (rest/watch/
active/play/attention + paired edge-interactions) chosen by variation. The variation model is now
split **structural** (recipe-controlled: material/size/label-placement/repair-slot/Smeech-family)
vs **surface** (seed-varied: which variant asset + tiny rotation/offset), so SSE re-renders never
jitter decoration. Deferred: other-material label art (user-generated), asset/recipe registry,
Smeech animation.

## The end goal: server-driven iOS (SDUI)

The motivating problem: **new jobs keep landing** (calendar, health/fasting,
food-photo, garden…) and we don't want to ship a new iOS binary for each one.
The answer is Server-Driven UI, but a *small, bounded* version — not a
build-your-own-React engine.

### The model

- The gateway serves a **screen** as JSON: an ordered list of **components** drawn
  from a **fixed vocabulary**.
- The native SwiftUI app is a **renderer** for that vocabulary plus native
  affordances (camera, HealthKit, notifications, offline).
- **The boundary — memorize this, it's the whole design:**
  - **New job, reusing existing components** → the server emits a new screen from
    the existing vocabulary → **no app update.** (~90% of cases.)
  - **New component *type*** → **app update.** (Rare.)

### Component vocabulary (the shared contract)

This is the same list the *website* is built from (see below). Start small; grow
only on the rule of three.

| Component      | Purpose                                  | Web render (HTML/CSS)        | iOS render (SwiftUI)      |
|----------------|------------------------------------------|------------------------------|---------------------------|
| `screen`       | ordered container of sections            | `<main>` + stack             | `ScrollView`/`List`       |
| `section`      | titled group                             | `<section>` + heading        | `Section`                 |
| `list` / `row` | vertical list of rows (calendar events)  | `<ul class="events">`        | `List`/`ForEach`          |
| `card`         | bordered block of content                | `.card`                      | rounded `VStack`          |
| `stat`         | one number + label (fasting hours, wt.)  | `.stat`                      | metric tile               |
| `chart`        | small time-series/spark                  | inline SVG                   | Swift Charts              |
| `form` / field | inputs + submit → POST/action            | `<form>` + Datastar action   | `Form`                    |
| `image-capture`| "photograph this" (food-photo job)       | `<input type=file capture>`  | native camera             |
| `action`       | button → bus/HTTP call                   | Datastar `@post`/`@get`      | button → API call         |

### Shared Rust core (optional, later)

Not the UI — a **core library** (screen/component model types, API client,
decoding) compiled via **UniFFI** to a Swift package, so the iOS type contract
tracks the Rust backend instead of being hand-maintained. Add when hand-decoding
JSON in Swift starts to hurt; don't build it up front.

## Near-term: the website, built mobile-aware

Decision: **nail the website first.** It's the cheap, fast surface to validate the
component vocabulary and visual language on, and it de-risks iOS — the `screen`
concept and the design tokens get proven before anything ships in a binary.

**Mobile-aware, not mobile-first.** Desktop is the primary dev target (side-by-side
while we work), but every layout must be usable and good-looking on a phone
browser, because:
1. It's genuinely used from a phone on the couch / out of the house.
2. The responsive layout *is the rehearsal* for the iOS layout — the same
   sections, the same card/stat/list vocabulary, the same spacing rhythm.

### How "mobile-aware" is achieved concretely

- **Fluid, single-column-friendly layout.** Content lives in a centered column
  (`.container`, already ~42rem max). On a phone it's a clean single column; on
  desktop it stays a comfortable reading width rather than sprawling. Multi-column
  areas use `display: grid` with `repeat(auto-fit, minmax(…, 1fr))` so they
  collapse to one column on narrow screens **without** media-query bespoke work.
- **Design tokens, not magic numbers.** A small set of CSS custom properties —
  spacing scale, radius, color roles (`--muted`, `--accent`, surface/border),
  font — defined once in `:root`. These tokens are the artifact that ports to
  iOS: the SwiftUI theme mirrors the same names/values. (Start from the current
  `BASE_CSS` tokens and formalize them.)
- **`color-scheme: light dark`** + role tokens so light/dark both work for free
  (already on). Prefer `color-mix()`/semantic roles over hardcoded hex.
- **Fluid type & touch targets.** Body type via `clamp()` so it scales sensibly;
  interactive targets ≥ 44px tall (Apple's touch minimum) so the same markup is
  finger-friendly and the iOS port inherits the sizing intuition.
- **Component-first CSS.** Style by component class (`.card`, `.stat`, `.event`,
  `.section`) — one class per vocabulary entry above — **not** by page. This is
  what makes the styles shareable: each web component is the visual spec for its
  SDUI counterpart. A page is just a composition of these.
- **Semantic HTML.** `<section>`, `<ul>`, `<form>`, headings — maps cleanly to
  SwiftUI's `Section`/`List`/`Form`, and keeps things accessible.

### Design workflow (Adobe vibecode tool)

Prototype the *look* in Adobe's vibecode tool (React). Port only the **HTML
structure + CSS** into maud templates; discard the React/JS. Extract the tool's
output into our **token set** and **per-component CSS** — don't paste page-shaped
markup wholesale. Nothing sensitive goes into the tool; layout mockups are fine.

## Build order

1. **Formalize the design system in CSS** — promote `BASE_CSS` into an explicit
   token set (spacing/color/radius/type) + a component stylesheet (`.card`,
   `.stat`, `.section`, `.list/.row`, `.form`, `.action`). Vendor Datastar to
   `/static`. This is the shared foundation.
2. **Rebuild the calendar page from the component vocabulary** — prove that a page
   is "a screen = sections of components," rendered from reusable maud partials
   (one partial per component). This is the web mirror of an SDUI screen.
3. **Add the health surface** to the gateway the same way (list of fasting/weight
   stats + a form) — second job through the same vocabulary is the real test that
   the component set generalizes (rule of three watch).
4. **Extract a `screen`/component model** once ≥2 jobs share it, and have the
   gateway emit it as **JSON** (`/api/*/screen`) — the SDUI contract — rendered
   to HTML on web from the *same* model. Now web and iOS consume one description.
5. **iOS**: SwiftUI renderer over that JSON; native camera/HealthKit; then UniFFI
   core if warranted.

Web validates each step before iOS commits it to a binary.

## Principles / guardrails

- **Don't pre-abstract.** Copy the calendar surface for the next job; extract the
  shared `screen`/component model on the rule of three, not before. The vocabulary
  table above is a *target to grow into*, not a framework to build up front.
- **One gateway, many frontends.** All surfaces read the same Postgres + bus; no
  per-frontend backend.
- **Server owns content and (eventually) layout; clients render.** New capability =
  new job + new screen description, ideally no client change.
- **Tokens are the portable artifact.** If a value appears in both web CSS and the
  future SwiftUI theme, it's a token with one name in both places.

## Open questions

- Exact token scale (spacing steps, type ramp, color roles) — settle in step 1.
- Where forms/actions POST: dedicated `/action` endpoints vs. per-job routes.
- Whether the web `screen` model is generated from the same Rust types the SDUI
  JSON uses (shared source of truth) or kept parallel until iOS starts.
- Auth for the browser specifically once public: Tailscale covers the common case;
  a real session/cookie provider is a later `AuthProvider` impl if needed.

## Pointers

- Code: `crates/gateway/{lib,calendar,views,auth}.rs`, `crates/common/src/auth.rs`
- Related memory: `gateway-frontend-plan`, `health-hub-plan`, `dont-pre-abstract`
- Datastar: https://data-star.dev (v1.0.2; SSE events `datastar-patch-elements` /
  `datastar-patch-signals`)
