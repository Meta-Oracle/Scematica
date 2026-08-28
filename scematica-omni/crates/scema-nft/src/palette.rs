//! The plate's palette: the one file in this crate that contains a colour.
//!
//! Same rule as `scema_tui::theme` and `lib/mesh/view.ts::toneFor`, for the same reason:
//! **a renderer names a [`Role`], never a hex.** A rule that encodes a claim about trust
//! gets exactly one implementation, and the moment `plate.rs` can write a literal colour
//! inline, "risk" and "invalid" start drifting apart in different corners of the drawing.
//!
//! The inks are the console's, copied deliberately rather than imported. `scema-tui` pulls
//! in ratatui, and this crate is meant to be linkable from a CLI, a daemon and eventually a
//! contract-facing tool without dragging a terminal stack behind it. That makes this a
//! **port**, with the same status as `plugins/scema-web/src/theme.js`: `scema-tui` is
//! authoritative, and the test below pins the hexes so a drift fails the build rather than
//! quietly producing an off-brand plate.
//!
//! ## What is different here, and why
//!
//! A terminal can fall back. `Depth::Mono` strips colour and the console still reads,
//! because every state is also carried by a glyph or a word. **An SVG has no fallback** —
//! it is going to be rendered once, by a wallet or a marketplace, at whatever size that
//! viewer chose, and nobody is going to pipe it through `less`. So the same discipline is
//! enforced structurally instead: every distinction the plate draws is carried by a
//! *shape* — dashed versus solid, filled versus outline, a notch versus an unbroken ring —
//! and colour only ever agrees with a shape that already said it. Print the plate in
//! greyscale and nothing is lost but pleasure.

/// A 24-bit ink. No terminal fallbacks: SVG has exactly one depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ink(pub u8, pub u8, pub u8);

impl Ink {
    /// Lowercase, always six digits.
    ///
    /// The SVG text is compared byte for byte against the TypeScript port, so "the same
    /// colour spelled differently" is a parity failure rather than a cosmetic one.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

// The raw palette. Named by appearance here and only here.

const VOID: Ink = Ink(0x08, 0x06, 0x0F);
const PANEL: Ink = Ink(0x0F, 0x0B, 0x1A);
const RULE: Ink = Ink(0x2A, 0x1F, 0x45);
const RULE_HOT: Ink = Ink(0x6D, 0x40, 0xC4);
const TEXT: Ink = Ink(0xE6, 0xE0, 0xF5);
const MUTED: Ink = Ink(0x8A, 0x81, 0xA8);
const GHOST: Ink = Ink(0x54, 0x4C, 0x6C);
/// A counted absence: a `Provenance::Absent` object, a reported blind spot.
///
/// Deliberately **lighter than `GHOST`**, and the one place this palette diverges from
/// `scema-tui`, which maps both to the same ghost. That is right in a terminal, where an
/// absence is the word `ABSENT` and a dim word is still a word. It is wrong here: a 3px
/// dashed arc in `GHOST` against this ground is not recessive, it is invisible, and an
/// invisible arc in a composition ring silently shrinks the denominator — a viewer counts
/// the segments they can see and concludes the parts do not sum to the whole.
///
/// The deeper reason is that the two are not the same kind of thing. `GHOST` stands in for
/// something **nobody measured**. An absent object and a reported blind spot were *counted*:
/// somebody looked, failed, and recorded the failure. That is a measurement about ignorance
/// and it has earned its ink.
const SLATE: Ink = Ink(0x6F, 0x66, 0x90);
const VIOLET: Ink = Ink(0xA9, 0x6B, 0xFF);
const VIOLET_HI: Ink = Ink(0xCB, 0xA6, 0xFF);
const AZURE: Ink = Ink(0x7D, 0xD3, 0xFC);
const AMBER: Ink = Ink(0xF2, 0xB1, 0x5C);
const ROSE: Ink = Ink(0xFF, 0x7B, 0x9C);
const MINT: Ink = Ink(0x86, 0xE5, 0xC0);

/// What the plate is allowed to ask for.
///
/// Short and boring on purpose: every entry is a distinction somebody has to be able to
/// make while looking at the finished image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The plate ground.
    Ground,
    /// The inner field, one step up from `Ground`.
    Field,
    /// Rules, tracks, and the unfilled part of any gauge.
    Chrome,
    /// The plate border.
    Frame,
    /// Ordinary text.
    Body,
    /// Field labels and axis text.
    Label,
    /// Anything standing in for something nobody measured. Must read as *quieter* than
    /// `Body`, never merely a different hue — the em-dash rule in pigment.
    Ghost,
    /// A measured quantity.
    Measured,
    /// The title.
    Heading,
    /// The commitment. Azure is reserved for a claim, exactly as in the console.
    Claim,
    /// A counted risk signal.
    Risk,
    /// A counted opportunity signal.
    Opportunity,
    /// Staleness, and the unbounded-extent marker.
    Stale,
    /// A counted absence: a `Provenance::Absent` object, a reported blind spot.
    ///
    /// Not the same role as [`Role::Ghost`] and not the same ink. Somebody looked and
    /// recorded the failure, which is a measurement — see `SLATE`.
    Absent,
}

impl Role {
    /// Every role, for the parity test and the palette strip.
    pub const ALL: [Role; 14] = [
        Role::Ground,
        Role::Field,
        Role::Chrome,
        Role::Frame,
        Role::Body,
        Role::Label,
        Role::Ghost,
        Role::Measured,
        Role::Heading,
        Role::Claim,
        Role::Risk,
        Role::Opportunity,
        Role::Stale,
        Role::Absent,
    ];

