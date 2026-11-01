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

mod mnemonics;

/// A CPU ca65 assembles for, as its `--cpu` option and `.setcpu` name it.
///
/// Every CPU ca65 V2.19 knows is here, so the expander can follow each one.
/// Lowering handles only those [`Cpu::lowered`] says it does; the others
/// are refused by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Cpu {
    /// The NMOS 6502, ca65's default.
    Mos6502,
    /// The NMOS 6502 with its undocumented instructions.
    Nmos6502X,
    /// The 6502 of the C64 Direct-to-TV.
    Dtv6502,
    /// The 65SC02: the CMOS 6502 without the bit instructions.
    Mos65sc02,
    /// The 65C02, with Rockwell's bit instructions.
    Mos65c02,
    /// WDC's 65C02, with `WAI` and `STP` too.
    Wdc65c02,
    /// The CSG 65CE02.
    Csg65ce02,
    /// The CSG 4510 of the Commodore 65: a 65CE02 with `MAP`.
    Csg4510,
    /// The 45GS02 of the MEGA65.
    Mega45gs02,
    /// The WDC 65816.
    Wdc65816,
    /// Wozniak's SWEET16, the Apple II's 16-bit interpreter.
    Sweet16,
    /// Hudson's HuC6280, of the PC Engine.
    HuC6280,
    /// Mitsubishi's 740 microcontrollers.
    M740,
}

impl Cpu {
    /// Every CPU, in the order ca65 lists them.
    pub const ALL: [Self; 13] = [
        Self::Mos6502,
        Self::Nmos6502X,
        Self::Dtv6502,
        Self::Mos65sc02,
        Self::Mos65c02,
        Self::Wdc65816,
        Self::Sweet16,
        Self::HuC6280,
        Self::M740,
        Self::Csg4510,
        Self::Mega45gs02,
        Self::Wdc65c02,
        Self::Csg65ce02,
    ];

    /// The name ca65 gives this CPU in `--cpu` and `.setcpu`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mos6502 => "6502",
            Self::Nmos6502X => "6502X",
            Self::Dtv6502 => "6502DTV",
            Self::Mos65sc02 => "65SC02",
            Self::Mos65c02 => "65C02",
            Self::Wdc65c02 => "W65C02",
            Self::Csg65ce02 => "65CE02",
            Self::Csg4510 => "4510",
            Self::Mega45gs02 => "45GS02",
            Self::Wdc65816 => "65816",
            Self::Sweet16 => "sweet16",
            Self::HuC6280 => "huc6280",
            Self::M740 => "m740",
        }
    }

    /// The CPU `.setcpu` names `name`, in any case.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|cpu| cpu.name().eq_ignore_ascii_case(name))
    }

    /// The command that chooses this CPU, such as `.p816`, in lower case.
    #[must_use]
    pub const fn command(self) -> &'static str {
        match self {
            Self::Mos6502 => ".p02",
            Self::Nmos6502X => ".p02x",
            Self::Dtv6502 => ".pdtv",
            Self::Mos65sc02 => ".psc02",
            Self::Mos65c02 => ".pc02",
            Self::Wdc65c02 => ".pwc02",
            Self::Csg65ce02 => ".pce02",
            Self::Csg4510 => ".p4510",
            Self::Mega45gs02 => ".p45gs02",
            Self::Wdc65816 => ".p816",
            Self::Sweet16 => ".psweet16",
            Self::HuC6280 => ".p6280",
            Self::M740 => ".pm740",
        }
    }

    /// The CPU `word`, such as `.P816`, chooses, in any case.
    #[must_use]
    pub fn chosen_by(word: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|cpu| cpu.command().eq_ignore_ascii_case(word))
    }

    /// The CPU the conditional `word`, such as `.IFP816`, asks about, in any
    /// case: the command's name with `if` after the `.`.
    #[must_use]
    pub fn asked_by(word: &str) -> Option<Self> {
        let rest = word
            .get(..3)?
            .eq_ignore_ascii_case(".if")
            .then(|| &word[3..])?;
        Self::ALL
            .into_iter()
            .find(|cpu| cpu.command()[1..].eq_ignore_ascii_case(rest))
    }

    /// Whether lowering handles this CPU's instructions yet.
    #[must_use]
    pub const fn lowered(self) -> bool {
        matches!(
            self,
            Self::Mos6502
                | Self::Nmos6502X
                | Self::Mos65sc02
                | Self::Mos65c02
                | Self::Wdc65c02
                | Self::Csg65ce02
                | Self::Csg4510
                | Self::HuC6280
                | Self::Wdc65816
                | Self::Dtv6502
                | Self::Mega45gs02
        )
    }

    /// The mnemonics of this CPU's instructions, in upper case and in the
    /// order of ca65 V2.19's table for it. ca65 ignores their case.
    #[must_use]
    pub const fn mnemonics(self) -> &'static [&'static str] {
        match self {
            Self::Mos6502 => mnemonics::MOS_6502,
            Self::Nmos6502X => mnemonics::NMOS_6502X,
            Self::Dtv6502 => mnemonics::DTV_6502,
            Self::Mos65sc02 => mnemonics::MOS_65SC02,
            Self::Mos65c02 => mnemonics::MOS_65C02,
            Self::Wdc65c02 => mnemonics::WDC_W65C02,
            Self::Csg65ce02 => mnemonics::CSG_65CE02,
            Self::Csg4510 => mnemonics::CSG_4510,
            Self::Mega45gs02 => mnemonics::MEGA_45GS02,
            Self::Wdc65816 => mnemonics::WDC_65816,
            Self::Sweet16 => mnemonics::SWEET16,
            Self::HuC6280 => mnemonics::HUC6280,
            Self::M740 => mnemonics::M740,
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
    /// Every CPU has `A`, `X` and `Y`. ca65's scanner adds `S` for the
    /// 65CE02 family and the 65816, `SP` and `Z` for the 65CE02 family, `Q`
    /// for the 45GS02, and `R0` to `R15` under SWEET16.
    #[must_use]
    pub const fn registers(self) -> &'static [&'static str] {
        match self {
            Self::Csg65ce02 | Self::Csg4510 => &["A", "X", "Y", "S", "SP", "Z"],
            Self::Mega45gs02 => &["A", "X", "Y", "S", "SP", "Z", "Q"],
            Self::Wdc65816 => &["A", "X", "Y", "S"],
            Self::Sweet16 => &[
                "A", "X", "Y", "R0", "R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10",
                "R11", "R12", "R13", "R14", "R15",
            ],
            _ => &["A", "X", "Y"],
        }
    }

    /// The identifier of the profile for this CPU.
    const fn profile_id(self) -> &'static str {
        match self {
            Self::Mos6502 => "ca65-2.19-6502",
            Self::Nmos6502X => "ca65-2.19-6502x",
            Self::Dtv6502 => "ca65-2.19-6502dtv",
            Self::Mos65sc02 => "ca65-2.19-65sc02",
            Self::Mos65c02 => "ca65-2.19-65c02",
            Self::Wdc65c02 => "ca65-2.19-w65c02",
            Self::Csg65ce02 => "ca65-2.19-65ce02",
            Self::Csg4510 => "ca65-2.19-4510",
            Self::Mega45gs02 => "ca65-2.19-45gs02",
            Self::Wdc65816 => "ca65-2.19-65816",
            Self::Sweet16 => "ca65-2.19-sweet16",
            Self::HuC6280 => "ca65-2.19-huc6280",
            Self::M740 => "ca65-2.19-m740",
        }
    }
}

