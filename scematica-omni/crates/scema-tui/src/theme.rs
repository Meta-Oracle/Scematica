//! The palette: black and violet, with soft blue where a decision is being claimed.
//!
//! Every other terminal surface in this repository has its own identity — the sniper
//! dashboard is black and red, `mesh-dashboard` is indigo over slate, `sdk-dashboard` is
//! the bond pipeline's own green. Omni is **black and violet with soft-blue accents**, and
//! that is not decoration for its own sake: an operator with three of these open at once
//! must be able to tell at a glance which one is making a claim about their money and which
//! one is making a claim about a decision record.
//!
//! ## Two rules, both learned elsewhere in this repo and both load-bearing
//!
//! 1. **A renderer names a [`Role`], never a colour.** Same rule as `alchem_link.theme`
//!    (a test fails the build on a hardcoded hex in a render module) and
//!    `lib/mesh/view.ts::toneFor` (the only thing in the web app allowed to pick a tone).
//!    A rule that encodes a claim about trust gets exactly one implementation.
//!
//! 2. **Colour is decoration, never the message.** `NO_COLOR`, a pipe, `--no-color` and a
//!    16-colour terminal must all produce the same *text*. Every state this palette can
//!    express is also carried by a glyph or a word — `—` for unmeasured, `▸` for chosen,
//!    `LIVE`/`STALE`/`ABSENT` for provenance, `EXCLUDED` for a forbidden branch. Strip the
//!    colour and nothing has been lost but pleasure.
//!
//! The second rule is what makes the first one cheap. Because the message survives without
//! colour, [`Depth`] can degrade truecolor -> 256 -> 16 -> none without any renderer
//! knowing.
//!
//! ## Why violet, specifically
//!
//! Omni's whole subject is the boundary between *measured* and *unmeasured*. That is a
//! two-tone problem, and it wants a palette where the interesting distinction is
//! luminance-first rather than hue-first: a measured value glows, an unmeasured one recedes
//! to a note in the same family. A red/green palette would put the emphasis on
//! good-vs-bad, which is the wrong axis — a measured `0.00` risk and an unmeasured risk are
//! not good and bad, they are known and unknown. Soft blue is reserved for exactly one
//! thing, the branch that was actually chosen, so the eye lands on the claim.

use ratatui::style::{Color, Modifier, Style};

/// A 24-bit colour in the palette, plus its fallbacks.
///
/// The 256 and 16 approximations are chosen by hand rather than computed. A nearest-cube
/// search collapses the four violets into two indices on a 256-colour terminal, and then
/// "chosen" and "measured" render identically — which is precisely the distinction the
/// screen exists to make.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ink {
    pub rgb: (u8, u8, u8),
    pub xterm: u8,
    pub ansi: Color,
}

const fn ink(r: u8, g: u8, b: u8, xterm: u8, ansi: Color) -> Ink {
    Ink { rgb: (r, g, b), xterm, ansi }
}

// ── the raw palette ───────────────────────────────────────────────────────────
//
// Named by appearance here and *only* here. Nothing outside this module may refer to these
// constants; renderers ask for a `Role`.

/// Near-black with a violet cast. Not `#000`: a true black next to a violet border reads as
/// a hole punched in the screen, and terminals with translucent backgrounds show through.
const VOID: Ink = ink(0x08, 0x06, 0x0F, 232, Color::Black);
/// Panel ground, one step up from the void.
const PANEL: Ink = ink(0x0F, 0x0B, 0x1A, 233, Color::Black);
/// Selected row.
const SELECT: Ink = ink(0x1E, 0x14, 0x38, 236, Color::Black);
/// Quiet rules and inactive borders.
const RULE: Ink = ink(0x2A, 0x1F, 0x45, 238, Color::DarkGray);
/// Active border, and the frame around whatever has focus.
const RULE_HOT: Ink = ink(0x6D, 0x40, 0xC4, 98, Color::Magenta);

/// Body text: lavender-white, not pure white. Pure white against violet vibrates.
const TEXT: Ink = ink(0xE6, 0xE0, 0xF5, 189, Color::White);
/// Labels, column heads, anything structural rather than informational.
const MUTED: Ink = ink(0x8A, 0x81, 0xA8, 103, Color::Gray);
/// The recessive tone. An unmeasured term wears this, and it must read as *quieter* than
/// the surrounding text at every depth — never merely a different hue.
const GHOST: Ink = ink(0x54, 0x4C, 0x6C, 60, Color::DarkGray);

