# Gateway CSS guide

This project uses server-rendered Maud templates and Datastar. Keep the CSS
simple and local to the thing that owns the markup.

## Ownership

- **Shell/layout** owns the navigation, page container, bento grid, and card
  placement.
- **Card chrome** owns the card surface, material, label, decoration, padding,
  and size modifiers.
- **A feature** owns everything inside `.card__surface`: its internal grid,
  rows, controls, empty states, and responsive behavior.

The important boundary is:

```text
.bento -> .card -> .card__surface -> .feature-root -> feature content
```

Card chrome should not decide how calendar, health, or another feature arranges
its content. A feature must remain readable when the card is narrow, even if
the page itself is wide.

## Naming

Give each feature one root class and namespace its descendants beneath it:

```html
<div class="calendar-month">...</div>
<div class="calendar-events">
  <article class="calendar-event">...</article>
</div>
```

Use semantic feature names rather than generic names such as `.title`,
`.content`, or `.main`. Shared primitives are allowed when their behavior is
truly shared (`.stat`, `.list`, `.badge`).

Avoid broad descendant rules such as `.card p` and `.card a`; they make a new
feature inherit behavior accidentally. Style the feature or shared primitive
that owns the element instead.

## Sizing rules

- Put `min-width: 0` on grid and flex children that contain text or another
  layout.
- Avoid fixed minimum track sizes inside cards unless the card contract
  guarantees that width.
- Use `minmax(0, 1fr)` for flexible feature columns.
- Use a container query when a feature changes layout based on the width of its
  card. Viewport media queries are not enough for bento grids because a card can
  be narrow on a wide screen.
- Keep clipping on `.panel` or another feature-owned surface, not on `.card`,
  because card decoration may intentionally overhang.

## Adding a feature

1. Add the feature root and templates next to the feature code.
2. Add feature CSS beside those templates.
3. Namespace selectors under the feature root.
4. Test the feature in a small card, its normal card size, and a mobile-width
   card.
5. Only move a rule into shared CSS after a second feature genuinely needs the
   same behavior.

The design tokens in the shared stylesheet are the portable visual contract.
Feature CSS should consume those tokens instead of introducing unrelated magic
colors, spacing, or typography.

## Reusable feature views

A feature root may be rendered in more than one shell context—for example, as a
full page and as a card in the hub. Keep those presentation modes explicit with
modifier classes or separate render functions. Page-level layout rules such as
`display: grid`, column tracks, and full-row sizing must not be applied to the
shared root when that root can also be a bento child; otherwise the hub card can
turn into a compressed nested layout.

For layered feature decoration, make the stacking order explicit: the visual
scrap belongs below the readable content (`::before` at the lower layer and the
number/content above it). If a hover transform is added to the scrap, preserve
any centering translate in the transformed state.

## Interactive fragments

Datastar responses are HTML, so an interaction can change layout as well as
content. When a reusable view has multiple presentation modes, carry that mode
through every request that can replace or patch it: initial render, click
actions, SSE/live updates, and empty/error states. A response that renders the
right data with the wrong root modifier class can silently move a feature from
page layout into hub-card layout (or vice versa).

Keep the mode explicit in the render function or request context, and add a
rendering test that checks the returned root class plus the mode-bearing action
and live-update URLs. This catches context loss without needing a browser test.
