//! Profiles, CPUs and the lexer's mode.

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

    /// The names ca65 reads as registers on this CPU, in upper case. ca65
    /// ignores their case.
    ///
    /// The 6502 has `A`, `X` and `Y`. Later CPUs add more, such as the
    /// 65816's `S`, and they arrive with those CPUs.
    #[must_use]
    pub const fn registers(self) -> &'static [&'static str] {
        match self {
            Self::Mos6502 => &["A", "X", "Y"],
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

/// What the lexer needs to know about the source at one point in it.
///
/// ca65 can change how it reads text partway through a file. `.setcpu`
/// changes which names are registers, and `.feature`, `.case` and
/// `.linecont` change what counts as a name, a number, a comment or a line.
/// So the parser hands the lexer a mode for each token it asks for.
///
/// Only ca65's default mode exists so far: every `.feature` off, names
/// case-sensitive and no line continuation. The CPU is the one setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    cpu: Cpu,
}

impl Mode {
    /// The mode a file starts in under `profile`.
    #[must_use]
    pub const fn of(profile: Profile) -> Self {
        Self { cpu: profile.cpu }
    }

    /// The CPU whose registers the lexer recognises.
    #[must_use]
    pub const fn cpu(self) -> Cpu {
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

    #[test]
    fn a_file_starts_in_its_profiles_cpu() {
        let mode = Mode::of(Profile::CA65_2_19_6502);
        assert_eq!(mode.cpu(), Cpu::Mos6502);
        assert_eq!(mode.cpu().registers(), ["A", "X", "Y"]);
    }
}
