/**
 * The omni palette, in the browser.
 *
 * A **port** of `crates/scema-tui/src/theme.rs`. Rust is authoritative: the constants there
 * are the ones the console draws with, the ones `tools/make-icons.py` renders the icon
 * from, and the ones this file copies. `test/theme.test.js` pins the hex values so a drift
 * fails a test rather than surfacing as two surfaces of the same product that do not look
 * like each other.
 *
 * ## Why this exists as a module rather than as CSS in three files
 *
 * The extension has three surfaces — the injected HUD (a closed shadow root, so it cannot
 * use a stylesheet the page can see), the options page, and the popup. Before this file
 * each of them carried its own hex values, and they had already drifted: the HUD was
 * `#4c4cff`, the options page `#8b8bff`, and neither was the console's violet.
 *
 * ## The two rules travel with the palette
 *
 * 1. **Name a role, never a colour.** [`ROLE`] below is the same list `theme.rs` exposes,
 *    minus the terminal-only entries. A renderer asks for `ROLE.unmeasured`, not for a hex.
 * 2. **Colour is decoration, never the message.** Everything the palette distinguishes is
 *    also carried by text — an em dash for unmeasured, `RISK`/`OPP`/`EST?` for signals,
 *    `LIVE`/`STALE`/`ABSENT` for provenance. The HUD must stay legible in a forced-colours
 *    mode, and it does, because nothing depends on the hue alone.
 */

/** Raw palette. Mirrors the `const INK` block in `theme.rs`, name for name. */
const INK = {
  void: '#08060f',
  panel: '#0f0b1a',
  select: '#1e1438',
  rule: '#2a1f45',
  ruleHot: '#6d40c4',

  text: '#e6e0f5',
  muted: '#8a81a8',
  ghost: '#544c6c',

  violet: '#a96bff',
  violetLo: '#7c4dd8',
  violetHi: '#cba6ff',

  azure: '#7dd3fc',
  azureLo: '#4ba3d8',

  amber: '#f2b15c',
  rose: '#ff7b9c',
  mint: '#86e5c0',
};

/**
 * Semantic roles. The only thing a renderer is allowed to name.
 *
 * Terminal-only roles from `theme.rs` (`Chrome`, `ChromeFocus`, `Working`) are folded into
 * the CSS below rather than exposed, because a browser has borders and spinners of its own.
 */
const ROLE = {
  body: INK.text,
  label: INK.muted,
  hint: INK.ghost,

  measured: INK.violet,
  /** Nobody looked. Must read as *quieter*, never merely as a different hue. */
  unmeasured: INK.ghost,

  heading: INK.violetLo,
  headingActive: INK.violetHi,

  /** The one place azure appears in a matrix: the branch that was actually chosen. */
  chosen: INK.azure,
  runner: INK.text,
  excluded: INK.ghost,
  abstained: INK.amber,

  risk: INK.rose,
  opportunity: INK.mint,
  estimated: INK.amber,

  live: INK.mint,
  stale: INK.amber,
  absent: INK.ghost,
  simulated: INK.azureLo,

  valid: INK.azure,
  invalid: INK.rose,
};

/**
 * The stylesheet, as a string.
 *
 * Emitted rather than linked because the HUD lives in a **closed** shadow root and a
 * `<link>` to an extension resource would need `web_accessible_resources`, which is a
 * declaration that any page may fetch the file. The popup and options page use the same
 * string through a `<style>` tag, so all three surfaces are one edit apart.
 *
 * `all: initial` on the host and an explicit reset inside: a page whose own reset says
 * `* { display: none }` would otherwise make the HUD look like a broken extension.
 */
