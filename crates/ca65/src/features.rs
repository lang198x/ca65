//! The switches a `.feature` command makes, read once for the expander,
//! which acts on them, and for lowering, which reports what is wrong.

use lang198x_syntax::{SyntaxToken, TextRange};

use crate::Feature;
use crate::ast::{Command, View};
use crate::kinds;

/// Whether this crate reads source the way `feature` asks. The rest are
/// refused as not supported yet.
pub(crate) const fn is_read(feature: Feature) -> bool {
    matches!(
        feature,
        Feature::AtInIdentifiers
            | Feature::CComments
            | Feature::DollarInIdentifiers
            | Feature::DollarIsPc
            | Feature::LeadingDotInIdentifiers
            | Feature::LineContinuations
            | Feature::LooseCharTerm
            | Feature::LooseStringTerm
            | Feature::MissingCharTerm
            | Feature::StringEscapes
            | Feature::UnderlineInNumbers
    )
}

/// One feature a `.feature` command turns on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Switch {
    /// The feature, or `None` for `addrsize`, which ca65 still accepts with
    /// a warning that it does nothing.
    pub(crate) feature: Option<Feature>,
    /// Whether the feature is turned on.
    pub(crate) on: bool,
    /// Where the feature is named.
    pub(crate) range: TextRange,
}

/// The switches `command` makes, in order, or where the first problem is
/// and what it is, in ca65 V2.19's words.
///
/// Each switch is a feature's name, in any case, then `+`, `-`, `on` or
/// `off`, or nothing, which turns it on. Switches are separated by commas.
pub(crate) fn switches(command: &Command) -> Result<Vec<Switch>, (TextRange, String)> {
    let range = command.syntax().text_range();
    let tokens: Vec<SyntaxToken> = command
        .tokens()
        .map(|list| list.tokens().collect())
        .unwrap_or_default();
    let mut tokens = tokens.into_iter().peekable();
    let mut found = Vec::new();
    loop {
        let Some(name) = tokens.next().filter(|t| t.kind() == kinds::IDENT) else {
            return Err((range, "Expected an identifier".into()));
        };
        let feature = match Feature::named(name.text()) {
            Some(feature) => Some(feature),
            None if name.text().eq_ignore_ascii_case("addrsize") => None,
            None => {
                return Err((
                    name.text_range(),
                    format!("Invalid feature: `{}`", name.text()),
                ));
            }
        };
        let on = match tokens.next_if(|t| t.kind() != kinds::COMMA) {
            None => true,
            Some(t) if t.kind() == kinds::PLUS || t.text().eq_ignore_ascii_case("on") => true,
            Some(t) if t.kind() == kinds::MINUS || t.text().eq_ignore_ascii_case("off") => false,
            Some(t) => return Err((t.text_range(), "Expected ON or OFF".into())),
        };
        found.push(Switch {
            feature,
            on,
            range: name.text_range(),
        });
        match tokens.next() {
            None => return Ok(found),
            Some(t) if t.kind() == kinds::COMMA => {}
            Some(t) => return Err((t.text_range(), "Expected end-of-line".into())),
        }
    }
}
