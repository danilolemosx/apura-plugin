//! Análise do clip inteiro: espectro em bandas de 1/3 de oitava, curva de EQ
//! (música: a do Mastering Assistant medida; vídeo: a de voz) e largura do estéreo.

use crate::modos::{Ajustes, TipoEq};
use crate::util::{interp, interp_log};
use realfft::RealFftPlanner;

/// Centros das bandas de 1/3 de oitava.
pub const BANDAS: [f64; 30] = [
    25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0, 200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0,
    1250.0, 1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0, 10000.0, 12500.0, 16000.0, 20000.0,
];

/// Curva-alvo do Mastering Assistant, medida em ruído rosa plano (dB relativos a 1 kHz).
fn alvo_musica(f: f64) -> f64 {
    interp_log(
        f,
        &[25.0, 30.0, 60.0, 100.0, 200.0, 400.0, 600.0, 1000.0, 2000.0, 3500.0, 5000.0, 7000.0, 10000.0, 15000.0, 20000.0],
        &[0.0, -0.2, 1.1, 1.2, 0.7, -0.4, -0.6, 0.0, -0.3, -1.3, -2.1, -3.1, -2.9, -0.8, 0.0],
    )
}

/// Espectro médio de longo prazo da fala (LTASS, Byrne et al. 1994, aproximado),
/// potência por banda de 1/3 de oitava, dB relativos a 1 kHz.
fn referencia_voz(f: f64) -> f64 {
    interp_log(
        f,
        &[25.0, 63.0, 80.0, 100.0, 125.0, 200.0, 250.0, 400.0, 500.0, 800.0, 1000.0, 2000.0, 4000.0, 8000.0, 10000.0, 20000.0],
        &[-25.0, -12.0, -6.0, -3.0, 1.0, 6.0, 7.0, 6.0, 5.0, 1.0, 0.0, -5.0, -8.0, -11.0, -13.0, -20.0],
    )
}

// Auto EQ de música (medido no Logic): fração do desvio corrigida e limites da curva.
const EQ_K: f64 = 0.4;
const EQ_MIN: f64 = -4.5;
const EQ_MAX: f64 = 2.6;
// EQ de voz (desenho nosso)
const VOZ_K: f64 = 0.3;
const VOZ_MIN: f64 = -3.0;
const VOZ_MAX: f64 = 2.0;
pub const VOZ_PASSA_ALTAS_HZ: f64 = 60.0;
const VOZ_SEM_REFORCO_ABAIXO_HZ: f64 = 150.0;
// Largura (medido no Logic): ganho no Side = limita(k · (alvo − S/M), ±limite)
const WIDTH_ALVO: f64 = -8.4;
const WIDTH_K: f64 = 0.7;
const WIDTH_LIMITE: f64 = 6.0;

const NPERSEG: usize = 16384;

#[derive(Clone, Debug)]
pub struct Analise {
    /// potência por banda do clip (soma dos canais), dB
    pub espectro_db: [f64; 30],
    /// curva de música inteira (alvo + correção), dB por banda
    pub curva_musica: [f64; 30],
    /// correção de voz (sem o passa-altas), dB por banda
    pub curva_voz: [f64; 30],
    /// ganho do Side sugerido pela análise, dB (0 em mono)
    pub side_sugerido_db: f64,
    /// razão Side/Mid da entrada, dB
    pub sm_entrada: f64,
    pub canais: usize,
}

impl Analise {
    /// Curva de EQ que os ajustes pedem: a curva escolhida × AUTO EQ + passa-altas.
    pub fn eq_final(&self, aj: &Ajustes) -> [f64; 30] {
        let base = match aj.curva {
            TipoEq::Musica => &self.curva_musica,
            TipoEq::Voz => &self.curva_voz,
        };
        let mut c = [0.0; 30];
        for (i, &b) in BANDAS.iter().enumerate() {
            c[i] = aj.eq_quantidade * base[i];
            if aj.passa_altas_hz > 0.0 {
                c[i] += passa_altas_db(b, aj.passa_altas_hz);
            }
        }
        c
    }

