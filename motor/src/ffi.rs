//! Interface em C para o plugin (ver `juce/Source/Apura.h`, que tem que bater com isto).
//! Nenhum pânico atravessa a fronteira: erro vira código de retorno.

use crate::analise::{analisar, BANDAS};
use crate::cadeia::{correlacao, masterizar_com, original_nivelado, pre_processar, Opcoes};
use crate::eq4::{self, Banda};
use crate::modos::{Ajustes, Carater, TipoEq};
use crate::tempo_real::{AjustesRT, ApuraRT, MedidasRT};
use crate::{loudness, true_peak};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub const APURA_OK: i32 = 0;
pub const APURA_ERRO_ARGUMENTOS: i32 = -1;
pub const APURA_ERRO_INTERNO: i32 = -2;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ApuraAjustes {
    pub alvo_lufs: f32,
    pub teto_db: f32,
    /// 0 = curva de música, 1 = curva de voz
    pub curva: i32,
    /// AUTO EQ, 0 a 1
    pub eq_quantidade: f32,
    /// HIGH-PASS, Hz; 0 = desligado
    pub passa_altas_hz: f32,
    /// 1 = largura sugerida pela análise; 0 = `width_db`
    pub width_auto: i32,
    pub width_db: f32,
    /// 1 = limitar o ganho a `ganho_max_db`
    pub tem_teto_de_ganho: i32,
    pub ganho_max_db: f32,
    /// 0 CLEAN, 1 VALVE, 2 PUNCH, 3 TRANSPARENT
    pub carater: i32,
    /// 1 = BYPASS · LOUDNESS MATCH: o original só com ganho até o alvo
    pub comparar: i32,
    /// 1 = medir também a faixa de loudness (LRA)
    pub medir_faixa: i32,
    /// 1 = EQ de 4 pontos (`bandas_*`); 0 = curva da análise × `eq_quantidade`
    pub eq_bandas: i32,
    pub bandas_freq: [f32; 4],
    pub bandas_ganho: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ApuraResultado {
    pub lufs_entrada: f32,
    pub lufs_saida: f32,
    pub tp_entrada: f32,
    pub tp_saida: f32,
    pub lra_entrada: f32,
    pub lra_saida: f32,
    pub ganho_db: f32,
    pub no_teto_de_ganho: i32,
    pub compressor_max_db: f32,
    pub limitador_max_db: f32,
    pub side_db: f32,
    pub side_sugerido_db: f32,
    pub sm_entrada: f32,
    pub correlacao_entrada: f32,
    pub correlacao_saida: f32,
    pub espectro_entrada_db: [f32; 30],
    pub espectro_saida_db: [f32; 30],
    /// curva aplicada
    pub eq_db: [f32; 30],
    /// as duas curvas sugeridas inteiras (para a janela mostrar com AUTO EQ em 0%)
    pub curva_musica_db: [f32; 30],
    pub curva_voz_db: [f32; 30],
}

impl Default for ApuraResultado {
    fn default() -> Self {
        // tudo zero é um estado válido ("nada medido ainda")
        unsafe { std::mem::zeroed() }
    }
}

fn para_f32(v: &[f64; 30]) -> [f32; 30] {
    let mut o = [0.0f32; 30];
    for (a, b) in o.iter_mut().zip(v) {
        *a = *b as f32;
    }
    o
}

fn ajustes_de(a: &ApuraAjustes) -> Option<Ajustes> {
    Some(Ajustes {
        alvo_lufs: a.alvo_lufs as f64,
        teto_db: a.teto_db as f64,
        curva: if a.curva == 1 { TipoEq::Voz } else { TipoEq::Musica },
        eq_quantidade: (a.eq_quantidade as f64).clamp(0.0, 1.0),
        passa_altas_hz: (a.passa_altas_hz as f64).max(0.0),
        side_db: if a.width_auto != 0 { None } else { Some(a.width_db as f64) },
        ganho_max_db: if a.tem_teto_de_ganho != 0 { Some(a.ganho_max_db as f64) } else { None },
        carater: Carater::de_i32(a.carater)?,
        eq_bandas: if a.eq_bandas != 0 { Some(bandas_de(&a.bandas_freq, &a.bandas_ganho)) } else { None },
    })
}

fn bandas_de(freq: &[f32; 4], ganho: &[f32; 4]) -> [Banda; 4] {
    let mut b = eq4::PADRAO;
    for k in 0..4 {
        b[k] = Banda { freq: freq[k] as f64, ganho_db: ganho[k] as f64 };
    }
    b
}

/// Lê os canais do plugin (float) para o motor (double).
unsafe fn ler_canais(entradas: *const *const f32, n_canais: u32, n: u64) -> Option<Vec<Vec<f64>>> {
    if entradas.is_null() || n_canais == 0 || n_canais > 8 {
        return None;
    }
    let ptrs = std::slice::from_raw_parts(entradas, n_canais as usize);
    let mut canais = Vec::with_capacity(n_canais as usize);
    for &p in ptrs {
        if p.is_null() {
            return None;
        }
        canais.push(std::slice::from_raw_parts(p, n as usize).iter().map(|&v| v as f64).collect());
    }
    Some(canais)
}

fn preencher_analise(res: &mut ApuraResultado, a: &crate::Analise) {
    res.espectro_entrada_db = para_f32(&a.espectro_db);
    res.curva_musica_db = para_f32(&a.curva_musica);
    res.curva_voz_db = para_f32(&a.curva_voz);
    res.side_sugerido_db = a.side_sugerido_db as f32;
    res.sm_entrada = a.sm_entrada as f32;
}

/// Processa o clip inteiro. `saidas` pode apontar para os mesmos buffers de `entradas`.
///
/// # Safety
/// `entradas` e `saidas` apontam para `n_canais` buffers de `n` amostras cada.
#[no_mangle]
pub unsafe extern "C" fn apura_processar(
    entradas: *const *const f32,
    saidas: *const *mut f32,
    n_canais: u32,
    n: u64,
    sr: f64,
    aj: *const ApuraAjustes,
    res: *mut ApuraResultado,
) -> i32 {
    let r = catch_unwind(AssertUnwindSafe(|| {
        if saidas.is_null() || aj.is_null() || !(sr > 0.0) {
            return APURA_ERRO_ARGUMENTOS;
        }
        let Some(canais) = ler_canais(entradas, n_canais, n) else { return APURA_ERRO_ARGUMENTOS };
        let Some(ajustes) = ajustes_de(&*aj) else { return APURA_ERRO_ARGUMENTOS };
        let mut out = ApuraResultado::default();
        let a = analisar(&canais, sr);
        preencher_analise(&mut out, &a);
        let y = if (*aj).comparar != 0 {
            let (y, ganho) = original_nivelado(&canais, sr, ajustes.alvo_lufs);
            out.ganho_db = ganho as f32;
            out.lufs_entrada = loudness::integrado(&canais, sr) as f32;
            out.lufs_saida = loudness::integrado(&y, sr) as f32;
            out.tp_entrada = true_peak::maximo_db(&canais) as f32;
            out.tp_saida = true_peak::maximo_db(&y) as f32;
            out.correlacao_entrada = correlacao(&canais) as f32;
            out.correlacao_saida = out.correlacao_entrada;
            out.espectro_saida_db = out.espectro_entrada_db;
            y
        } else {
            let op = Opcoes { medir_faixa: (*aj).medir_faixa != 0, ..Default::default() };
            let (y, r) = masterizar_com(&canais, sr, a, &ajustes, op);
            out.lufs_entrada = r.lufs_entrada as f32;
            out.lufs_saida = r.lufs_saida as f32;
            out.tp_entrada = r.tp_entrada as f32;
            out.tp_saida = r.tp_saida as f32;
            out.lra_entrada = r.lra_entrada as f32;
            out.lra_saida = r.lra_saida as f32;
            out.ganho_db = r.ganho_db as f32;
            out.no_teto_de_ganho = r.no_teto_de_ganho as i32;
            out.compressor_max_db = r.dinamica.compressor_max_db as f32;
            out.limitador_max_db = r.dinamica.limitador_max_db as f32;
            out.side_db = r.side_db as f32;
            out.correlacao_entrada = r.correlacao_entrada as f32;
            out.correlacao_saida = r.correlacao_saida as f32;
            out.espectro_saida_db = para_f32(&r.espectro_saida_db);
            out.eq_db = para_f32(&r.eq_db);
            y
        };
        let ptrs = std::slice::from_raw_parts(saidas, n_canais as usize);
        for (c, &p) in ptrs.iter().enumerate() {
            if p.is_null() {
                return APURA_ERRO_ARGUMENTOS;
            }
            let dst = std::slice::from_raw_parts_mut(p, n as usize);
            for (d, v) in dst.iter_mut().zip(&y[c]) {
                *d = *v as f32;
            }
        }
        if !res.is_null() {
            *res = out;
        }
        APURA_OK
    }));
    r.unwrap_or(APURA_ERRO_INTERNO)
}

/// Só a análise (rápida): espectro, curvas sugeridas, loudness e true peak da entrada,
/// e o ganho previsto para o alvo, sem processar.
///
/// # Safety
/// `entradas` aponta para `n_canais` buffers de `n` amostras cada.
#[no_mangle]
pub unsafe extern "C" fn apura_analisar(
    entradas: *const *const f32,
    n_canais: u32,
    n: u64,
    sr: f64,
    aj: *const ApuraAjustes,
    res: *mut ApuraResultado,
) -> i32 {
    let r = catch_unwind(AssertUnwindSafe(|| {
        if aj.is_null() || res.is_null() || !(sr > 0.0) {
            return APURA_ERRO_ARGUMENTOS;
        }
        let Some(canais) = ler_canais(entradas, n_canais, n) else { return APURA_ERRO_ARGUMENTOS };
        let Some(ajustes) = ajustes_de(&*aj) else { return APURA_ERRO_ARGUMENTOS };
        let mut out = ApuraResultado::default();
        let a = analisar(&canais, sr);
        preencher_analise(&mut out, &a);
        let eq_db = a.eq_final(&ajustes);
        let side_db = a.side_final(&ajustes);
        out.eq_db = para_f32(&eq_db);
        out.side_db = side_db as f32;
        out.lufs_entrada = loudness::integrado(&canais, sr) as f32;
        out.tp_entrada = true_peak::maximo_db(&canais) as f32;
        out.correlacao_entrada = correlacao(&canais) as f32;
        let pre = pre_processar(&canais, sr, &eq_db, side_db);
        let previsto = ajustes.alvo_lufs - loudness::integrado(&pre, sr);
        let teto = ajustes.ganho_max_db.unwrap_or(f64::INFINITY);
        out.ganho_db = if previsto.is_finite() { previsto.min(teto) as f32 } else { 0.0 };
        out.no_teto_de_ganho = (previsto >= teto) as i32;
        *res = out;
        APURA_OK
    }));
    r.unwrap_or(APURA_ERRO_INTERNO)
}

/// Centros das 30 bandas (para a janela desenhar o gráfico).
#[no_mangle]
pub extern "C" fn apura_banda_hz(i: u32) -> f32 {
    BANDAS.get(i as usize).copied().unwrap_or(0.0) as f32
}

// --- MATCH ---------------------------------------------------------------------------------

/// MATCH: os 4 pontos que melhor desenham a curva (30 bandas, dB).
///
/// # Safety
/// `curva` tem 30 valores; `freq` e `ganho` têm espaço para 4.
#[no_mangle]
pub unsafe extern "C" fn apura_match(curva: *const f32, sr: f64, freq: *mut f32, ganho: *mut f32) -> i32 {
    let r = catch_unwind(AssertUnwindSafe(|| {
        if curva.is_null() || freq.is_null() || ganho.is_null() || !(sr > 0.0) {
            return APURA_ERRO_ARGUMENTOS;
        }
        let c = std::slice::from_raw_parts(curva, 30);
        let mut alvo = [0.0f64; 30];
        for (a, &v) in alvo.iter_mut().zip(c) {
            *a = if v.is_finite() { v as f64 } else { 0.0 };
        }
        let b = eq4::ajustar(&alvo, sr);
        for k in 0..4 {
            *freq.add(k) = b[k].freq as f32;
            *ganho.add(k) = b[k].ganho_db as f32;
        }
        APURA_OK
    }));
    r.unwrap_or(APURA_ERRO_INTERNO)
}

// --- tempo real ----------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ApuraAjustesRT {
    pub alvo_lufs: f32,
    pub teto_db: f32,
    /// 0 CLEAN, 1 VALVE, 2 PUNCH, 3 TRANSPARENT
    pub carater: i32,
    pub bandas_freq: [f32; 4],
    pub bandas_ganho: [f32; 4],
    pub passa_altas_hz: f32,
    pub width_db: f32,
    pub ganho_db: f32,
    pub tem_teto_de_ganho: i32,
    pub ganho_max_db: f32,
    pub comparar: i32,
    pub ganho_comparar_db: f32,
    pub escutando: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ApuraMedidasRT {
    pub momentaneo: f32,
    pub curto: f32,
    pub integrado: f32,
    pub pico_db: f32,
    pub limitador_db: f32,
    pub compressor_db: f32,
    pub espectro_entrada: [f32; 30],
    pub espectro_saida: [f32; 30],
    pub escutando_s: f32,
    pub lufs_entrada_escuta: f32,
    pub lufs_pre_escuta: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ApuraAprendizado {
    pub segundos: f32,
    pub lufs_entrada: f32,
    pub lufs_pre: f32,
    pub lufs_saida: f32,
    pub sm_db: f32,
    pub ganho_sugerido_db: f32,
    pub espectro_db: [f32; 30],
    pub curva_musica: [f32; 30],
    pub curva_voz: [f32; 30],
}

pub struct ApuraRTC {
    motor: ApuraRT,
    nc: usize,
}

/// Cria o motor ao vivo (carrega tudo aqui, fora da thread de áudio). Nulo se inválido.
#[no_mangle]
pub extern "C" fn apura_rt_novo(n_canais: u32, sr: f64) -> *mut ApuraRTC {
    if n_canais == 0 || n_canais > 8 || !(sr > 0.0) {
        return std::ptr::null_mut();
    }
    catch_unwind(|| Box::into_raw(Box::new(ApuraRTC { motor: ApuraRT::novo(n_canais as usize, sr), nc: n_canais as usize })))
        .unwrap_or(std::ptr::null_mut())
}

/// # Safety
/// `h` veio de `apura_rt_novo` (ou é nulo) e não é usado depois.
#[no_mangle]
pub unsafe extern "C" fn apura_rt_liberar(h: *mut ApuraRTC) {
    if !h.is_null() {
        drop(Box::from_raw(h));
    }
}

/// # Safety
/// `h` veio de `apura_rt_novo`.
#[no_mangle]
pub unsafe extern "C" fn apura_rt_latencia(h: *const ApuraRTC) -> u32 {
    if h.is_null() {
        0
    } else {
        (*h).motor.latencia() as u32
    }
}

/// # Safety
/// `h` veio de `apura_rt_novo`.
#[no_mangle]
pub unsafe extern "C" fn apura_rt_zerar_integrado(h: *mut ApuraRTC) {
    if !h.is_null() {
        (*h).motor.zerar_integrado();
    }
}

fn medidas_c(m: &MedidasRT) -> ApuraMedidasRT {
    ApuraMedidasRT {
        momentaneo: m.momentaneo as f32,
        curto: m.curto as f32,
        integrado: m.integrado as f32,
        pico_db: m.pico_db as f32,
        limitador_db: m.limitador_db as f32,
        compressor_db: m.compressor_db as f32,
        espectro_entrada: para_f32(&m.espectro_entrada),
        espectro_saida: para_f32(&m.espectro_saida),
        escutando_s: m.escutando_s as f32,
        lufs_entrada_escuta: m.lufs_entrada_escuta as f32,
        lufs_pre_escuta: m.lufs_pre_escuta as f32,
    }
}

/// Processa um bloco ao vivo. `saidas` e `entradas` têm que ser buffers diferentes.
///
/// # Safety
/// `h` veio de `apura_rt_novo`; `entradas`/`saidas` têm `n_canais` buffers de `n` amostras.
#[no_mangle]
pub unsafe extern "C" fn apura_rt_processar(
    h: *mut ApuraRTC,
    entradas: *const *const f32,
    saidas: *const *mut f32,
    n_canais: u32,
    n: u32,
    aj: *const ApuraAjustesRT,
    med: *mut ApuraMedidasRT,
) -> i32 {
    let r = catch_unwind(AssertUnwindSafe(|| {
        if h.is_null() || entradas.is_null() || saidas.is_null() || aj.is_null() || n_canais as usize != (*h).nc {
            return APURA_ERRO_ARGUMENTOS;
        }
        let a = &*aj;
        let Some(carater) = Carater::de_i32(a.carater) else { return APURA_ERRO_ARGUMENTOS };
        let ajustes = AjustesRT {
            alvo_lufs: a.alvo_lufs as f64,
            teto_db: a.teto_db as f64,
            carater,
            bandas: bandas_de(&a.bandas_freq, &a.bandas_ganho),
            passa_altas_hz: (a.passa_altas_hz as f64).max(0.0),
            width_db: a.width_db as f64,
            ganho_db: a.ganho_db as f64,
            ganho_max_db: if a.tem_teto_de_ganho != 0 { Some(a.ganho_max_db as f64) } else { None },
            comparar: a.comparar != 0,
            ganho_comparar_db: a.ganho_comparar_db as f64,
            escutando: a.escutando != 0,
        };
        let nc = n_canais as usize;
        let ent_p = std::slice::from_raw_parts(entradas, nc);
        let sai_p = std::slice::from_raw_parts(saidas, nc);
        if ent_p.iter().any(|p| p.is_null()) || sai_p.iter().any(|p| p.is_null()) {
            return APURA_ERRO_ARGUMENTOS;
        }
        // entrada e saída têm que ser buffers diferentes (o plugin copia a entrada antes)
        let ent: Vec<&[f32]> = ent_p.iter().map(|&p| std::slice::from_raw_parts(p, n as usize)).collect();
        let mut sai: Vec<&mut [f32]> = sai_p.iter().map(|&p| std::slice::from_raw_parts_mut(p, n as usize)).collect();
        let mut m = MedidasRT::default();
        (*h).motor.processar(&ent, &mut sai, &ajustes, &mut m);
        if !med.is_null() {
            *med = medidas_c(&m);
        }
        APURA_OK
    }));
    r.unwrap_or(APURA_ERRO_INTERNO)
}

/// O que o último LISTEN aprendeu. 0 = há aprendizado; 1 = ainda não.
///
/// # Safety
/// `h` veio de `apura_rt_novo`; `saida` aponta para um `ApuraAprendizado`.
#[no_mangle]
pub unsafe extern "C" fn apura_rt_aprendizado(h: *const ApuraRTC, saida: *mut ApuraAprendizado) -> i32 {
    if h.is_null() || saida.is_null() {
        return APURA_ERRO_ARGUMENTOS;
    }
    match (*h).motor.aprendizado() {
        None => 1,
        Some(a) => {
            *saida = ApuraAprendizado {
                segundos: a.segundos as f32,
                lufs_entrada: a.lufs_entrada as f32,
                lufs_pre: a.lufs_pre as f32,
                lufs_saida: a.lufs_saida as f32,
                sm_db: a.sm_db as f32,
                ganho_sugerido_db: a.ganho_sugerido_db as f32,
                espectro_db: para_f32(&a.espectro_db),
                curva_musica: para_f32(&a.curva_musica),
                curva_voz: para_f32(&a.curva_voz),
            };
            APURA_OK
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn ajustes_tv() -> ApuraAjustes {
        ApuraAjustes {
            alvo_lufs: -23.0,
            teto_db: -1.0,
            curva: 1,
            eq_quantidade: 0.0,
            passa_altas_hz: 0.0,
            width_auto: 0,
            width_db: 0.0,
            tem_teto_de_ganho: 1,
            ganho_max_db: 15.0,
            carater: 3,
            comparar: 0,
            medir_faixa: 1,
            eq_bandas: 1,
            bandas_freq: [80.0, 300.0, 2500.0, 10000.0],
            bandas_ganho: [0.0; 4],
        }
    }

    #[test]
    fn processa_no_lugar_pela_interface_em_c() {
        let mut l: Vec<f32> = (0..48000 * 5).map(|i| (0.05 * (i as f32 * 0.01).sin()) * ((i / 9000) % 3) as f32).collect();
        let mut r = l.clone();
        let ent = [l.as_ptr(), r.as_ptr()];
        let sai = [l.as_mut_ptr(), r.as_mut_ptr()];
        let mut res = ApuraResultado::default();
        let cod = unsafe { apura_processar(ent.as_ptr(), sai.as_ptr(), 2, 48000 * 5, 48000.0, &ajustes_tv(), &mut res) };
        assert_eq!(cod, APURA_OK);
        assert!((res.lufs_saida + 23.0).abs() < 0.06, "{}", res.lufs_saida);
        assert!(res.tp_saida <= -0.95);
        assert!(res.lra_saida.is_finite());
    }

    #[test]
    fn tempo_real_pela_interface_em_c() {
        let h = apura_rt_novo(2, 48000.0);
        assert!(!h.is_null());
        assert_eq!(unsafe { apura_rt_latencia(h) }, 107);
        let l: Vec<f32> = (0..4800).map(|i| 0.3 * (i as f32 * 0.03).sin()).collect();
        let r = l.clone();
        let (mut sl, mut sr_) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
        let ent = [l.as_ptr(), r.as_ptr()];
        let sai = [sl.as_mut_ptr(), sr_.as_mut_ptr()];
        let aj = ApuraAjustesRT {
            alvo_lufs: -23.0, teto_db: -1.0, carater: 0, bandas_freq: [80.0, 300.0, 2500.0, 10000.0], bandas_ganho: [0.0, -2.0, 1.0, 0.0],
            passa_altas_hz: 0.0, width_db: 0.0, ganho_db: 3.0, tem_teto_de_ganho: 1, ganho_max_db: 15.0, comparar: 0,
            ganho_comparar_db: 0.0, escutando: 1,
        };
        let mut m: ApuraMedidasRT = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { apura_rt_processar(h, ent.as_ptr(), sai.as_ptr(), 2, 4800, &aj, &mut m) }, APURA_OK);
        assert!(m.escutando_s > 0.09);
        let mut a: ApuraAprendizado = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { apura_rt_aprendizado(h, &mut a) }, 1);
        unsafe { apura_rt_liberar(h) };
        let (mut f, mut g) = ([0.0f32; 4], [0.0f32; 4]);
        assert_eq!(unsafe { apura_match([0.0f32; 30].as_ptr(), 48000.0, f.as_mut_ptr(), g.as_mut_ptr()) }, APURA_OK);
        assert!(g.iter().all(|v| v.abs() < 0.05));
    }

    #[test]
    fn recusa_argumentos_invalidos() {
        let mut res = ApuraResultado::default();
        let mut aj = ajustes_tv();
        aj.carater = 9;
        let x = vec![0.0f32; 100];
        let ent = [x.as_ptr()];
        assert_eq!(unsafe { apura_analisar(ent.as_ptr(), 1, 100, 48000.0, &aj, &mut res) }, APURA_ERRO_ARGUMENTOS);
        assert_eq!(unsafe { apura_analisar(std::ptr::null(), 1, 100, 48000.0, &ajustes_tv(), &mut res) }, APURA_ERRO_ARGUMENTOS);
    }
}
