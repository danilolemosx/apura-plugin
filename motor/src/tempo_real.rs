//! Motor ao vivo (insert no master): a mesma cadeia do clip inteiro, amostra a amostra.
//!
//! EQ de 4 pontos → largura → ganho → compressor (DENSE) → saturação (WARM) → limitador
//! true peak. O limitador olha 2·lookahead + 11 amostras à frente (o filtro de pico
//! verdadeiro precisa de 11), e esse é o atraso que o plugin informa ao programa.
//! Com o mesmo ganho, a saída é igual à de `cadeia` (testado abaixo).
//!
//! LISTEN: enquanto `escutando`, mede o loudness da entrada, do sinal antes do ganho e da
//! saída, e o espectro médio da entrada. Ao parar, sugere o ganho e as curvas do MATCH.
//! Nada aqui aloca memória durante o processamento (exceto, raramente, a lista de blocos
//! do LISTEN passar de 2 horas).

use crate::analise::{curva_musica, curva_voz, BANDAS};
use crate::eq4::{coeficientes, Banda, Coef, Estado, PADRAO};
use crate::loudness::{filtro_k_itu, integrado_de_blocos};
use crate::modos::{Carater, CARATERES, COMP_ATAQUE_MS, COMP_RELEASE_MS, COMP_RMS_MS, LOOKAHEAD_MS, MODOS};
use crate::true_peak::{filtro as filtro_tp, ATRASO as ATRASO_TP, FATOR};
use crate::util::db_para_lin;
use crate::Modo;
use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};
use std::collections::VecDeque;
use std::sync::Arc;

/// Amostras à frente que o filtro de pico verdadeiro precisa.
const ANTES_TP: usize = 11;

#[derive(Clone, Copy, Debug)]
pub struct AjustesRT {
    pub alvo_lufs: f64,
    pub teto_db: f64,
    pub carater: Carater,
    pub bandas: [Banda; 4],
    pub passa_altas_hz: f64,
    pub width_db: f64,
    pub ganho_db: f64,
    pub ganho_max_db: Option<f64>,
    /// BYPASS · SAME VOLUME: o original (com o mesmo atraso) × `ganho_comparar_db`
    pub comparar: bool,
    pub ganho_comparar_db: f64,
    pub escutando: bool,
}

