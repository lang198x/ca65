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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Cpu {
    /// The NMOS 6502, ca65's default.
    Mos6502,
    /// The CSG 4510 of the Commodore 65: a 65CE02 with `MAP`.
    Csg4510,
}

impl Cpu {
    /// The name ca65 gives this CPU in `--cpu` and `.setcpu`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mos6502 => "6502",
            Self::Csg4510 => "4510",
        }
    }

    /// The CPU `.setcpu` names `name`, in any case, if a profile has it.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        [Self::Mos6502, Self::Csg4510]
            .into_iter()
            .find(|cpu| cpu.name().eq_ignore_ascii_case(name))
    }

    /// The mnemonics of this CPU's instructions, in upper case. ca65 ignores
    /// their case.
    ///
    /// For the 6502, the 56 in ca65 V2.19's `InsTab6502`.
    #[must_use]
    pub const fn mnemonics(self) -> &'static [&'static str] {
        match self {
            Self::Mos6502 => &[
                "ADC", "AND", "ASL", "BCC", "BCS", "BEQ", "BIT", "BMI", "BNE", "BPL", "BRK", "BVC",
                "BVS", "CLC", "CLD", "CLI", "CLV", "CMP", "CPX", "CPY", "DEC", "DEX", "DEY", "EOR",
                "INC", "INX", "INY", "JMP", "JSR", "LDA", "LDX", "LDY", "LSR", "NOP", "ORA", "PHA",
                "PHP", "PLA", "PLP", "ROL", "ROR", "RTI", "RTS", "SBC", "SEC", "SED", "SEI", "STA",
                "STX", "STY", "TAX", "TAY", "TSX", "TXA", "TXS", "TYA",
            ],
            // The 133 in ca65 V2.19's `InsTab4510`.
            Self::Csg4510 => &[
                "ADC", "AND", "ASL", "ASR", "ASW", "BBR0", "BBR1", "BBR2", "BBR3", "BBR4", "BBR5",
                "BBR6", "BBR7", "BBS0", "BBS1", "BBS2", "BBS3", "BBS4", "BBS5", "BBS6", "BBS7",
                "BCC", "BCS", "BEQ", "BIT", "BMI", "BNE", "BPL", "BRA", "BRK", "BSR", "BVC", "BVS",
                "CLC", "CLD", "CLE", "CLI", "CLV", "CMP", "CPX", "CPY", "CPZ", "DEA", "DEC", "DEW",
                "DEX", "DEY", "DEZ", "EOM", "EOR", "INA", "INC", "INW", "INX", "INY", "INZ", "JMP",
                "JSR", "LBCC", "LBCS", "LBEQ", "LBMI", "LBNE", "LBPL", "LBRA", "LBVC", "LBVS",
                "LDA", "LDX", "LDY", "LDZ", "LSR", "MAP", "NEG", "NOP", "ORA", "PHA", "PHD", "PHP",
                "PHW", "PHX", "PHY", "PHZ", "PLA", "PLP", "PLX", "PLY", "PLZ", "RMB0", "RMB1",
                "RMB2", "RMB3", "RMB4", "RMB5", "RMB6", "RMB7", "ROL", "ROR", "ROW", "RTI", "RTN",
                "RTS", "SBC", "SEC", "SED", "SEE", "SEI", "SMB0", "SMB1", "SMB2", "SMB3", "SMB4",
                "SMB5", "SMB6", "SMB7", "STA", "STX", "STY", "STZ", "TAB", "TAX", "TAY", "TAZ",
                "TBA", "TRB", "TSB", "TSX", "TSY", "TXA", "TXS", "TYA", "TYS", "TZA",
            ],
        }
    }

    /// Whether `word` is one of this CPU's mnemonics, in any case.
    #[must_use]
    pub fn is_mnemonic(self, word: &str) -> bool {
        self.mnemonics()
            .iter()
            .any(|mnemonic| mnemonic.eq_ignore_ascii_case(word))
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
            // ca65's scanner gives the 65CE02 family `S`, `SP` and `Z` too.
            Self::Csg4510 => &["A", "X", "Y", "S", "SP", "Z"],
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

    /// ca65 V2.19 at cc65 commit `8d86ca7`, assembling for the 4510.
    pub const CA65_2_19_4510: Self = Self {
        id: "ca65-2.19-4510",
        tool: "ca65",
        version: "V2.19 - Git 8d86ca7",
        cpu: Cpu::Csg4510,
    };

    /// This version of ca65, assembling for `cpu`.
    #[must_use]
    pub const fn with_cpu(self, cpu: Cpu) -> Self {
        match cpu {
            Cpu::Mos6502 => Self::CA65_2_19_6502,
            Cpu::Csg4510 => Self::CA65_2_19_4510,
        }
    }

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

    #[test]
    fn the_4510_is_a_cpu_setcpu_can_name() {
        assert_eq!(Cpu::named("4510"), Some(Cpu::Csg4510));
        assert_eq!(Cpu::named("6502"), Some(Cpu::Mos6502));
        assert_eq!(Cpu::named("65816"), None);
        let profile = Profile::CA65_2_19_6502.with_cpu(Cpu::Csg4510);
        assert_eq!(profile, Profile::CA65_2_19_4510);
        assert_eq!(profile.id(), "ca65-2.19-4510");
        assert_eq!(profile.with_cpu(Cpu::Mos6502), Profile::CA65_2_19_6502);
        let cpu = Cpu::Csg4510;
        assert_eq!(cpu.mnemonics().len(), 133);
        assert!(cpu.mnemonics().windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(cpu.registers(), ["A", "X", "Y", "S", "SP", "Z"]);
    }

    #[test]
    fn the_6502_has_56_mnemonics_in_any_case() {
        let cpu = Cpu::Mos6502;
        assert_eq!(cpu.mnemonics().len(), 56);
        assert!(cpu.mnemonics().windows(2).all(|pair| pair[0] < pair[1]));
        assert!(cpu.is_mnemonic("lda") && cpu.is_mnemonic("LdA"));
        assert!(!cpu.is_mnemonic("bra") && !cpu.is_mnemonic("ld"));
    }
}