/// The signature violet.
const VIOLET: Ink = ink(0xA9, 0x6B, 0xFF, 141, Color::Magenta);
/// Violet, dimmer — a measured value that is not the headline.
const VIOLET_LO: Ink = ink(0x7C, 0x4D, 0xD8, 98, Color::Magenta);
/// Violet, brighter — headings and the runtime banner.
const VIOLET_HI: Ink = ink(0xCB, 0xA6, 0xFF, 183, Color::LightMagenta);

/// The soft blue accent. Reserved for the chosen branch and for the commitment when it
/// verifies. Used sparingly on purpose: an accent that appears on every third row is not an
/// accent, it is a second body colour.
const AZURE: Ink = ink(0x7D, 0xD3, 0xFC, 117, Color::LightCyan);
/// Azure, dimmer.
const AZURE_LO: Ink = ink(0x4B, 0xA3, 0xD8, 74, Color::Cyan);

/// Abstention and staleness. Warm, so it separates from the whole violet/blue family
/// without becoming an alarm.
const AMBER: Ink = ink(0xF2, 0xB1, 0x5C, 179, Color::Yellow);
/// Risk signals and invalid commitments. The only genuinely alarming colour here.
const ROSE: Ink = ink(0xFF, 0x7B, 0x9C, 204, Color::LightRed);
/// Opportunity signals. Cool mint, deliberately not the same as `AZURE` — an opportunity is
/// an observation, a choice is a claim, and they must not share a colour.
const MINT: Ink = ink(0x86, 0xE5, 0xC0, 115, Color::Green);

/// What a renderer is allowed to ask for.
///
/// Add a role rather than reaching for an `Ink`. The list being short and boring is the
/// feature: every entry here is a distinction somebody has to be able to make on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Screen background.
    Ground,
    /// Panel background, one step up from `Ground`.
    Panel,
    /// The highlighted row in a list.
    Selection,
    /// Inactive borders and horizontal rules.
    Chrome,
    /// The border of whatever currently has focus.
    ChromeFocus,
    /// Ordinary body text.
    Body,
    /// Column headings, field labels, anything structural.
    Label,
    /// Key hints in the footer.
    Hint,

    /// A number that somebody actually measured.
    Measured,
    /// The em dash. Nobody looked. Must read as quieter than `Body` at every depth.
    Unmeasured,
    /// A heading, a tab name, the runtime banner.
    Heading,
    /// The active tab.
    HeadingActive,

    /// The branch that was chosen. The one place azure appears in the matrix.
    Chosen,
    /// A branch that competed and lost.
    Runner,
    /// A branch removed before ranking by a constraint.
    Excluded,
    /// The agent declined to choose.
    Abstained,

    /// A counted risk signal.
    Risk,
    /// A counted opportunity signal.
    Opportunity,
    /// A signal the observer estimated rather than counted.
    Estimated,

    /// `Provenance::Live`.
    Live,
    /// `Provenance::Stale` — visible, not actionable.
    Stale,
    /// `Provenance::Absent` — nothing is known here.
    Absent,
    /// `Provenance::Simulated`.
    Simulated,

    /// A commitment that recomputes to what it claims.
    Valid,
    /// A commitment that does not. The strongest thing on the screen.
    Invalid,
    /// Something is in flight.
    Working,
}

impl Role {
    /// Every role, for tests and for the palette preview.
    pub const ALL: [Role; 26] = [
        Role::Ground, Role::Panel, Role::Selection, Role::Chrome, Role::ChromeFocus,
        Role::Body, Role::Label, Role::Hint, Role::Measured, Role::Unmeasured,
        Role::Heading, Role::HeadingActive, Role::Chosen, Role::Runner, Role::Excluded,
        Role::Abstained, Role::Risk, Role::Opportunity, Role::Estimated, Role::Live,
        Role::Stale, Role::Absent, Role::Simulated, Role::Valid, Role::Invalid,
        Role::Working,
    ];
}

/// How much colour the terminal can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Depth {
    /// 24-bit.
    True,
    /// 256-colour cube.
    Xterm,
    /// The 16 ANSI names.
    Ansi,
    /// None at all. Modifiers only.
    Mono,
}