    /// Ganho do Side que os ajustes pedem (0 em mono).
    pub fn side_final(&self, aj: &Ajustes) -> f64 {
        if self.canais != 2 {
            return 0.0;
        }
        aj.side_db.unwrap_or(self.side_sugerido_db)
    }
}

/// `scipy.signal.welch(x, sr, nperseg=16384)`: Hann periódica, 50% de sobreposição,
/// média tirada de cada segmento, densidade unilateral.
fn welch(x: &[f64], sr: f64) -> (Vec<f64>, Vec<f64>) {
    let nperseg = NPERSEG.min(x.len()).max(2);
    let sobrepos = nperseg / 2;
    let passo = nperseg - sobrepos;
    let n_seg = if x.len() >= sobrepos { (x.len() - sobrepos) / passo } else { 0 };
    let janela: Vec<f64> =
        (0..nperseg).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / nperseg as f64).cos()).collect();
    let escala = 1.0 / (sr * janela.iter().map(|w| w * w).sum::<f64>());
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(nperseg);
    let mut entrada = fft.make_input_vec();
    let mut saida = fft.make_output_vec();
    let mut psd = vec![0.0; nperseg / 2 + 1];
    for s in 0..n_seg {
        let seg = &x[s * passo..s * passo + nperseg];
        let media = seg.iter().sum::<f64>() / nperseg as f64;
        for i in 0..nperseg {
            entrada[i] = (seg[i] - media) * janela[i];
        }
        fft.process(&mut entrada, &mut saida).expect("fft");
        for (p, c) in psd.iter_mut().zip(&saida) {
            *p += c.norm_sqr() * escala;
        }
    }
    let ultimo = psd.len() - 1;
    for (k, p) in psd.iter_mut().enumerate() {
        *p /= n_seg.max(1) as f64;
        // unilateral: dobra tudo menos DC (e Nyquist, quando nperseg é par)
        if k > 0 && (k < ultimo || nperseg % 2 == 1) {
            *p *= 2.0;
        }
    }
    let freqs = (0..psd.len()).map(|k| k as f64 * sr / nperseg as f64).collect();
    (freqs, psd)
}

