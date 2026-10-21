//! EQ de 4 pontos (a que a janela mostra para arrastar): shelf grave, dois sinos e shelf
//! agudo, mais o passa-altas opcional. Biquads de fase mínima (fórmulas do RBJ), sem atraso,
//! iguais no tempo real e no clip inteiro.
//!
//! `ajustar` é o MATCH: encaixa nos 4 pontos a curva que a análise sugere.

use crate::analise::BANDAS;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Banda {
    pub freq: f64,
    pub ganho_db: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tipo {
    ShelfGrave,
    Sino,
    ShelfAgudo,
    PassaAltas,
}

/// Tipo de cada ponto: LOW, LOW-MID, HIGH-MID, HIGH.
pub const TIPOS: [Tipo; 4] = [Tipo::ShelfGrave, Tipo::Sino, Tipo::Sino, Tipo::ShelfAgudo];
/// Pontos na posição inicial (EQ plana).
pub const PADRAO: [Banda; 4] = [
    Banda { freq: 80.0, ganho_db: 0.0 },
    Banda { freq: 300.0, ganho_db: 0.0 },
    Banda { freq: 2500.0, ganho_db: 0.0 },
    Banda { freq: 10000.0, ganho_db: 0.0 },
];
pub const GANHO_MAX_DB: f64 = 12.0;
const Q_SINO: f64 = 0.9;
const Q_SHELF: f64 = std::f64::consts::FRAC_1_SQRT_2;
const Q_PASSA_ALTAS: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// Coeficientes normalizados (a0 = 1): [b0, b1, b2, a1, a2].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coef(pub [f64; 5]);

impl Coef {
    pub const PLANO: Coef = Coef([1.0, 0.0, 0.0, 0.0, 0.0]);

    pub fn novo(tipo: Tipo, freq: f64, ganho_db: f64, sr: f64) -> Coef {
        let f = freq.clamp(10.0, 0.45 * sr);
        let w0 = 2.0 * std::f64::consts::PI * f / sr;
        let (s, c) = (w0.sin(), w0.cos());
        let a = 10f64.powf(ganho_db / 40.0);
        let (b, a_) = match tipo {
            Tipo::Sino => {
                let alfa = s / (2.0 * Q_SINO);
                ([1.0 + alfa * a, -2.0 * c, 1.0 - alfa * a], [1.0 + alfa / a, -2.0 * c, 1.0 - alfa / a])
            }
            Tipo::ShelfGrave => {
                let alfa = s / (2.0 * Q_SHELF);
                let r = 2.0 * a.sqrt() * alfa;
                (
                    [a * ((a + 1.0) - (a - 1.0) * c + r), 2.0 * a * ((a - 1.0) - (a + 1.0) * c), a * ((a + 1.0) - (a - 1.0) * c - r)],
                    [(a + 1.0) + (a - 1.0) * c + r, -2.0 * ((a - 1.0) + (a + 1.0) * c), (a + 1.0) + (a - 1.0) * c - r],
                )
            }
            Tipo::ShelfAgudo => {
                let alfa = s / (2.0 * Q_SHELF);
                let r = 2.0 * a.sqrt() * alfa;
                (
                    [a * ((a + 1.0) + (a - 1.0) * c + r), -2.0 * a * ((a - 1.0) + (a + 1.0) * c), a * ((a + 1.0) + (a - 1.0) * c - r)],
                    [(a + 1.0) - (a - 1.0) * c + r, 2.0 * ((a - 1.0) - (a + 1.0) * c), (a + 1.0) - (a - 1.0) * c - r],
                )
            }
            Tipo::PassaAltas => {
                let alfa = s / (2.0 * Q_PASSA_ALTAS);
                ([(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0], [1.0 + alfa, -2.0 * c, 1.0 - alfa])
            }
        };
        let a0 = a_[0];
        Coef([b[0] / a0, b[1] / a0, b[2] / a0, a_[1] / a0, a_[2] / a0])
    }

    /// Ganho em dB na frequência `f`.
    pub fn resposta_db(&self, f: f64, sr: f64) -> f64 {
        let w = 2.0 * std::f64::consts::PI * f / sr;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let [b0, b1, b2, a1, a2] = self.0;
        let (nr, ni) = (b0 + b1 * c1 + b2 * c2, -(b1 * s1 + b2 * s2));
        let (dr, di) = (1.0 + a1 * c1 + a2 * c2, -(a1 * s1 + a2 * s2));
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

/// Os 5 filtros da EQ (passa-altas + 4 pontos) para uma taxa.
pub fn coeficientes(bandas: &[Banda; 4], passa_altas_hz: f64, sr: f64) -> [Coef; 5] {
    let mut c = [Coef::PLANO; 5];
    if passa_altas_hz > 0.0 {
        c[0] = Coef::novo(Tipo::PassaAltas, passa_altas_hz, 0.0, sr);
    }
    for (i, b) in bandas.iter().enumerate() {
        if b.ganho_db.abs() > 1e-9 {
            c[i + 1] = Coef::novo(TIPOS[i], b.freq, b.ganho_db.clamp(-GANHO_MAX_DB, GANHO_MAX_DB), sr);
        }
    }
    c
}

/// Resposta total em dB nas 30 bandas (para a janela e para o MATCH).
pub fn resposta_bandas(bandas: &[Banda; 4], passa_altas_hz: f64, sr: f64) -> [f64; 30] {
    let c = coeficientes(bandas, passa_altas_hz, sr);
    let mut r = [0.0; 30];
    for (i, &f) in BANDAS.iter().enumerate() {
        if f < 0.5 * sr {
            r[i] = c.iter().map(|k| k.resposta_db(f, sr)).sum();
        }
    }
    r
}

/// Filtro de um canal (forma direta II transposta).
#[derive(Clone, Copy, Default)]
pub struct Estado {
    z1: f64,
    z2: f64,
}

impl Estado {
    #[inline]
    pub fn filtrar(&mut self, c: &Coef, x: f64) -> f64 {
        let [b0, b1, b2, a1, a2] = c.0;
        let y = b0 * x + self.z1;
        self.z1 = b1 * x - a1 * y + self.z2;
        self.z2 = b2 * x - a2 * y;
        y
    }
}

/// EQ de um canal inteiro (clip inteiro).
pub fn aplicar(canal: &[f64], coefs: &[Coef; 5]) -> Vec<f64> {
    let mut est = [Estado::default(); 5];
    canal
        .iter()
        .map(|&x| {
            let mut v = x;
            for (e, c) in est.iter_mut().zip(coefs) {
                if *c != Coef::PLANO {
                    v = e.filtrar(c, v);
                }
            }
            v
        })
        .collect()
}

// --- MATCH -------------------------------------------------------------------------------

const CAND_GRAVE: [f64; 4] = [60.0, 80.0, 100.0, 150.0];
const CAND_MEDIO_GRAVE: [f64; 7] = [200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0];
const CAND_MEDIO_AGUDO: [f64; 6] = [1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0];
const CAND_AGUDO: [f64; 4] = [6000.0, 8000.0, 10000.0, 12000.0];

fn peso(f: f64) -> f64 {
    if (40.0..=12500.0).contains(&f) {
        1.0
    } else {
        0.2
    }
}

/// Resolve um sistema 4×4 (eliminação de Gauss com pivô); None se singular.
fn resolver(mut a: [[f64; 4]; 4], mut b: [f64; 4]) -> Option<[f64; 4]> {
    for i in 0..4 {
        let p = (i..4).max_by(|&x, &y| a[x][i].abs().partial_cmp(&a[y][i].abs()).unwrap())?;
        if a[p][i].abs() < 1e-12 {
            return None;
        }
        a.swap(i, p);
        b.swap(i, p);
        for j in i + 1..4 {
            let f = a[j][i] / a[i][i];
            for k in i..4 {
                a[j][k] -= f * a[i][k];
            }
            b[j] -= f * b[i];
        }
    }
    let mut x = [0.0; 4];
    for i in (0..4).rev() {
        let s: f64 = (i + 1..4).map(|k| a[i][k] * x[k]).sum();
        x[i] = (b[i] - s) / a[i][i];
    }
    Some(x)
}

fn resposta_banda(tipo: Tipo, f0: f64, g: f64, sr: f64) -> [f64; 30] {
    let c = Coef::novo(tipo, f0, g, sr);
    let mut r = [0.0; 30];
    for (i, &f) in BANDAS.iter().enumerate() {
        if f < 0.5 * sr {
            r[i] = c.resposta_db(f, sr);
        }
    }
    r
}

/// Ganhos que melhor aproximam `alvo` com as frequências dadas (Gauss-Newton), e o erro.
fn ganhos_para(freqs: [f64; 4], alvo: &[f64; 30], sr: f64) -> ([f64; 4], f64) {
    let mut g = [0.0f64; 4];
    for _ in 0..4 {
        let atual: Vec<[f64; 30]> = (0..4).map(|k| resposta_banda(TIPOS[k], freqs[k], g[k], sr)).collect();
        // jacobiano por diferença central
        let jac: Vec<[f64; 30]> = (0..4)
            .map(|k| {
                let (mais, menos) = (resposta_banda(TIPOS[k], freqs[k], g[k] + 0.5, sr), resposta_banda(TIPOS[k], freqs[k], g[k] - 0.5, sr));
                let mut d = [0.0; 30];
                for i in 0..30 {
                    d[i] = mais[i] - menos[i];
                }
                d
            })
            .collect();
        let (mut ata, mut atb) = ([[0.0; 4]; 4], [0.0; 4]);
        for i in 0..30 {
            let w = peso(BANDAS[i]);
            let resid = alvo[i] - (0..4).map(|k| atual[k][i]).sum::<f64>();
            for p in 0..4 {
                atb[p] += w * jac[p][i] * resid;
                for q in 0..4 {
                    ata[p][q] += w * jac[p][i] * jac[q][i];
                }
            }
        }
        // um pouco de regularização: prefere ganhos pequenos quando a curva não pede
        for p in 0..4 {
            ata[p][p] += 0.02;
        }
        let Some(passo) = resolver(ata, atb) else { break };
        for k in 0..4 {
            g[k] = (g[k] + passo[k]).clamp(-GANHO_MAX_DB, GANHO_MAX_DB);
        }
    }
    let mut erro = 0.0;
    let total: Vec<[f64; 30]> = (0..4).map(|k| resposta_banda(TIPOS[k], freqs[k], g[k], sr)).collect();
    for i in 0..30 {
        let d = alvo[i] - (0..4).map(|k| total[k][i]).sum::<f64>();
        erro += peso(BANDAS[i]) * d * d;
    }
    (g, erro)
}

/// MATCH: os 4 pontos que melhor desenham a curva sugerida (dB por banda).
pub fn ajustar(alvo: &[f64; 30], sr: f64) -> [Banda; 4] {
    let mut melhor = (f64::INFINITY, PADRAO);
    for &fg in &CAND_GRAVE {
        for &fa in &CAND_AGUDO {
            // os sinos primeiro com os shelves fixos, para não testar todas as combinações
            for &fmg in &CAND_MEDIO_GRAVE {
                for &fma in &CAND_MEDIO_AGUDO {
                    let freqs = [fg, fmg, fma, fa];
                    let (g, erro) = ganhos_para(freqs, alvo, sr);
                    if erro < melhor.0 {
                        let mut b = PADRAO;
                        for k in 0..4 {
                            b[k] = Banda { freq: freqs[k], ganho_db: (g[k] * 10.0).round() / 10.0 };
                        }
                        melhor = (erro, b);
                    }
                }
            }
        }
    }
    melhor.1
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn sino_tem_o_ganho_no_centro() {
        let c = Coef::novo(Tipo::Sino, 1000.0, 6.0, 48000.0);
        assert!((c.resposta_db(1000.0, 48000.0) - 6.0).abs() < 1e-9);
        let s = Coef::novo(Tipo::ShelfGrave, 100.0, -4.0, 48000.0);
        assert!((s.resposta_db(20.0, 48000.0) + 4.0).abs() < 0.2);
        assert!(s.resposta_db(5000.0, 48000.0).abs() < 0.05);
    }

    #[test]
    fn match_reencontra_uma_eq_conhecida() {
        let sr = 48000.0;
        let original = [
            Banda { freq: 100.0, ganho_db: 2.5 },
            Banda { freq: 400.0, ganho_db: -3.0 },
            Banda { freq: 3150.0, ganho_db: 1.5 },
            Banda { freq: 10000.0, ganho_db: -2.0 },
        ];
        let alvo = resposta_bandas(&original, 0.0, sr);
        let achado = ajustar(&alvo, sr);
        let refeito = resposta_bandas(&achado, 0.0, sr);
        let erro = alvo.iter().zip(&refeito).skip(2).take(26).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(erro < 0.15, "erro {erro} {achado:?}");
    }

    #[test]
    fn match_de_curva_plana_e_plano() {
        let b = ajustar(&[0.0; 30], 48000.0);
        assert!(b.iter().all(|x| x.ganho_db.abs() < 0.05), "{b:?}");
    }
}