impl Depth {
    /// Negotiate from the environment, honouring `NO_COLOR` above everything else.
    ///
    /// `NO_COLOR` is checked for *presence*, not for a truthy value — that is what the
    /// convention says, and a user who exported `NO_COLOR=0` meaning "off" is a user whose
    /// terminal we should not be arguing with.
    pub fn detect(forced_mono: bool) -> Depth {
        if forced_mono || std::env::var_os("NO_COLOR").is_some() {
            return Depth::Mono;
        }
        if matches!(
            std::env::var("COLORTERM").ok().as_deref(),
            Some("truecolor") | Some("24bit")
        ) {
            return Depth::True;
        }
        let term = std::env::var("TERM").unwrap_or_default();
        if term == "dumb" {
            return Depth::Mono;
        }
        if term.contains("256") {
            return Depth::Xterm;
        }
        // The 16-colour terminals that still exist and still get used: a serial console, a
        // stripped-down container image, `TERM=linux` on a bare VT. They are rare enough
        // that nobody tests on them and common enough that the palette has to survive one,
        // which is why `Depth::Ansi` is a real arm rather than a theoretical one.
        if matches!(term.as_str(), "linux" | "ansi" | "vt100" | "vt220" | "cons25")
            || term.contains("16color")
        {
            return Depth::Ansi;
        }
        // Windows Terminal, the VS Code terminal and modern conhost all do truecolor and
        // none of them set COLORTERM reliably. Assuming truecolor and being wrong costs a
        // slightly approximated hue; assuming 16 colours and being wrong costs the whole
        // palette.
        Depth::True
    }
}

/// The palette, bound to a depth.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub depth: Depth,
}

impl Theme {
    pub fn new(depth: Depth) -> Self {
        Theme { depth }
    }

    fn colour(&self, i: Ink) -> Option<Color> {
        match self.depth {
            Depth::True => Some(Color::Rgb(i.rgb.0, i.rgb.1, i.rgb.2)),
            Depth::Xterm => Some(Color::Indexed(i.xterm)),
            Depth::Ansi => Some(i.ansi),
            Depth::Mono => None,
        }
    }

    /// The one place a [`Role`] becomes a [`Style`].
    ///
    /// The `Modifier`s are not garnish. In [`Depth::Mono`] they are the *only* thing left,
    /// so every role that needs to be distinguishable without colour carries a modifier
    /// too: dim for the recessive tones, bold for the claims. A role whose entire identity
    /// is a hue disappears on a monochrome terminal, and that is a bug in the role, not in
    /// the terminal.
    pub fn style(&self, role: Role) -> Style {
        let (ink, modifier) = match role {
            Role::Ground => (VOID, Modifier::empty()),
            Role::Panel => (PANEL, Modifier::empty()),
            Role::Selection => (SELECT, Modifier::empty()),
            // DIM rather than plain, so an inactive border recedes from the focused one
            // even where there is no colour at all. Without it `Chrome` and `ChromeFocus`
            // are one bold apart on a monochrome terminal, and "which pane has focus"
            // becomes a guess.
            Role::Chrome => (RULE, Modifier::DIM),
            Role::ChromeFocus => (RULE_HOT, Modifier::BOLD),
            Role::Body => (TEXT, Modifier::empty()),
            Role::Label => (MUTED, Modifier::empty()),
            Role::Hint => (MUTED, Modifier::DIM),

            Role::Measured => (VIOLET, Modifier::empty()),
            Role::Unmeasured => (GHOST, Modifier::DIM),
            Role::Heading => (VIOLET_LO, Modifier::BOLD),
            Role::HeadingActive => (VIOLET_HI, Modifier::BOLD),

            Role::Chosen => (AZURE, Modifier::BOLD),
            Role::Runner => (TEXT, Modifier::empty()),
            Role::Excluded => (GHOST, Modifier::DIM),
            Role::Abstained => (AMBER, Modifier::BOLD),

            Role::Risk => (ROSE, Modifier::empty()),
            Role::Opportunity => (MINT, Modifier::empty()),
            Role::Estimated => (AMBER, Modifier::DIM),

            Role::Live => (MINT, Modifier::empty()),
            Role::Stale => (AMBER, Modifier::empty()),
            Role::Absent => (GHOST, Modifier::DIM),
            Role::Simulated => (AZURE_LO, Modifier::ITALIC),

            Role::Valid => (AZURE, Modifier::BOLD),
            Role::Invalid => (ROSE, Modifier::BOLD | Modifier::REVERSED),
            Role::Working => (VIOLET_HI, Modifier::BOLD),
        };
        match self.colour(ink) {
            Some(c) => Style::default().fg(c).add_modifier(modifier),
            None => Style::default().add_modifier(modifier),
        }
    }

