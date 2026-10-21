//! Pico verdadeiro (true peak) por oversampling 4×, com o mesmo filtro do
//! `scipy.signal.resample_poly(x, 4, 1)` do protótipo: FIR de 81 coeficientes,
//! janela de Kaiser (β = 5), corte em 1/4 da banda.

use crate::util::{bessel_i0, sinc};

pub(crate) const FATOR: usize = 4;
const MEIO: usize = 10 * FATOR; // half_len do SciPy
pub(crate) const ATRASO: usize = MEIO + 1; // MEIO + 1 zero inserido antes do filtro (n_pre_pad)

pub(crate) fn filtro() -> Vec<f64> {
    let n = 2 * MEIO + 1;
    let alfa = (n - 1) as f64 / 2.0;
    let beta = 5.0;
    let corte = 1.0 / FATOR as f64;
    let mut h: Vec<f64> = (0..n)
        .map(|i| {
            let m = i as f64 - alfa;
            let janela = bessel_i0(beta * (1.0 - (m / alfa).powi(2)).max(0.0).sqrt()) / bessel_i0(beta);
            corte * sinc(corte * m) * janela
        })
        .collect();
    let soma: f64 = h.iter().sum();
    for v in &mut h {
        *v = *v / soma * FATOR as f64;
    }
    // um zero na frente, como o resample_poly faz para centrar a saída
    let mut com_zero = vec![0.0];
    com_zero.extend(h);
    com_zero
}

/// Para cada amostra, o maior módulo entre as 4 sub-amostras e entre os canais.
pub fn picos_por_amostra(canais: &[Vec<f64>]) -> Vec<f64> {
    let h = filtro();
    let n = canais.first().map_or(0, |c| c.len());
    let mut picos = vec![0.0f64; n];
    for c in canais {
        for s in 0..n {
            let mut maior = picos[s];
            for t in FATOR * s..FATOR * s + FATOR {
                let base = ATRASO + t;
                let mut acc = 0.0;
                let mut i = base % FATOR;
                while i < h.len() {
                    if i <= base {
                        let k = (base - i) / FATOR;
                        if k < n {
                            acc += h[i] * c[k];
                        }
                    }
                    i += FATOR;
                }
                maior = maior.max(acc.abs());
            }
            picos[s] = maior;
        }
    }
    picos
}

/// True peak do sinal inteiro, em dBTP.
pub fn maximo_db(canais: &[Vec<f64>]) -> f64 {
    let p = picos_por_amostra(canais).into_iter().fold(0.0f64, f64::max);
    20.0 * p.max(1e-12).log10()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn acha_o_pico_entre_amostras() {
        // seno em sr/4 com fase de 45°: as amostras ficam em ±0,707, o pico real é 1
        let x: Vec<f64> = (0..4800)
            .map(|i| (std::f64::consts::FRAC_PI_2 * i as f64 + std::f64::consts::FRAC_PI_4).sin())
            .collect();
        let amostral = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
        assert!((amostral - 0.7071).abs() < 1e-3);
        let tp = maximo_db(&[x]);
        assert!(tp.abs() < 0.2, "{tp}");
    }

    #[test]
    fn filtro_tem_ganho_4_no_dc() {
        let s: f64 = filtro().iter().sum();
        assert!((s - 4.0).abs() < 1e-12);
    }
}