/// Potência por banda de 1/3 de oitava da soma dos canais, em dB (ruído rosa = plano).
pub fn espectro_bandas(canais: &[Vec<f64>], sr: f64) -> [f64; 30] {
    let n = canais.first().map_or(0, |c| c.len());
    let soma: Vec<f64> = (0..n).map(|i| canais.iter().map(|c| c[i]).sum()).collect();
    let (f, p) = welch(&soma, sr);
    let mut db = [0.0; 30];
    for (b, &fc) in BANDAS.iter().enumerate() {
        let (lo, hi) = (fc / 2f64.powf(1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
        let s: f64 = f.iter().zip(&p).filter(|(&fi, _)| fi >= lo && fi < hi).map(|(_, &pi)| pi).sum();
        db[b] = 10.0 * (s + 1e-20).log10();
    }
    db
}

/// Peso da correção no modo música: 1 entre 50 Hz e 8 kHz, cai a 0 em 25 Hz e 16 kHz.
fn peso_pontas(f: f64) -> f64 {
    let lf = ((f / 25.0).log2() / (50f64 / 25.0).log2()).clamp(0.0, 1.0);
    let hf = ((16000.0 / f).log2() / (16000f64 / 8000.0).log2()).clamp(0.0, 1.0);
    lf * hf
}

fn media_das_bandas(v: &[f64; 30], lo: f64, hi: f64) -> f64 {
    let sel: Vec<f64> = BANDAS.iter().zip(v).filter(|(&b, _)| b >= lo && b <= hi).map(|(_, &x)| x).collect();
    sel.iter().sum::<f64>() / sel.len() as f64
}

pub fn curva_musica(db: &[f64; 30]) -> [f64; 30] {
    let media = media_das_bandas(db, 40.0, 12500.0);
    let mut c = [0.0; 30];
    for (i, &b) in BANDAS.iter().enumerate() {
        // a curva-alvo vale inteira; só a correção some nas pontas
        c[i] = (alvo_musica(b) - EQ_K * (db[i] - media) * peso_pontas(b)).clamp(EQ_MIN, EQ_MAX);
    }
    c
}

/// Passa-altas de 2ª ordem (−12 dB/oitava) em dB.
pub fn passa_altas_db(f: f64, fc: f64) -> f64 {
    10.0 * (1.0 / (1.0 + (fc / f).powi(4))).log10()
}

/// Correção de voz, sem o passa-altas (que é um ajuste à parte).
pub fn curva_voz(db: &[f64; 30]) -> [f64; 30] {
    let mut desvio = [0.0; 30];
    for (i, &b) in BANDAS.iter().enumerate() {
        desvio[i] = db[i] - referencia_voz(b);
    }
    let media = media_das_bandas(&desvio, 100.0, 10000.0);
    let mut c = [0.0; 30];
    for (i, &b) in BANDAS.iter().enumerate() {
        let peso = ((b / 63.0).log2() / (100f64 / 63.0).log2()).clamp(0.0, 1.0) * (16000.0 / b).log2().clamp(0.0, 1.0);
        let mut correcao = (-VOZ_K * (desvio[i] - media) * peso).clamp(VOZ_MIN, VOZ_MAX);
        if b < VOZ_SEM_REFORCO_ABAIXO_HZ {
            correcao = correcao.min(0.0);
        }
        c[i] = correcao;
    }
    c
}

/// Razão Side/Mid em dB (só estéreo).
pub fn side_mid_db(canais: &[Vec<f64>]) -> f64 {
    if canais.len() != 2 {
        return -200.0;
    }
    let (mut sm, mut mm) = (0.0, 0.0);
    for (l, r) in canais[0].iter().zip(&canais[1]) {
        let (m, s) = (l + r, l - r);
        mm += m * m;
        sm += s * s;
    }
    if mm == 0.0 {
        return -200.0;
    }
    10.0 * (sm / mm + 1e-20).log10()
}

pub fn analisar(canais: &[Vec<f64>], sr: f64) -> Analise {
    let espectro_db = espectro_bandas(canais, sr);
    let sm_entrada = side_mid_db(canais);
    let side_sugerido_db =
        if canais.len() == 2 { (WIDTH_K * (WIDTH_ALVO - sm_entrada)).clamp(-WIDTH_LIMITE, WIDTH_LIMITE) } else { 0.0 };
    Analise {
        curva_musica: curva_musica(&espectro_db),
        curva_voz: curva_voz(&espectro_db),
        espectro_db,
        side_sugerido_db,
        sm_entrada,
        canais: canais.len(),
    }
}

/// Valor da curva numa frequência qualquer (interpolação em log, presa nas pontas).
pub fn eq_em(eq_db: &[f64; 30], f: f64) -> f64 {
    let xp: Vec<f64> = BANDAS.iter().map(|b| b.ln()).collect();
    interp(f.max(1.0).ln(), &xp, eq_db)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn passa_altas_tem_menos_3_db_no_corte() {
        assert!((passa_altas_db(60.0, 60.0) + 3.0103).abs() < 1e-3);
    }

    #[test]
    fn mono_nao_mexe_na_largura() {
        let x = vec![(0..48000).map(|i| ((i * 7919) % 1000) as f64 / 1000.0 - 0.5).collect::<Vec<_>>()];
        let a = analisar(&x, 48000.0);
        let aj = Ajustes::de_modo(crate::Modo::Musica, crate::Carater::Clean);
        assert_eq!(a.side_final(&aj), 0.0);
    }
}
