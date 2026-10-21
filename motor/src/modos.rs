//! Entregas (alvo de loudness, teto, tipo de EQ) e caracteres (dinâmica).

/// Entrega: o alvo e o que a análise pode mexer.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modo {
    /// −14 LUFS, −1 dBTP: o Mastering Assistant do Logic (medido)
    Musica = 0,
    /// −16 LUFS, −1 dBTP
    Web = 1,
    /// EBU R128: −23 LUFS, −1 dBTP
    Tv = 2,
    /// ATSC A/85: −24 LKFS, −2 dBTP
    TvEua = 3,
}

impl Modo {
    pub fn de_i32(v: i32) -> Option<Modo> {
        [Modo::Musica, Modo::Web, Modo::Tv, Modo::TvEua].into_iter().find(|m| *m as i32 == v)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoEq {
    Musica,
    Voz,
}

pub struct ConfigModo {
    pub alvo_lufs: f64,
    pub teto_db: f64,
    pub eq: TipoEq,
    pub width: bool,
    /// teto de ganho, dB (None: sem teto)
    pub ganho_max_db: Option<f64>,
}

pub const MODOS: [ConfigModo; 4] = [
    ConfigModo { alvo_lufs: -14.0, teto_db: -1.0, eq: TipoEq::Musica, width: true, ganho_max_db: None },
    ConfigModo { alvo_lufs: -16.0, teto_db: -1.0, eq: TipoEq::Voz, width: false, ganho_max_db: Some(15.0) },
    ConfigModo { alvo_lufs: -23.0, teto_db: -1.0, eq: TipoEq::Voz, width: false, ganho_max_db: Some(15.0) },
    ConfigModo { alvo_lufs: -24.0, teto_db: -2.0, eq: TipoEq::Voz, width: false, ganho_max_db: Some(15.0) },
];

/// Caráter: só a dinâmica. Na interface: CLEAN = DENSE, VALVE = WARM,
/// PUNCH/TRANSPARENT = TRANSPARENT (no ruído de teste os dois saíram iguais).
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Carater {
    Clean = 0,
    Valve = 1,
    Punch = 2,
    Transparent = 3,
}

impl Carater {
    pub fn de_i32(v: i32) -> Option<Carater> {
        [Carater::Clean, Carater::Valve, Carater::Punch, Carater::Transparent].into_iter().find(|c| *c as i32 == v)
    }
}

pub struct Compressor {
    /// limiar a −14 LUFS; acompanha o alvo das outras entregas
    pub limiar_db: f64,
    pub razao: f64,
    pub joelho_db: f64,
}

pub struct ConfigCarater {
    pub compressor: Option<Compressor>,
    pub limitador_release_ms: f64,
    /// saturação `tanh` (0 = desligada)
    pub saturacao: f64,
}

/// Ajustados por busca contra os bounces do Logic (testes 10 e 11 e um trecho de som direto), 20/10/2026.
pub const CARATERES: [ConfigCarater; 4] = [
    ConfigCarater {
        compressor: Some(Compressor { limiar_db: -16.5, razao: 1.8, joelho_db: 6.0 }),
        limitador_release_ms: 50.0,
        saturacao: 0.0,
    },
    ConfigCarater { compressor: None, limitador_release_ms: 50.0, saturacao: 0.15 },
    ConfigCarater { compressor: None, limitador_release_ms: 2.0, saturacao: 0.0 },
    ConfigCarater { compressor: None, limitador_release_ms: 2.0, saturacao: 0.0 },
];

pub const COMP_ATAQUE_MS: f64 = 30.0;
pub const COMP_RELEASE_MS: f64 = 150.0;
pub const COMP_RMS_MS: f64 = 5.0;
pub const LOOKAHEAD_MS: f64 = 1.0;

/// Tudo o que muda o som, como a janela mostra. Os modos são só pontos de partida.
#[derive(Clone, Copy, Debug)]
pub struct Ajustes {
    /// alvo de loudness integrado (TARGET)
    pub alvo_lufs: f64,
    /// teto de true peak (CEILING)
    pub teto_db: f64,
    /// curva de EQ: a de música (Mastering Assistant) ou a de voz
    pub curva: TipoEq,
    /// quanto da curva aplicar, 0 a 1 (AUTO EQ)
    pub eq_quantidade: f64,
    /// passa-altas de 2ª ordem, Hz; 0 = desligado (HIGH-PASS)
    pub passa_altas_hz: f64,
    /// ganho do Side em dB; None = o que a análise sugere (WIDTH)
    pub side_db: Option<f64>,
    /// teto de ganho, dB; None = sem teto
    pub ganho_max_db: Option<f64>,
    pub carater: Carater,
    /// EQ de 4 pontos (a da janela); None = a curva da análise × AUTO EQ (protótipo)
    pub eq_bandas: Option<[crate::eq4::Banda; 4]>,
}

impl Ajustes {
    /// Os ajustes de uma entrega como no protótipo (e nos testes de paridade).
    pub fn de_modo(modo: Modo, carater: Carater) -> Ajustes {
        let m = &MODOS[modo as usize];
        Ajustes {
            alvo_lufs: m.alvo_lufs,
            teto_db: m.teto_db,
            curva: m.eq,
            eq_quantidade: 1.0,
            passa_altas_hz: if m.eq == TipoEq::Voz { crate::analise::VOZ_PASSA_ALTAS_HZ } else { 0.0 },
            side_db: if m.width { None } else { Some(0.0) },
            ganho_max_db: m.ganho_max_db,
            carater,
            eq_bandas: None,
        }
    }
}