impl Default for AjustesRT {
    fn default() -> Self {
        AjustesRT {
            alvo_lufs: -23.0,
            teto_db: -1.0,
            carater: Carater::Transparent,
            bandas: PADRAO,
            passa_altas_hz: 0.0,
            width_db: 0.0,
            ganho_db: 0.0,
            ganho_max_db: Some(15.0),
            comparar: false,
            ganho_comparar_db: 0.0,
            escutando: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MedidasRT {
    /// loudness da saída: 400 ms, 3 s e integrado desde o último LISTEN (LUFS; −∞ sem som)
    pub momentaneo: f64,
    pub curto: f64,
    pub integrado: f64,
    /// pico (amostra) da saída neste bloco, dBFS
    pub pico_db: f64,
    /// menor ganho do limitador e do compressor neste bloco, dB (≤ 0)
    pub limitador_db: f64,
    pub compressor_db: f64,
    /// espectro ao vivo da entrada e da saída (densidade por banda de 1/3 de oitava, dB)
    pub espectro_entrada: [f64; 30],
    pub espectro_saida: [f64; 30],
    /// LISTEN em andamento
    pub escutando_s: f64,
    pub lufs_entrada_escuta: f64,
    pub lufs_pre_escuta: f64,
}

/// O que o LISTEN aprendeu.
#[derive(Clone, Copy, Debug)]
pub struct Aprendizado {
    pub segundos: f64,
    pub lufs_entrada: f64,
    /// antes do ganho (depois da EQ e da largura)
    pub lufs_pre: f64,
    pub lufs_saida: f64,
    pub espectro_db: [f64; 30],
    pub curva_musica: [f64; 30],
    pub curva_voz: [f64; 30],
    pub sm_db: f64,
    pub ganho_sugerido_db: f64,
}

// --- medidor de loudness ao vivo --------------------------------------------------------

struct Medidor {
    k: [Coef; 2],
    estados: Vec<[Estado; 2]>,
    hop: usize,
    n_hop: usize,
    soma_hop: f64,
    hops: [f64; 30],
    i_hop: usize,
    n_hops: usize,
    blocos: Vec<f64>,
}

impl Medidor {
    fn novo(nc: usize, sr: f64) -> Self {
        Medidor {
            k: filtro_k_itu(sr),
            estados: vec![[Estado::default(); 2]; nc],
            hop: (0.1 * sr).round() as usize,
            n_hop: 0,
            soma_hop: 0.0,
            hops: [0.0; 30],
            i_hop: 0,
            n_hops: 0,
            blocos: Vec::with_capacity(72_000),
        }
    }

    #[inline]
    fn medir(&mut self, quadro: &[f64]) {
        for (c, &x) in quadro.iter().enumerate() {
            let [e0, e1] = &mut self.estados[c];
            let y = e1.filtrar(&self.k[1], e0.filtrar(&self.k[0], x));
            self.soma_hop += y * y;
        }
        self.n_hop += 1;
        if self.n_hop == self.hop {
            self.hops[self.i_hop] = self.soma_hop;
            self.i_hop = (self.i_hop + 1) % 30;
            self.n_hops += 1;
            self.n_hop = 0;
            self.soma_hop = 0.0;
            if self.n_hops >= 4 {
                self.blocos.push(self.ultimos(4) / (4 * self.hop) as f64);
            }
        }
    }

    fn ultimos(&self, n: usize) -> f64 {
        (1..=n).map(|k| self.hops[(self.i_hop + 30 - k) % 30]).sum()
    }

    fn janela(&self, n: usize) -> f64 {
        let n = n.min(self.n_hops);
        if n < 4 {
            return f64::NEG_INFINITY;
        }
        -0.691 + 10.0 * (self.ultimos(n) / (n * self.hop) as f64).log10()
    }

    fn integrado(&self) -> f64 {
        integrado_de_blocos(&self.blocos)
    }

    fn zerar(&mut self) {
        self.blocos.clear();
    }
}

// --- espectro (Welch, como a análise do clip inteiro) -----------------------------------

struct Analisador {
    buf: Vec<f64>,
    pos: usize,
    cheio: bool,
    passo: usize,
    conta: usize,
    fft: Arc<dyn RealToComplex<f64>>,
    entrada: Vec<f64>,
    saida: Vec<Complex<f64>>,
    scratch: Vec<Complex<f64>>,
    janela: Vec<f64>,
    escala: f64,
    faixas: [(usize, usize); 30],
    /// última leitura (dB) e a média acumulada (linear)
    pub ultimo: [f64; 30],
    soma: [f64; 30],
    n_soma: usize,
}

impl Analisador {
    fn novo(n: usize, passo: usize, sr: f64) -> Self {
        let fft = RealFftPlanner::<f64>::new().plan_fft_forward(n);
        let janela: Vec<f64> = (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()).collect();
        let escala = 1.0 / (sr * janela.iter().map(|w| w * w).sum::<f64>());
        let mut faixas = [(0, 0); 30];
        for (b, &fc) in BANDAS.iter().enumerate() {
            let (lo, hi) = (fc / 2f64.powf(1.0 / 6.0), fc * 2f64.powf(1.0 / 6.0));
            let k0 = (0..=n / 2).find(|&k| k as f64 * sr / n as f64 >= lo).unwrap_or(n / 2 + 1);
            let k1 = (0..=n / 2).find(|&k| k as f64 * sr / n as f64 >= hi).unwrap_or(n / 2 + 1);
            faixas[b] = (k0, k1);
        }
        Analisador {
            buf: vec![0.0; n],
            pos: 0,
            cheio: false,
            passo,
            conta: 0,
            entrada: fft.make_input_vec(),
            saida: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            janela,
            escala,
            faixas,
            ultimo: [-200.0; 30],
            soma: [0.0; 30],
            n_soma: 0,
        }
    }

    /// Devolve true quando calculou um quadro novo (`ultimo`).
    #[inline]
    fn empurrar(&mut self, x: f64, acumular: bool, queda_db: f64) -> bool {
        let n = self.buf.len();
        self.buf[self.pos] = x;
        self.pos = (self.pos + 1) % n;
        if self.pos == 0 {
            self.cheio = true;
        }
        self.conta += 1;
        if self.conta < self.passo || !self.cheio {
            return false;
        }
        self.conta = 0;
        let media = self.buf.iter().sum::<f64>() / n as f64;
        for i in 0..n {
            self.entrada[i] = (self.buf[(self.pos + i) % n] - media) * self.janela[i];
        }
        if self.fft.process_with_scratch(&mut self.entrada, &mut self.saida, &mut self.scratch).is_err() {
            return false;
        }
        let ultimo_bin = n / 2;
        for (b, &(k0, k1)) in self.faixas.iter().enumerate() {
            let mut p = 0.0;
            for k in k0..k1.min(ultimo_bin + 1) {
                let dobro = if k == 0 || k == ultimo_bin { 1.0 } else { 2.0 };
                p += self.saida[k].norm_sqr() * self.escala * dobro;
            }
            if acumular {
                self.soma[b] += p;
            }
            let db = 10.0 * (p + 1e-20).log10();
            self.ultimo[b] = if db > self.ultimo[b] - queda_db { db } else { self.ultimo[b] - queda_db };
        }
        if acumular {
            self.n_soma += 1;
        }
        true
    }

    fn media_db(&self) -> [f64; 30] {
        let mut o = [-200.0; 30];
        if self.n_soma > 0 {
            for b in 0..30 {
                o[b] = 10.0 * (self.soma[b] / self.n_soma as f64 + 1e-20).log10();
            }
        }
        o
    }

    fn zerar_media(&mut self) {
        self.soma = [0.0; 30];
        self.n_soma = 0;
    }
}

/// Curva estática do compressor (dB de redução para um nível), a mesma de `CompressorRT`.
fn reducao_estatica(nivel: f64, limiar: f64, razao: f64, joelho: f64) -> f64 {
    let acima = nivel - limiar;
    if 2.0 * acima < -joelho {
        0.0
    } else if 2.0 * acima > joelho {
        acima * (1.0 / razao - 1.0)
    } else {
        (1.0 / razao - 1.0) * (acima + joelho / 2.0).powi(2) / (2.0 * joelho)
    }
}

// --- compressor e limitador amostra a amostra (iguais aos de `dinamica`) -----------------

struct CompressorRT {
    janela: Vec<f64>,
    pos: usize,
    cheio: usize,
    soma: f64,
    desde_recontar: usize,
    estado: f64,
    a_at: f64,
    a_rel: f64,
}

impl CompressorRT {
    fn novo(sr: f64) -> Self {
        CompressorRT {
            janela: vec![0.0; ((COMP_RMS_MS * sr / 1000.0) as usize).max(1)],
            pos: 0,
            cheio: 0,
            soma: 0.0,
            desde_recontar: 0,
            estado: 0.0,
            a_at: (-1.0 / (COMP_ATAQUE_MS * sr / 1000.0)).exp(),
            a_rel: (-1.0 / (COMP_RELEASE_MS * sr / 1000.0)).exp(),
        }
    }

    /// Redução (dB) para a média dos quadrados desta amostra.
    #[inline]
    fn reducao(&mut self, quadrado: f64, limiar: f64, razao: f64, joelho: f64) -> f64 {
        let nivel = self.nivel(quadrado);
        let alvo = reducao_estatica(nivel, limiar, razao, joelho);
        let coef = if alvo < self.estado { self.a_at } else { self.a_rel };
        self.estado = coef * self.estado + (1.0 - coef) * alvo;
        if !self.estado.is_finite() {
            self.estado = 0.0;
        }
        self.estado
    }

    /// Nível que o compressor enxerga: RMS dos últimos 5 ms, em dB (+3: escala de pico de senoide).
    #[inline]
    fn nivel(&mut self, quadrado: f64) -> f64 {
        let n = self.janela.len();
        if self.cheio == n {
            self.soma -= self.janela[self.pos];
        } else {
            self.cheio += 1;
        }
        self.janela[self.pos] = quadrado;
        self.soma += quadrado;
        self.pos = (self.pos + 1) % n;
        self.desde_recontar += 1;
        if self.desde_recontar >= 1 << 16 {
            // a soma corrida acumula erro de arredondamento: recalcula de vez em quando
            self.soma = self.janela[..self.cheio.min(n)].iter().sum();
            self.desde_recontar = 0;
        }
        // a soma corrida pode ficar um tiquinho negativa depois de um trecho forte: nunca abaixo de 0
        10.0 * ((self.soma / self.cheio as f64).max(0.0) + 1e-20).log10() + 3.0
    }

    /// Recomeça do zero (depois de um valor inválido na entrada).
    fn zerar(&mut self) {
        self.janela.iter_mut().for_each(|v| *v = 0.0);
        self.soma = 0.0;
        self.cheio = 0;
        self.estado = 0.0;
    }
}

struct LimitadorRT {
    la: usize,
    h: Vec<f64>,
    hist: Vec<Vec<f64>>,
    mascara: usize,
    n: usize,
    req: VecDeque<(usize, f64)>,
    g: Vec<f64>,
    g_soma: f64,
    g_n: usize,
    estado: f64,
}

impl LimitadorRT {
    fn novo(nc: usize, sr: f64) -> Self {
        let la = (LOOKAHEAD_MS * sr / 1000.0) as usize;
        let tamanho = (ANTES_TP + 2 * la + 64).next_power_of_two();
        LimitadorRT {
            la,
            h: filtro_tp(),
            hist: vec![vec![0.0; tamanho]; nc],
            mascara: tamanho - 1,
            n: 0,
            req: VecDeque::with_capacity(la + 4),
            g: vec![1.0; la + 1],
            g_soma: 0.0,
            g_n: 0,
            estado: 1.0,
        }
    }

    fn latencia(&self) -> usize {
        ANTES_TP + 2 * self.la
    }

    /// Recebe uma amostra (todos os canais) e escreve em `saida` a amostra de `latencia()`
    /// atrás, já limitada. Devolve o ganho aplicado nela (1 enquanto ainda enche).
    #[inline]
    fn empurrar(&mut self, quadro: &[f64], saida: &mut [f64], teto: f64, a_rel: f64) -> f64 {
        let n = self.n;
        for (c, &x) in quadro.iter().enumerate() {
            self.hist[c][n & self.mascara] = x;
        }
        self.n += 1;
        saida.iter_mut().for_each(|v| *v = 0.0);
        if n < ANTES_TP {
            return 1.0;
        }
        // pico verdadeiro da amostra s (4 sub-amostras, todos os canais)
        let s = n - ANTES_TP;
        let mut pico = 0.0f64;
        for hc in &self.hist {
            for j in 0..FATOR {
                let base = ATRASO_TP + FATOR * s + j;
                let mut acc = 0.0;
                let mut i = base % FATOR;
                while i < self.h.len() {
                    if i <= base {
                        acc += self.h[i] * hc[((base - i) / FATOR) & self.mascara];
                    }
                    i += FATOR;
                }
                pico = pico.max(acc.abs());
            }
        }
        let r = (teto / pico.max(1e-12)).min(1.0);
        // mínimo da janela [i, i + la]
        while let Some(&(_, v)) = self.req.back() {
            if v >= r {
                self.req.pop_back();
            } else {
                break;
            }
        }
        self.req.push_back((s, r));
        if s < self.la {
            return 1.0;
        }
        let i = s - self.la;
        while let Some(&(k, _)) = self.req.front() {
            if k < i {
                self.req.pop_front();
            } else {
                break;
            }
        }
        let r2 = self.req.front().map_or(1.0, |&(_, v)| v);
        self.estado = if r2 < self.estado { r2 } else { a_rel * self.estado + (1.0 - a_rel) * r2 };
        let m = self.la + 1;
        let pos = i % m;
        if self.g_n >= m {
            self.g_soma -= self.g[pos];
        }
        self.g[pos] = self.estado;
        self.g_soma += self.estado;
        self.g_n += 1;
        if self.g_n % 4096 == 0 {
            self.g_soma = self.g.iter().sum();
        }
        if i < self.la {
            return 1.0;
        }
        // a descida suavizada: média das próximas la + 1, sem deixar subir
        let j = i - self.la;
        let gf = self.g[j % m].min(self.g_soma / m as f64);
        for (c, v) in saida.iter_mut().enumerate() {
            *v = self.hist[c][j & self.mascara] * gf;
        }
        gf
    }
}

// --- cópias silenciosas da cadeia, durante o LISTEN -----------------------------------------

/// Ganhos fixos da bateria (dB aplicados ao sinal antes do ganho).
const GANHOS_SOMBRA: [f64; 12] = [-16.0, -12.0, -8.0, -4.0, 0.0, 4.0, 8.0, 12.0, 16.0, 20.0, 24.0, 28.0];

/// Uma cópia da dinâmica (compressor, saturação, limitador) com um ganho fixo, que não toca
/// na saída e só mede o loudness do que sairia.
struct CadeiaSombra {
    ganho_db: f64,
    ganho: f64,
    comp: CompressorRT,
    lim: LimitadorRT,
    med: Medidor,
    quadro: Vec<f64>,
    saida: Vec<f64>,
}

/// A bateria: o programa inteiro passa por todas as cópias, então no fim o LISTEN sabe quanto
/// a dinâmica muda o loudness em cada ganho, com o mesmo conteúdo.
struct Sombra {
    cadeias: Vec<CadeiaSombra>,
}

impl Sombra {
    fn novo(nc: usize, sr: f64) -> Self {
        Sombra {
            cadeias: GANHOS_SOMBRA
                .iter()
                .map(|&g| CadeiaSombra {
                    ganho_db: g,
                    ganho: db_para_lin(g),
                    comp: CompressorRT::novo(sr),
                    lim: LimitadorRT::novo(nc, sr),
                    med: Medidor::novo(nc, sr),
                    quadro: vec![0.0; nc],
                    saida: vec![0.0; nc],
                })
                .collect(),
        }
    }

    #[inline]
    fn amostra(&mut self, pre: &[f64], cfg: &crate::modos::ConfigCarater, limiar: Option<f64>, teto: f64, a_rel: f64) {
        for c in self.cadeias.iter_mut() {
            for (q, &p) in c.quadro.iter_mut().zip(pre) {
                *q = p * c.ganho;
            }
            if let (Some(k), Some(limiar)) = (&cfg.compressor, limiar) {
                let q = c.quadro.iter().map(|v| v * v).sum::<f64>() / c.quadro.len() as f64;
                let red = c.comp.reducao(q, limiar, k.razao, k.joelho_db);
                let gr = 10f64.powf(red / 20.0);
                c.quadro.iter_mut().for_each(|v| *v *= gr);
            }
            if cfg.saturacao > 0.0 {
                let d = cfg.saturacao;
                c.quadro.iter_mut().for_each(|v| *v = (d * 3.0 * *v).tanh() / (d * 3.0));
            }
            c.lim.empurrar(&c.quadro, &mut c.saida, teto, a_rel);
            c.med.medir(&c.saida);
        }
    }

    /// (ganho, quanto a dinâmica muda o loudness nesse ganho), para quem já tem medida.
    fn efeitos(&self, lufs_pre: f64) -> Vec<(f64, f64)> {
        self.cadeias
            .iter()
            .filter_map(|c| {
                let l = c.med.integrado();
                (l.is_finite() && lufs_pre.is_finite()).then_some((c.ganho_db, l - (lufs_pre + c.ganho_db)))
            })
            .collect()
    }
}

// --- o motor ------------------------------------------------------------------------------

pub struct ApuraRT {
    sr: f64,
    nc: usize,
    eq_atual: Option<([Banda; 4], f64)>,
    coefs: [Coef; 5],
    eq_est: Vec<[Estado; 5]>,
    ganho_lin: f64,
    a_ganho: f64,
    primeiro: bool,
    comp: CompressorRT,
    lim: LimitadorRT,
    orig: Vec<Vec<f64>>,
    orig_mascara: usize,
    n_orig: usize,
    med_entrada: Medidor,
    med_pre: Medidor,
    med_saida: Medidor,
    an_entrada: Analisador,
    an_saida: Analisador,
    an_aprender: Analisador,
    escutando: bool,
    escuta_amostras: usize,
    sombra: Sombra,
    sm_s: f64,
    sm_m: f64,
    aprendizado: Option<Aprendizado>,
    integrado_cache: (f64, f64, f64),
    desde_integrado: usize,
    quadro: Vec<f64>,
    saida: Vec<f64>,
}

impl ApuraRT {
    pub fn novo(nc: usize, sr: f64) -> ApuraRT {
        let lim = LimitadorRT::novo(nc, sr);
        let tam_orig = (lim.latencia() + 2).next_power_of_two();
        ApuraRT {
            sr,
            nc,
            eq_atual: None,
            coefs: [Coef::PLANO; 5],
            eq_est: vec![[Estado::default(); 5]; nc],
            ganho_lin: 1.0,
            a_ganho: 1.0 - (-1.0 / (0.03 * sr)).exp(),
            primeiro: true,
            comp: CompressorRT::novo(sr),
            lim,
            orig: vec![vec![0.0; tam_orig]; nc],
            orig_mascara: tam_orig - 1,
            n_orig: 0,
            med_entrada: Medidor::novo(nc, sr),
            med_pre: Medidor::novo(nc, sr),
            med_saida: Medidor::novo(nc, sr),
            an_entrada: Analisador::novo(4096, (0.05 * sr) as usize, sr),
            an_saida: Analisador::novo(4096, (0.05 * sr) as usize, sr),
            an_aprender: Analisador::novo(16384, 8192, sr),
            escutando: false,
            escuta_amostras: 0,
            sombra: Sombra::novo(nc, sr),
            sm_s: 0.0,
            sm_m: 0.0,
            aprendizado: None,
            integrado_cache: (f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            desde_integrado: usize::MAX / 2,
            quadro: vec![0.0; nc],
            saida: vec![0.0; nc],
        }
    }

    pub fn latencia(&self) -> usize {
        self.lim.latencia()
    }

    pub fn aprendizado(&self) -> Option<Aprendizado> {
        self.aprendizado
    }

    /// Recomeça o integrado da saída (sem mexer no LISTEN).
    pub fn zerar_integrado(&mut self) {
        self.med_saida.zerar();
    }

    fn comecar_escuta(&mut self) {
        self.sombra = Sombra::novo(self.nc, self.sr);
        self.med_entrada.zerar();
        self.med_pre.zerar();
        self.med_saida.zerar();
        self.an_aprender.zerar_media();
        self.escuta_amostras = 0;
        self.sm_s = 0.0;
        self.sm_m = 0.0;
    }

    /// Ganho que leva ao alvo: resolve g + efeito(g) = alvo − loudness antes do ganho, com o
    /// efeito da dinâmica medido pela bateria e interpolado entre os ganhos dela.
    fn ganho_para_o_alvo(&self, aj: &AjustesRT, lufs_pre: f64) -> f64 {
        if !lufs_pre.is_finite() {
            return f64::NAN;
        }
        let base = aj.alvo_lufs - lufs_pre;
        let pontos = self.sombra.efeitos(lufs_pre);
        if pontos.len() < 2 {
            return base;
        }
        // h(g) = g + efeito(g) − base cresce com g (a dinâmica nunca come mais que o ganho)
        let h = |&(g, m): &(f64, f64)| g + m - base;
        let reta = |a: (f64, f64), b: (f64, f64)| {
            let (ha, hb) = (h(&a), h(&b));
            if (hb - ha).abs() < 1e-9 { a.0 } else { a.0 + (b.0 - a.0) * (-ha) / (hb - ha) }
        };
        for par in pontos.windows(2) {
            if h(&par[0]) <= 0.0 && h(&par[1]) >= 0.0 {
                return reta(par[0], par[1]);
            }
        }
        // fora da bateria: estende o segmento da ponta
        let n = pontos.len();
        if h(&pontos[0]) > 0.0 { reta(pontos[0], pontos[1]) } else { reta(pontos[n - 2], pontos[n - 1]) }
    }

    fn terminar_escuta(&mut self, aj: &AjustesRT) {
        let espectro_db = self.an_aprender.media_db();
        let (lufs_entrada, lufs_pre, lufs_saida) = (self.med_entrada.integrado(), self.med_pre.integrado(), self.med_saida.integrado());
        let g1 = self.ganho_para_o_alvo(aj, lufs_pre);
        let mut g = if g1.is_finite() { g1 } else { aj.ganho_db };
        if let Some(max) = aj.ganho_max_db {
            g = g.min(max);
        }
        self.aprendizado = Some(Aprendizado {
            segundos: self.escuta_amostras as f64 / self.sr,
            lufs_entrada,
            lufs_pre,
            lufs_saida,
            curva_musica: curva_musica(&espectro_db),
            curva_voz: curva_voz(&espectro_db),
            espectro_db,
            sm_db: if self.sm_m > 0.0 { 10.0 * (self.sm_s / self.sm_m + 1e-20).log10() } else { -200.0 },
            ganho_sugerido_db: g,
        });
    }

    /// Processa um bloco. `entradas` e `saidas` têm `nc` canais de `n` amostras.
    pub fn processar(&mut self, entradas: &[&[f32]], saidas: &mut [&mut [f32]], aj: &AjustesRT, med: &mut MedidasRT) {
        let nc = self.nc;
        let n = entradas.first().map_or(0, |c| c.len());
        if self.eq_atual != Some((aj.bandas, aj.passa_altas_hz)) {
            self.coefs = coeficientes(&aj.bandas, aj.passa_altas_hz, self.sr);
            self.eq_atual = Some((aj.bandas, aj.passa_altas_hz));
        }
        if aj.escutando && !self.escutando {
            self.comecar_escuta();
        } else if !aj.escutando && self.escutando {
            self.terminar_escuta(aj);
        }
        self.escutando = aj.escutando;

        let cfg = &CARATERES[aj.carater as usize];
        let limiar = cfg.compressor.as_ref().map(|c| c.limiar_db + (aj.alvo_lufs - MODOS[Modo::Musica as usize].alvo_lufs));
        let teto = db_para_lin(aj.teto_db);
        let a_rel_lim = (-1.0 / (cfg.limitador_release_ms * self.sr / 1000.0)).exp();
        let side = db_para_lin(aj.width_db);
        let g_alvo = db_para_lin(aj.ganho_db);
        let g_comparar = db_para_lin(aj.ganho_comparar_db);
        if self.primeiro {
            self.ganho_lin = g_alvo;
            self.primeiro = false;
        }
        let latencia = self.latencia();
        let (mut menor_lim, mut menor_comp, mut pico) = (1.0f64, 0.0f64, 0.0f64);

        for i in 0..n {
            let mut mono = 0.0;
            for c in 0..nc {
                let x = entradas[c][i] as f64;
                self.quadro[c] = x;
                self.orig[c][self.n_orig & self.orig_mascara] = x;
                mono += x;
            }
            self.n_orig += 1;
            self.an_entrada.empurrar(mono, false, 1.0);
            if self.escutando {
                self.an_aprender.empurrar(mono, true, 0.0);
                self.med_entrada.medir(&self.quadro);
                self.escuta_amostras += 1;
            }
            // EQ
            for c in 0..nc {
                let mut v = self.quadro[c];
                for (e, k) in self.eq_est[c].iter_mut().zip(&self.coefs) {
                    if *k != Coef::PLANO {
                        v = e.filtrar(k, v);
                    }
                }
                self.quadro[c] = v;
            }
            // largura (como no clip inteiro: sempre recompõe M/S em estéreo)
            if nc == 2 {
                let (l, r) = (self.quadro[0], self.quadro[1]);
                let m = (l + r) / 2.0;
                let s = (l - r) / 2.0 * side;
                self.quadro[0] = m + s;
                self.quadro[1] = m - s;
            }
            if self.escutando {
                self.med_pre.medir(&self.quadro);
                self.sombra.amostra(&self.quadro, cfg, limiar, teto, a_rel_lim);
                if nc == 2 {
                    let (l, r) = (self.quadro[0], self.quadro[1]);
                    self.sm_m += (l + r) * (l + r);
                    self.sm_s += (l - r) * (l - r);
                }
            }
            // ganho (suavizado quando muda)
            self.ganho_lin += (g_alvo - self.ganho_lin) * self.a_ganho;
            if (self.ganho_lin - g_alvo).abs() < 1e-12 {
                self.ganho_lin = g_alvo;
            }
            for v in self.quadro.iter_mut() {
                *v *= self.ganho_lin;
            }
            // compressor (DENSE)
            if let (Some(c), Some(limiar)) = (&cfg.compressor, limiar) {
                let q = self.quadro.iter().map(|v| v * v).sum::<f64>() / nc as f64;
                let red = self.comp.reducao(q, limiar, c.razao, c.joelho_db);
                menor_comp = menor_comp.min(red);
                let g = 10f64.powf(red / 20.0);
                self.quadro.iter_mut().for_each(|v| *v *= g);
            }
            // saturação (WARM)
            if cfg.saturacao > 0.0 {
                let d = cfg.saturacao;
                self.quadro.iter_mut().for_each(|v| *v = (d * 3.0 * *v).tanh() / (d * 3.0));
            }
            // limitador
            let gf = self.lim.empurrar(&self.quadro, &mut self.saida, teto, a_rel_lim);
            menor_lim = menor_lim.min(gf);
            if aj.comparar {
                for c in 0..nc {
                    self.saida[c] = if self.n_orig > latencia {
                        self.orig[c][(self.n_orig - 1 - latencia) & self.orig_mascara] * g_comparar
                    } else {
                        0.0
                    };
                }
            }
            // trava de segurança: nada que não seja número sai daqui (no master isso seria
            // um estalo ou silêncio); se aparecer, zera os estados que guardam o passado
            if self.saida.iter().any(|v| !v.is_finite()) {
                self.saida.iter_mut().for_each(|v| *v = 0.0);
                self.comp.zerar();
                self.eq_est.iter_mut().for_each(|e| *e = [Estado::default(); 5]);
                self.lim = LimitadorRT::novo(nc, self.sr);
            }
            let mut mono_s = 0.0;
            for c in 0..nc {
                saidas[c][i] = self.saida[c] as f32;
                mono_s += self.saida[c];
                pico = pico.max(self.saida[c].abs());
            }
            self.med_saida.medir(&self.saida);
            self.an_saida.empurrar(mono_s, false, 1.0);
        }

        // integrados: refaz a conta no máximo duas vezes por segundo
        self.desde_integrado = self.desde_integrado.saturating_add(n);
        if self.desde_integrado as f64 >= 0.5 * self.sr {
            self.desde_integrado = 0;
            self.integrado_cache = (
                self.med_saida.integrado(),
                if self.escutando { self.med_entrada.integrado() } else { f64::NEG_INFINITY },
                if self.escutando { self.med_pre.integrado() } else { f64::NEG_INFINITY },
            );
        }
        med.momentaneo = self.med_saida.janela(4);
        med.curto = self.med_saida.janela(30);
        med.integrado = self.integrado_cache.0;
        med.lufs_entrada_escuta = self.integrado_cache.1;
        med.lufs_pre_escuta = self.integrado_cache.2;
        med.pico_db = 20.0 * pico.max(1e-12).log10();
        med.limitador_db = 20.0 * menor_lim.max(1e-12).log10();
        med.compressor_db = menor_comp;
        med.espectro_entrada = self.an_entrada.ultimo;
        med.espectro_saida = self.an_saida.ultimo;
        med.escutando_s = if self.escutando { self.escuta_amostras as f64 / self.sr } else { 0.0 };
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::cadeia::{pre_processar_bandas, processar_com_ganho};
    use crate::modos::Ajustes;

    fn sinal(n: usize, sr: f64) -> Vec<Vec<f64>> {
        // ruído com rajadas fortes (faz o compressor e o limitador trabalharem)
        let mut s = 7u64;
        let mut ruido = || {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (s >> 33) as f64 / (1u64 << 31) as f64 - 0.5
        };
        (0..2)
            .map(|c| {
                (0..n)
                    .map(|i| {
                        let envelope = if (i / (sr as usize / 3)) % 4 == 1 { 1.2 } else { 0.08 };
                        envelope * ruido() + 0.1 * ((i as f64) * 0.05 + c as f64).sin()
                    })
                    .collect()
            })
            .collect()
    }

    fn rodar(rt: &mut ApuraRT, x: &[Vec<f64>], aj: &AjustesRT, extra: usize) -> Vec<Vec<f64>> {
        let n = x[0].len() + extra;
        let ent: Vec<Vec<f32>> = x.iter().map(|c| c.iter().map(|&v| v as f32).chain(std::iter::repeat(0.0).take(extra)).collect()).collect();
        let mut sai = vec![vec![0.0f32; n]; x.len()];
        let mut med = MedidasRT::default();
        let mut i = 0;
        while i < n {
            let fim = (i + 512).min(n);
            let e: Vec<&[f32]> = ent.iter().map(|c| &c[i..fim]).collect();
            let mut s: Vec<&mut [f32]> = sai.iter_mut().map(|c| &mut c[i..fim]).collect();
            rt.processar(&e, &mut s, aj, &mut med);
            i = fim;
        }
        sai.into_iter().map(|c| c.into_iter().map(|v| v as f64).collect()).collect()
    }

    #[test]
    fn igual_a_cadeia_do_clip_inteiro() {
        let sr = 48000.0;
        let x = sinal(48000 * 4, sr);
        // a mesma entrada em f32, como o plugin recebe
        let x32: Vec<Vec<f64>> = x.iter().map(|c| c.iter().map(|&v| v as f32 as f64).collect()).collect();
        for carater in [Carater::Transparent, Carater::Clean, Carater::Valve] {
            let aj = AjustesRT {
                carater,
                ganho_db: 4.0,
                bandas: [
                    Banda { freq: 100.0, ganho_db: 2.0 },
                    Banda { freq: 400.0, ganho_db: -3.0 },
                    Banda { freq: 3000.0, ganho_db: 1.5 },
                    Banda { freq: 10000.0, ganho_db: -1.0 },
                ],
                passa_altas_hz: 60.0,
                width_db: -2.0,
                ..AjustesRT::default()
            };
            let mut rt = ApuraRT::novo(2, sr);
            let lat = rt.latencia();
            assert_eq!(lat, 11 + 2 * 48);
            let y = rodar(&mut rt, &x32, &aj, lat);

            let coefs = coeficientes(&aj.bandas, aj.passa_altas_hz, sr);
            let pre = pre_processar_bandas(&x32, sr, &coefs, aj.width_db);
            let mut base = Ajustes::de_modo(Modo::Tv, carater);
            base.alvo_lufs = aj.alvo_lufs;
            base.teto_db = aj.teto_db;
            let (esperado, _) = processar_com_ganho(&pre, sr, &base, aj.ganho_db);
            let mut dif = 0.0f64;
            for c in 0..2 {
                // fora as últimas amostras (o fim do clip é tratado um pouco diferente)
                for k in 0..x[0].len() - 300 {
                    dif = dif.max((y[c][k + lat] - (esperado[c][k] as f32 as f64)).abs());
                }
            }
            assert!(dif < 2e-6, "{carater:?}: diferença {dif}");
        }
    }

    #[test]
    fn medidor_ao_vivo_bate_com_o_integrado() {
        let sr = 48000.0;
        let x = sinal(48000 * 8, sr);
        let mut rt = ApuraRT::novo(2, sr);
        let aj = AjustesRT { escutando: true, ganho_db: 0.0, teto_db: 0.0, ..AjustesRT::default() };
        rodar(&mut rt, &x, &aj, 0);
        let fim = AjustesRT { escutando: false, ..aj };
        rodar(&mut rt, &[vec![0.0; 512], vec![0.0; 512]], &fim, 0);
        let a = rt.aprendizado().expect("aprendizado");
        let offline = crate::loudness::integrado(&x, sr);
        assert!((a.lufs_entrada - offline).abs() < 0.05, "{} × {}", a.lufs_entrada, offline);
        assert!((a.segundos - 8.0).abs() < 0.02);
        assert!(a.espectro_db.iter().skip(4).all(|v| *v > -150.0));
    }

    #[test]
    fn um_listen_so_ja_chega_no_alvo() {
        // de ponta a ponta: LISTEN, depois toca com o ganho aprendido e mede a saída
        let sr = 48000.0;
        let x: Vec<Vec<f64>> = sinal(48000 * 30, sr).into_iter().map(|c| c.into_iter().map(|v| v * 0.3).collect()).collect();
        for (carater, alvo) in [(Carater::Transparent, -23.0), (Carater::Clean, -14.0), (Carater::Valve, -16.0)] {
            let mut rt = ApuraRT::novo(2, sr);
            let aj = AjustesRT { carater, alvo_lufs: alvo, ganho_max_db: None, escutando: true, ..AjustesRT::default() };
            rodar(&mut rt, &x, &aj, 0);
            rodar(&mut rt, &[vec![0.0; 512], vec![0.0; 512]], &AjustesRT { escutando: false, ..aj }, 0);
            let g = rt.aprendizado().expect("aprendizado").ganho_sugerido_db;
            let mut rt2 = ApuraRT::novo(2, sr);
            let lat = rt2.latencia();
            let y = rodar(&mut rt2, &x, &AjustesRT { ganho_db: g, escutando: false, ..aj }, lat);
            let y: Vec<Vec<f64>> = y.into_iter().map(|c| c[lat..].to_vec()).collect();
            let l = crate::loudness::integrado(&y, sr);
            assert!((l - alvo).abs() < 0.15, "{carater:?}: saiu {l:.2}, alvo {alvo} (ganho {g:.2})");
        }
    }

    #[test]
    fn rajada_seguida_de_silencio_nao_vira_nan() {
        // o caso que quebrou num som direto: trecho forte e depois silêncio absoluto, no DENSE
        let sr = 48000.0;
        let mut x = sinal(48000 * 3, sr);
        for c in x.iter_mut() {
            c.iter_mut().for_each(|v| *v *= 3.0);
            c.extend(std::iter::repeat(0.0).take(48000 * 3));
        }
        let aj = AjustesRT { carater: Carater::Clean, ganho_db: 12.0, ..AjustesRT::default() };
        let mut rt = ApuraRT::novo(2, sr);
        let y = rodar(&mut rt, &x, &aj, 0);
        assert!(y.iter().all(|c| c.iter().all(|v| v.is_finite())));
        // e um NaN na entrada não contamina o resto
        let mut z = sinal(48000, sr);
        z[0][100] = f64::NAN;
        let y = rodar(&mut rt, &z, &aj, 0);
        assert!(y.iter().all(|c| c.iter().all(|v| v.is_finite())));
        assert!(y[0][30000..].iter().any(|v| v.abs() > 1e-4));
    }

    #[test]
    fn comparar_devolve_o_original_atrasado() {
        let sr = 48000.0;
        let x = sinal(9000, sr);
        let mut rt = ApuraRT::novo(2, sr);
        let lat = rt.latencia();
        let aj = AjustesRT { comparar: true, ganho_comparar_db: -6.0, ..AjustesRT::default() };
        let y = rodar(&mut rt, &x, &aj, lat);
        let g = db_para_lin(-6.0);
        let dif = (0..9000).map(|k| (y[0][k + lat] - (x[0][k] as f32 as f64) * g).abs()).fold(0.0, f64::max);
        assert!(dif < 1e-6, "{dif}");
    }
}