/// The widths of the 65816's accumulator and index registers, in bits.
///
/// `.a8`, `.a16`, `.i8` and `.i16` set them, and so do `rep` and `sep`
/// under `.smart`. On the 65816 they size an immediate, and `.asize` and
/// `.isize` give them. Both start at 8. ca65 keeps them while another CPU
/// is chosen, but uses them only on the 65816.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Widths {
    accumulator16: bool,
    index16: bool,
}

impl Widths {
    /// The accumulator's width: 8 or 16.
    #[must_use]
    pub const fn accumulator(self) -> u8 {
        if self.accumulator16 { 16 } else { 8 }
    }

    /// The index registers' width: 8 or 16.
    #[must_use]
    pub const fn index(self) -> u8 {
        if self.index16 { 16 } else { 8 }
    }

    /// These widths, with the accumulator 16 bits wide if `sixteen`, or
    /// else 8.
    #[must_use]
    pub const fn with_accumulator(self, sixteen: bool) -> Self {
        Self {
            accumulator16: sixteen,
            ..self
        }
    }

    /// These widths, with the index registers 16 bits wide if `sixteen`,
    /// or else 8.
    #[must_use]
    pub const fn with_index(self, sixteen: bool) -> Self {
        Self {
            index16: sixteen,
            ..self
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
        Self {
            id: cpu.profile_id(),
            cpu,
            ..self
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
    fn every_cpu_has_ca65s_table_and_names() {
        // The sizes of ca65 V2.19's tables in `instr.c`.
        let sizes = [56, 75, 71, 66, 98, 100, 26, 135, 106, 133, 149, 100, 133];
        for (cpu, size) in Cpu::ALL.into_iter().zip(sizes) {
            assert_eq!(cpu.mnemonics().len(), size, "{}", cpu.name());
            // ca65 looks a mnemonic up by binary search, so its tables are
            // sorted.
            assert!(
                cpu.mnemonics().windows(2).all(|pair| pair[0] < pair[1]),
                "{}",
                cpu.name()
            );
            assert_eq!(Cpu::named(cpu.name()), Some(cpu));
            assert_eq!(
                Cpu::chosen_by(&cpu.command().to_ascii_uppercase()),
                Some(cpu)
            );
            let test = format!(".IF{}", cpu.command()[1..].to_ascii_uppercase());
            assert_eq!(Cpu::asked_by(&test), Some(cpu), "{test}");
            assert!(cpu.registers().starts_with(&["A", "X", "Y"]));
            let profile = Profile::CA65_2_19_6502.with_cpu(cpu);
            assert_eq!(profile.cpu(), cpu);
            assert!(profile.id().starts_with("ca65-2.19-"));
        }
        assert_eq!(Cpu::named("6510"), None);
        assert_eq!(Cpu::chosen_by(".p6510"), None);
        assert_eq!(Cpu::asked_by(".if"), None);
        assert_eq!(Cpu::asked_by(".ifp6510"), None);
        assert_eq!(Cpu::Mega45gs02.registers().last(), Some(&"Q"));
        assert_eq!(Cpu::Sweet16.registers().len(), 19);
        assert!(Cpu::Mos6502.lowered() && Cpu::Csg4510.lowered());
        assert!(Cpu::Wdc65816.lowered() && Cpu::Dtv6502.lowered());
        assert!(!Cpu::M740.lowered() && !Cpu::Sweet16.lowered() && Cpu::Mega45gs02.lowered());
    }

    #[test]
    fn the_4510_is_a_cpu_setcpu_can_name() {
        assert_eq!(Cpu::named("4510"), Some(Cpu::Csg4510));
        assert_eq!(Cpu::named("6502"), Some(Cpu::Mos6502));
        assert_eq!(Cpu::named("65816"), Some(Cpu::Wdc65816));
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