    /// A style whose *background* is the role's ink. Only backgrounds belong here —
    /// `Ground`, `Panel`, `Selection`.
    pub fn bg(&self, role: Role) -> Style {
        let ink = match role {
            Role::Ground => VOID,
            Role::Selection => SELECT,
            // Anything else asking for a background gets the panel ground. Silently
            // painting a body colour behind text would be worse than a visible fallback.
            _ => PANEL,
        };
        match self.colour(ink) {
            Some(c) => Style::default().bg(c),
            None => Style::default(),
        }
    }

    /// Foreground of one role over the background of another.
    pub fn on(&self, fg: Role, bg: Role) -> Style {
        let f = self.style(fg);
        match self.bg(bg).bg {
            Some(c) => f.bg(c),
            None => f,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::new(Depth::detect(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Roles that *are* the baseline, so having no modifier is correct for them.
    ///
    /// `Label` is here because a label is **positional**: a column heading sits at the top
    /// of its column and a field name at the start of its line, so it is identifiable by
    /// where it is rather than by how it is drawn. Nothing is lost when the colour goes.
    const BASELINE: [Role; 6] =
        [Role::Body, Role::Runner, Role::Ground, Role::Panel, Role::Selection, Role::Label];

    /// Roles whose distinction is carried by the text itself rather than by a modifier —
    /// `RISK`/`OPP`, `LIVE`/`STALE`/`ABSENT`, a number versus an em dash. Listing them is
    /// the point: it forces the question "what word carries this when the colour is gone?"
    const TEXT_CARRIED: [Role; 5] =
        [Role::Measured, Role::Risk, Role::Opportunity, Role::Live, Role::Stale];

    #[test]
    fn every_role_survives_a_monochrome_terminal() {
        // Rule 2, mechanised. In `Mono` there is no colour at all, so a role that carries
        // no modifier is indistinguishable from body text — which means a distinction the
        // screen is supposed to make has silently vanished.
        let t = Theme::new(Depth::Mono);
        for r in Role::ALL {
            let s = t.style(r);
            assert!(s.fg.is_none(), "{r:?} must not emit a colour in Mono");
            if BASELINE.contains(&r) || TEXT_CARRIED.contains(&r) {
                continue;
            }
            assert!(
                s.add_modifier != Modifier::empty(),
                "{r:?} has no colour and no modifier in Mono — it would vanish"
            );
        }
    }

    #[test]
    fn unmeasured_is_dim_at_every_depth_not_merely_a_different_hue() {
        // The single most important thing this palette does. If an unmeasured term is only
        // a *hue* away from a measured one, a colourblind reader, a 16-colour terminal and
        // a screenshot all lose the distinction the entire type system below is protecting.
        for d in [Depth::True, Depth::Xterm, Depth::Ansi, Depth::Mono] {
            let s = Theme::new(d).style(Role::Unmeasured);
            assert!(s.add_modifier.contains(Modifier::DIM), "unmeasured at {d:?}");
        }
    }

    #[test]
    fn the_azure_accent_is_reserved_for_claims() {
        // Chosen and Valid are claims. Opportunity and Live are observations, and they must
        // not borrow the accent — the eye is being trained to read azure as "this is the
        // thing the agent asserted".
        let t = Theme::new(Depth::True);
        let azure = t.style(Role::Chosen).fg;
        assert_eq!(t.style(Role::Valid).fg, azure);
        assert_ne!(t.style(Role::Opportunity).fg, azure);
        assert_ne!(t.style(Role::Live).fg, azure);
        assert_ne!(t.style(Role::Measured).fg, azure);
    }

    #[test]
    fn the_violets_stay_distinct_on_a_256_colour_terminal() {
        // The reason the xterm indices are hand-picked rather than computed. A nearest-cube
        // search collapses these and then "measured", "heading" and "focus border" render
        // identically.
        let t = Theme::new(Depth::Xterm);
        let mut seen: Vec<_> = [Role::Measured, Role::Heading, Role::HeadingActive, Role::Unmeasured]
            .iter()
            .map(|r| format!("{:?}", t.style(*r).fg))
            .collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 4, "the violets must not collapse at 256 colours");
    }

    #[test]
    fn no_color_beats_every_other_signal() {
        // Presence, not truthiness — the convention is explicit about that.
        assert_eq!(Depth::detect(true), Depth::Mono);
    }
}