function css() {
  return `
    :host { all: initial; }
    * { box-sizing: border-box; }

    .omni {
      font: 12px/1.55 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      background: ${INK.void};
      color: ${ROLE.body};
      border: 1px solid ${INK.ruleHot};
      border-radius: 10px;
      box-shadow: 0 16px 48px rgba(0,0,0,.6), 0 0 0 1px rgba(169,107,255,.12);
      overflow: hidden;
    }

    .bar {
      display: flex; align-items: center; gap: 8px;
      padding: 8px 10px;
      background: ${INK.panel};
      border-bottom: 1px solid ${INK.rule};
      position: sticky; top: 0; z-index: 2;
    }
    .bar b {
      color: ${ROLE.headingActive};
      letter-spacing: .14em; font-weight: 700; font-size: 11px;
    }
    .sp { flex: 1 }

    button {
      font: inherit;
      background: ${INK.panel}; color: ${ROLE.body};
      border: 1px solid ${INK.rule}; border-radius: 5px;
      padding: 4px 10px; cursor: pointer;
    }
    button:hover:not([disabled]) { border-color: ${INK.ruleHot}; color: ${ROLE.headingActive}; }
    button:focus-visible { outline: 2px solid ${INK.azure}; outline-offset: 1px; }
    button[disabled] { opacity: .45; cursor: default; }
    button.primary { border-color: ${INK.violetLo}; color: ${ROLE.headingActive}; }

    input, textarea {
      font: inherit; width: 100%;
      background: ${INK.void}; color: ${ROLE.body};
      border: 1px solid ${INK.rule}; border-radius: 5px; padding: 5px 8px;
    }
    input:focus, textarea:focus { outline: none; border-color: ${INK.ruleHot}; }

    section { padding: 9px 10px; border-bottom: 1px solid ${INK.rule}; }
    section:last-child { border-bottom: 0; }
    h4 {
      margin: 0 0 6px; font-size: 10px; font-weight: 600;
      letter-spacing: .14em; color: ${ROLE.label};
    }

    .row { display: flex; gap: 8px; align-items: center; }
    .k { color: ${ROLE.label}; display: inline-block; min-width: 74px; }
    .note { font-size: 10px; color: ${ROLE.hint}; margin-top: 5px; line-height: 1.5; }

    table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
    th, td { text-align: right; padding: 2px 3px; white-space: nowrap; }
    th { color: ${ROLE.label}; font-weight: 500; font-size: 10px; letter-spacing: .06em; }
    td.b, th.b { text-align: left; white-space: normal; }

    /* Roles. Every one of these is also carried by the text in the cell. */
    .measured    { color: ${ROLE.measured}; }
    .unmeasured  { color: ${ROLE.unmeasured}; opacity: .8; }
    .chosen      { color: ${ROLE.chosen}; font-weight: 700; }
    .excluded    { color: ${ROLE.excluded}; }
    .abstained   { color: ${ROLE.abstained}; }
    .risk        { color: ${ROLE.risk}; }
    .opportunity { color: ${ROLE.opportunity}; }
    .estimated   { color: ${ROLE.estimated}; }
    .live        { color: ${ROLE.live}; }
    .stale       { color: ${ROLE.stale}; }
    .absent      { color: ${ROLE.absent}; }
    .valid       { color: ${ROLE.valid}; font-weight: 700; }
    .invalid     { color: ${ROLE.invalid}; font-weight: 700; }
    .dim         { color: ${ROLE.hint}; }
    .label       { color: ${ROLE.label}; }

    ul { margin: 4px 0 0; padding-left: 16px; }
    li { margin: 1px 0; }
    a { color: ${ROLE.chosen}; }
    code, pre {
      background: ${INK.void}; border: 1px solid ${INK.rule};
      border-radius: 4px; color: ${ROLE.body};
    }
    pre { padding: 8px; overflow-x: auto; }
    code { padding: 0 3px; }

    label.pick {
      display: block; padding: 2px 0; cursor: pointer; border-radius: 4px;
    }
    label.pick:hover { background: ${INK.select}; }

    /* A forced-colours mode strips every colour above. Nothing is lost: the text carries
       the message, which is the whole reason rule 2 exists. */
    @media (forced-colors: active) {
      .omni { border-color: CanvasText; }
      .measured, .unmeasured, .chosen, .risk, .opportunity { color: CanvasText; }
    }
  `;
}

const ScemaTheme = { INK, ROLE, css };

if (typeof globalThis !== 'undefined') globalThis.ScemaTheme = ScemaTheme;
if (typeof module !== 'undefined' && module.exports) module.exports = ScemaTheme;
