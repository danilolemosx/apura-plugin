//! EQ do clip inteiro: FIR de fase linear de 8192 coeficientes desenhado na curva
//! (o atraso é descontado, então a saída fica alinhada com a entrada).

use crate::analise::eq_em;
use realfft::num_complex::Complex;
use realfft::RealFftPlanner;

const N_FIR: usize = 8192;
const BLOCO: usize = 8192;
const N_FFT: usize = 16384; // >= BLOCO + N_FIR − 1

/// Coeficientes: espectro real da curva → resposta ao impulso centrada, janela de Hann simétrica.
pub fn desenhar(eq_db: &[f64; 30], sr: f64) -> Vec<f64> {
    let mut planner = RealFftPlanner::<f64>::new();
    let inv = planner.plan_fft_inverse(N_FIR);
    let mut espectro: Vec<Complex<f64>> = (0..=N_FIR / 2)
        .map(|k| {
            let f = (k as f64 * sr / N_FIR as f64).max(1.0);
            Complex::new(10f64.powf(eq_em(eq_db, f) / 20.0), 0.0)
        })
        .collect();
    let mut h = inv.make_output_vec();
    inv.process(&mut espectro, &mut h).expect("ifft");
    // irfft do NumPy divide por N; depois roll de N/2 e janela de Hann simétrica (np.hanning)
    let mut saida = vec![0.0; N_FIR];
    for i in 0..N_FIR {
        let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (N_FIR - 1) as f64).cos();
        saida[i] = h[(i + N_FIR / 2) % N_FIR] / N_FIR as f64 * w;
    }
    saida
}

/// Convolução por blocos (overlap-add), devolvendo o trecho alinhado com a entrada.
pub fn aplicar(canal: &[f64], h: &[f64]) -> Vec<f64> {
    let n = canal.len();
    let mut planner = RealFftPlanner::<f64>::new();
    let fwd = planner.plan_fft_forward(N_FFT);
    let inv = planner.plan_fft_inverse(N_FFT);

    let mut tmp = fwd.make_input_vec();
    tmp[..h.len()].copy_from_slice(h);
    let mut hf = fwd.make_output_vec();
    fwd.process(&mut tmp, &mut hf).expect("fft");

    let atraso = N_FIR / 2;
    let mut saida = vec![0.0; n];
    let mut bloco = fwd.make_input_vec();
    let mut espectro = fwd.make_output_vec();
    let mut volta = inv.make_output_vec();
    let mut ini = 0;
    while ini < n {
        let fim = (ini + BLOCO).min(n);
        bloco.iter_mut().for_each(|v| *v = 0.0);
        bloco[..fim - ini].copy_from_slice(&canal[ini..fim]);
        fwd.process(&mut bloco, &mut espectro).expect("fft");
        for (e, hk) in espectro.iter_mut().zip(&hf) {
            *e *= hk;
        }
        espectro[0].im = 0.0;
        espectro[N_FFT / 2].im = 0.0;
        inv.process(&mut espectro, &mut volta).expect("ifft");
        // amostra m da convolução completa vai para a posição m − atraso da saída
        for (j, v) in volta.iter().enumerate() {
            let m = ini + j;
            if m >= atraso && m - atraso < n {
                saida[m - atraso] += v / N_FFT as f64;
            }
        }
        ini = fim;
    }
    saida
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn curva_plana_devolve_o_sinal() {
        let h = desenhar(&[0.0; 30], 48000.0);
        let x: Vec<f64> = (0..20000).map(|i| ((i * 31) % 97) as f64 / 97.0 - 0.5).collect();
        let y = aplicar(&x, &h);
        // a janela de Hann deixa um ganho levemente diferente de 1 só no DC/graves extremos
        let erro: f64 = x.iter().zip(&y).skip(5000).take(10000).map(|(a, b)| (a - b).abs()).fold(0.0, f64::max);
        assert!(erro < 1e-3, "{erro}");
    }
}
