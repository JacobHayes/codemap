# codemap: Design

How codemap looks and reads, on screen and in the terminal. codemap looks like a measurement
workspace, closer to an instrument panel or a drafting sheet than to a dashboard: opaque paper
panels pinned by corner ticks on a plain ground, crisp hairlines, mono labels and tabular
figures, with colour kept for measures and state. Token values live in `design/tokens.css`, base
styles and components in `design/base.css`, fonts in `design/fonts/`. On a rule, this document
wins over them. Screens are judged by their screenshots against these rules.

## Principles

- **The data is the interface.** The map, the matrix and the queue fill the screen; chrome is
  quiet and thin around them.
- **Structure over colour.** Hierarchy comes from type, hairlines and frames. Colour encodes a
  measure or marks state; it never decorates.
- **Honest encodings.** Area is proportional to size. A colour scale is ordered, monotonic in
  lightness, and labelled with its measure and range. No encoding exaggerates a difference the
  numbers do not have.
- **Accent means state.** The one accent marks what is selected, focused or flagged. Never a
  category, a link or a measure.
- **Calm by default, dense on request.** A view opens on its stock settings and shows the
  overview; detail is a hover, click or drill away. Leave a count where something is hidden.
- **Plain words over codes.** "312 lines of handling in 9 functions", never a bare number or an
  abbreviation. A missing value is said in words. A view never claims what codemap does not
  know; a skipped signal says it was skipped and why.
- **The settled state comes first.** Every view reads correctly with no motion; motion only
  adds.

Avoid: rounded cards, gradients, rainbow scales, 3D effects outside the city, decorative motion,
status by colour alone.

## Tokens

Screen code uses tokens only: no raw colours, no own sizes or durations. A theme-dependent token
is one `light-dark(light, dark)` declaration; light and dark are both native and follow the OS.

- **Neutrals** are steps of one cool ramp; every role picks a step.
- **Measure scale.** One sequential scale serves magnitudes and ratios alike, running from the
  neutral ramp to a warm amber, so a higher value is always warmer and the low end stays quiet.
  A ratio of zero (no tests, no handling) or a file with no line in any open finding is not a
  finding: like a value with no colour it is the plain neutral fill, never a scale step. It never uses the accent's blue, so a selection always
  stands out on a map. It is defined in OKLCH with even lightness steps and reads in both themes.
  The scale is generated from one definition shared by the canvases and their legends.
- **Categorical colours** are few and used only for role or concern, where the category is the
  point of the view. Everything else is neutral.
- **Contrast floors** (text 4.5:1 on its ground, labels on every scale step they can sit on) are
  checked by tests in both themes.
- **Shape.** Panels are square; controls slightly rounded; only status dots are round. Borders
  are 1px; 2px marks selection.
- **Motion** is one duration and one easing, off under reduced motion.

## Typography

| Role | Face | Notes |
| --- | --- | --- |
| Titles | Space Grotesk 700 | page and section titles uppercase |
| Labels, column headers, keys, chips | IBM Plex Mono 12px / 500 | uppercase, steel |
| Body, controls, rows | IBM Plex Sans 15px | sentence case |
| Prose | IBM Plex Sans 17px | |
| Numbers, paths, symbols, code | IBM Plex Mono | tabular figures |

12px is the floor, on canvases too: a label that would be smaller is not drawn. Fonts are
self-hosted.

## Layout

- **Shell.** A header with the breadcrumb (the drill path) and the filter bar; the view edge to
  edge below it; an inspector on the right for the selection.
- **One scroller per region.** The document never scrolls. A canvas never sits inside a
  scroller.
- **Desktop first.** Designed for 1024px and wider.

## Components

- **Panel.** Opaque paper, 1px border, four corner ticks, square. A panel's first label is its
  header.
- **Table.** The one list component: hairline rows, sticky header, tabular numbers right
  aligned, a 2px accent line on the selected row.
- **Filter chip.** One chip form for every active filter, with its dimension and value; removing
  it is one click.
- **Breadcrumb.** Each segment is a level of the drill path; clicking one returns there. The
  last segment offers "group this by...", which pivots in place.
- **Measure picker.** Chooses the size and colour measures; shows the legend for the colour
  scale with its range in words and numbers.
- **Area card.** In the queue: the subject's path (a group's title, which says the pattern and
  how many places and files), one sentence on why it stands out, a line "Also flagged by
  <detector>: <title>" linking to each area that agrees with it, the detectors that agree as
  chips, and its mass. Evidence and measures fold out; a file's or folder's card also lists the
  groups it belongs to with its share of each, and a group's lists its members. Acknowledged
  areas show their reason and sit below the rest.
- **Tooltip.** One component, never the browser's `title`. It says only what the view does not:
  the full path, the exact values, what a click does.
- **Badges** pair a word with a dot shape, so state never relies on colour.

## Views

- **Treemap.** Flat and quiet: squarified tiles with flat fills, separated by small gutters, no
  shading, borders or gradients. Nesting reads from spacing and a light group tone, and only a
  few levels below the current root are drawn; deeper levels aggregate into their ancestor until
  you drill in. Area is the size measure; fill is the colour measure on its scale. A tile big
  enough shows its name and value at the 12px floor; smaller tiles show nothing. Hover outlines
  the tile and names its path; when the colour is the share of lines in open findings (the tooltip says "26 of 514 lines"), the tooltip and
  the selection list the areas touching the tile (groups included) with the lines each has
  there, and the selection's entries link to the areas; click selects; double-click drills into a folder or opens a file's source.
- **Cross-tab heatmap.** Rows and columns are dimension values ordered by their totals; cells
  use the measure scale with the value printed where it fits. Margins show row and column
  totals. Clicking a cell opens the map filtered by both of its values (the grid would shrink
  to that one cell); a path value drills there. The grid draws the 60 largest rows and 24
  largest columns and says how many it left out.
- **Area queue.** A ranked table of area cards; the count of areas hidden by filters or below
  the fold is always shown. The header's filter chips (role, concern, language, kind) narrow it
  like the map; an empty result offers to clear them.
- **Source viewer.** Lines with a gutter of numbers and, where they change, role and concern;
  finding ranges are row highlights with links back to the area, and a long file pages by the
  answer's `more`. Syntax highlighting waits for the server to send token spans.
- **Code city** (later). The treemap's layout as a ground plan; height and colour are measures.
  It is a projection of the same data, never a separate system.

Out of focus is opacity 0.3; selection is the accent; nothing is dimmed and coloured at once.
When filters leave nothing to draw, the view says so and offers to clear them.

## Terminal output

- **Human text** leads with one line of totals, then aligned tables with headers, numbers right
  aligned, and paths relative to the root. No colour needed to read it.
- **`--json`** returns the `Answer` envelope unchanged; nothing in it is formatted for a
  terminal.
- **Overviews are aggregates.** A list in an overview is either complete or says how many more
  there are and how to get them.

## Accessibility

- Text meets WCAG AA (4.5:1) in both themes.
- Every view is usable from the keyboard: arrows move the selection, Enter drills, Backspace
  goes up.
- Content shown on hover or focus can be dismissed with Esc.
