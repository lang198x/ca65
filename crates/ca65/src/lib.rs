//! The ca65 assembler family for Lang198x.
//!
//! ca65 is the macro assembler of the cc65 suite. One dialect covers several
//! CPUs, from the 6502 to the 65816, so the CPU is an axis of the dialect's
//! [`Profile`] rather than a dialect of its own.
//!
//! This crate is starting. The plan, in the Lang198x hub's
//! `docs/plans/ca65-front-end.md`, builds the lexer, the parser,
//! preprocessing and lowering to `asm198x-ir` in turn, each checked against
//! ca65's own answers recorded in Asm198x.

/// A version of ca65, and the CPU it assembles for.
///
/// Each profile names the tool version it describes, because the answers it
/// encodes were measured on that version and no other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    id: &'static str,
    tool: &'static str,
    version: &'static str,
    cpu: Cpu,
}

/// A CPU ca65 assembles for, as its `--cpu` option and `.setcpu` name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cpu {
    /// The NMOS 6502, ca65's default.
    Mos6502,
}

impl Cpu {
    /// The name ca65 gives this CPU in `--cpu` and `.setcpu`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mos6502 => "6502",
        }
    }
}

impl Profile {
    /// ca65 V2.19 at cc65 commit `8d86ca7`, assembling for the 6502: the
    /// version Asm198x pins and records its verdicts with.
    pub const CA65_2_19_6502: Self = Self {
        id: "ca65-2.19-6502",
        tool: "ca65",
        version: "V2.19 - Git 8d86ca7",
        cpu: Cpu::Mos6502,
    };

    /// A short, stable identifier for this profile.
    #[must_use]
    pub const fn id(&self) -> &'static str {
        self.id
    }

    /// The tool this profile describes.
    #[must_use]
    pub const fn tool(&self) -> &'static str {
        self.tool
    }

    /// The tool's version, as its `--version` line prints it.
    #[must_use]
    pub const fn version(&self) -> &'static str {
        self.version
    }

    /// The CPU this profile assembles for.
    #[must_use]
    pub const fn cpu(&self) -> Cpu {
        self.cpu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_profile_names_the_pinned_tool_and_cpu() {
        let profile = Profile::CA65_2_19_6502;
        assert_eq!(profile.id(), "ca65-2.19-6502");
        assert_eq!(profile.tool(), "ca65");
        assert_eq!(profile.version(), "V2.19 - Git 8d86ca7");
        assert_eq!(profile.cpu(), Cpu::Mos6502);
        assert_eq!(profile.cpu().name(), "6502");
    }
}