    /// The wire name, used in the parity fixture and by the TypeScript port.
    pub fn name(self) -> &'static str {
        match self {
            Role::Ground => "ground",
            Role::Field => "field",
            Role::Chrome => "chrome",
            Role::Frame => "frame",
            Role::Body => "body",
            Role::Label => "label",
            Role::Ghost => "ghost",
            Role::Measured => "measured",
            Role::Heading => "heading",
            Role::Claim => "claim",
            Role::Risk => "risk",
            Role::Opportunity => "opportunity",
            Role::Stale => "stale",
            Role::Absent => "absent",
        }
    }

    /// The ink for this role.
    pub fn ink(self) -> Ink {
        match self {
            Role::Ground => VOID,
            Role::Field => PANEL,
            Role::Chrome => RULE,
            Role::Frame => RULE_HOT,
            Role::Body => TEXT,
            Role::Label => MUTED,
            Role::Ghost => GHOST,
            Role::Measured => VIOLET,
            Role::Heading => VIOLET_HI,
            Role::Claim => AZURE,
            Role::Risk => ROSE,
            Role::Opportunity => MINT,
            Role::Stale => AMBER,
            Role::Absent => SLATE,
        }
    }

    /// Shorthand for `self.ink().hex()`.
    pub fn hex(self) -> String {
        self.ink().hex()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_lowercase_and_padded() {
        // Byte-compared against the TS port. The same colour spelled two ways is a parity
        // failure, and a two-digit component would shift every later byte in the file.
        assert_eq!(Ink(0x08, 0x06, 0x0F).hex(), "#08060f");
        assert_eq!(Ink(0, 0, 0).hex(), "#000000");
        assert_eq!(Ink(255, 255, 255).hex(), "#ffffff");
    }

    #[test]
    fn ghost_is_darker_than_body() {
        // The rule this crate exists to serve: whatever stands in for an unmeasured thing
        // must recede, at a glance, without the viewer reading a legend. Compare luminance
        // rather than hue — a different-hue-same-brightness pair reads as a category, not
        // as an absence.
        let lum = |i: Ink| 0.2126 * i.0 as f64 + 0.7152 * i.1 as f64 + 0.0722 * i.2 as f64;
        assert!(lum(Role::Ghost.ink()) < lum(Role::Body.ink()));
        assert!(lum(Role::Ghost.ink()) < lum(Role::Measured.ink()));
    }

    #[test]
    fn every_role_has_a_distinct_name() {
        let mut names: Vec<&str> = Role::ALL.iter().map(|r| r.name()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "two roles share a wire name");
    }

    #[test]
    fn the_signalling_roles_do_not_collide() {
        // Risk, opportunity and stale are the three the eye has to separate at thumbnail
        // size. Shape carries each of them too, but a palette where two of them were the
        // same ink would make the shape do all the work.
        assert_ne!(Role::Risk.hex(), Role::Opportunity.hex());
        assert_ne!(Role::Risk.hex(), Role::Stale.hex());
        assert_ne!(Role::Opportunity.hex(), Role::Stale.hex());
    }

    #[test]
    fn a_counted_absence_is_visible_and_an_unmeasured_stand_in_is_not() {
        // The distinction that cost a render to notice: the console maps both to the same
        // ghost, which is right for a dim *word* and wrong for a 3px dashed arc against
        // near-black. An invisible segment in a composition ring silently shrinks the
        // denominator. And the two are not the same kind of claim — an absent object was
        // counted, so it has earned its ink.
        let lum = |i: Ink| 0.2126 * i.0 as f64 + 0.7152 * i.1 as f64 + 0.0722 * i.2 as f64;
        assert!(
            lum(Role::Absent.ink()) > lum(Role::Ghost.ink()),
            "a counted absence must be more visible than an unmeasured stand-in"
        );
        // Still clearly recessive against the ground, or it stops reading as an absence.
        assert!(lum(Role::Absent.ink()) < lum(Role::Body.ink()));
        assert!(lum(Role::Absent.ink()) > 4.0 * lum(Role::Ground.ink()));
    }

    #[test]
    fn the_console_hexes_have_not_drifted() {
        // `scema-tui::theme` is authoritative. This crate copies rather than imports it,
        // to stay free of ratatui, so the copy needs a tripwire — the same arrangement as
        // `plugins/scema-web/test/theme.test.js`.
        assert_eq!(Role::Ground.hex(), "#08060f");
        assert_eq!(Role::Field.hex(), "#0f0b1a");
        assert_eq!(Role::Chrome.hex(), "#2a1f45");
        assert_eq!(Role::Frame.hex(), "#6d40c4");
        assert_eq!(Role::Body.hex(), "#e6e0f5");
        assert_eq!(Role::Label.hex(), "#8a81a8");
        assert_eq!(Role::Ghost.hex(), "#544c6c");
        assert_eq!(Role::Measured.hex(), "#a96bff");
        assert_eq!(Role::Heading.hex(), "#cba6ff");
        assert_eq!(Role::Claim.hex(), "#7dd3fc");
        assert_eq!(Role::Risk.hex(), "#ff7b9c");
        assert_eq!(Role::Opportunity.hex(), "#86e5c0");
        assert_eq!(Role::Stale.hex(), "#f2b15c");
    }
}
