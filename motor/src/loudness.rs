//! Loudness integrado (ITU-R BS.1770-4 / EBU R128): filtro K, blocos de 400 ms com 75% de
//! sobreposição, gate absoluto de −70 LUFS e relativo de −10 LU.
//!
//! Dois desenhos do filtro K:
//! - `FiltroK::Itu` (padrão): os coeficientes da norma, recalculados para qualquer taxa
//!   (mesmas constantes da libebur128). Um seno de 997 Hz a 0 dBFS dá −3,01 LKFS.
//! - `FiltroK::Pyloudnorm`: o do pyloudnorm usado no protótipo (fórmulas do RBJ),
//!   ~0,04 LU diferente. Só para conferir o motor amostra a amostra contra o protótipo.

const BLOCO_S: f64 = 0.4;
const PASSO: f64 = 0.25; // 1 − sobreposição
const GATE_ABSOLUTO: f64 = -70.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FiltroK {
    #[default]
    Itu,
    Pyloudnorm,
}

/// Biquad normalizado (a0 = 1), aplicado como o `scipy.signal.lfilter` (forma direta II transposta).
struct Biquad {
    b: [f64; 3],
    a: [f64; 3],
}

impl Biquad {
    fn high_shelf(ganho_db: f64, q: f64, fc: f64, sr: f64) -> Self {
        let a = 10f64.powf(ganho_db / 40.0);
        let w0 = 2.0 * std::f64::consts::PI * (fc / sr);
        let alpha = w0.sin() / (2.0 * q);
        let c = w0.cos();
        let b0 = a * ((a + 1.0) + (a - 1.0) * c + 2.0 * a.sqrt() * alpha);
        let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * c);
        let b2 = a * ((a + 1.0) + (a - 1.0) * c - 2.0 * a.sqrt() * alpha);
        let a0 = (a + 1.0) - (a - 1.0) * c + 2.0 * a.sqrt() * alpha;
        let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * c);
        let a2 = (a + 1.0) - (a - 1.0) * c - 2.0 * a.sqrt() * alpha;
        Self::normalizar([b0, b1, b2], [a0, a1, a2])
    }

    fn high_pass(q: f64, fc: f64, sr: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * (fc / sr);
        let alpha = w0.sin() / (2.0 * q);
        let c = w0.cos();
        let b0 = (1.0 + c) / 2.0;
        let b1 = -(1.0 + c);
        let b2 = (1.0 + c) / 2.0;
        Self::normalizar([b0, b1, b2], [1.0 + alpha, -2.0 * c, 1.0 - alpha])
    }

    /// Estágio 1 da norma (high shelf), com as constantes analógicas da libebur128.
    fn itu_shelf(sr: f64) -> Self {
        let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
        let k = (std::f64::consts::PI * f0 / sr).tan();
        let vh = 10f64.powf(g / 20.0);
        let vb = vh.powf(0.4996667741545416);
        let a0 = 1.0 + k / q + k * k;
        Biquad {
            b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
            a: [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        }
    }

    /// Estágio 2 da norma (high-pass RLB): numerador [1, −2, 1].
    fn itu_passa_altas(sr: f64) -> Self {
        let (f0, q) = (38.13547087602444, 0.5003270373238773);
        let k = (std::f64::consts::PI * f0 / sr).tan();
        let a0 = 1.0 + k / q + k * k;
        Biquad { b: [1.0, -2.0, 1.0], a: [1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0] }
    }

    fn normalizar(b: [f64; 3], a: [f64; 3]) -> Self {
        let a0 = a[0];
        Biquad { b: [b[0] / a0, b[1] / a0, b[2] / a0], a: [1.0, a[1] / a0, a[2] / a0] }
    }

    fn filtrar(&self, x: &[f64]) -> Vec<f64> {
        let (mut z1, mut z2) = (0.0, 0.0);
        x.iter()
            .map(|&v| {
                let y = self.b[0] * v + z1;
                z1 = self.b[1] * v - self.a[1] * y + z2;
                z2 = self.b[2] * v - self.a[2] * y;
                y
            })
            .collect()
    }
}

/// Energia média (filtrada pelo filtro K) de cada bloco de 400 ms, por canal.
fn energias_dos_blocos(canais: &[Vec<f64>], sr: f64, filtro: FiltroK) -> Vec<Vec<f64>> {
    let n = canais.first().map_or(0, |c| c.len());
    let duracao = n as f64 / sr;
    let n_blocos = ((duracao - BLOCO_S) / (BLOCO_S * PASSO)).round_ties_even() as i64 + 1;
    if n_blocos <= 0 {
        return vec![Vec::new(); canais.len()];
    }
    let (shelf, hp) = match filtro {
        FiltroK::Itu => (Biquad::itu_shelf(sr), Biquad::itu_passa_altas(sr)),
        FiltroK::Pyloudnorm => (Biquad::high_shelf(4.0, 1.0 / 2f64.sqrt(), 1500.0, sr), Biquad::high_pass(0.5, 38.0, sr)),
    };
    canais
        .iter()
        .map(|c| {
            let k = hp.filtrar(&shelf.filtrar(c));
            (0..n_blocos)
                .map(|j| {
                    let j = j as f64;
                    let ini = ((BLOCO_S * (j * PASSO)) * sr) as usize;
                    let fim = (((BLOCO_S * (j * PASSO + 1.0)) * sr) as usize).min(k.len());
                    let soma: f64 = k[ini.min(fim)..fim].iter().map(|v| v * v).sum();
                    (1.0 / (BLOCO_S * sr)) * soma
                })
                .collect()
        })
        .collect()
}

/// Filtro K da norma (high shelf + high-pass) como coeficientes da EQ, para o medidor ao vivo.
pub fn filtro_k_itu(sr: f64) -> [crate::eq4::Coef; 2] {
    let (s, h) = (Biquad::itu_shelf(sr), Biquad::itu_passa_altas(sr));
    [crate::eq4::Coef([s.b[0], s.b[1], s.b[2], s.a[1], s.a[2]]), crate::eq4::Coef([h.b[0], h.b[1], h.b[2], h.a[1], h.a[2]])]
}

/// Loudness integrado a partir das energias de blocos de 400 ms (já somadas entre canais),
/// com os mesmos gates: absoluto ≥ −70, relativo > −10 LU.
pub fn integrado_de_blocos(energias: &[f64]) -> f64 {
    let l = |z: f64| -0.691 + 10.0 * z.log10();
    let media = |passa: &dyn Fn(f64) -> bool| {
        let (mut s, mut n) = (0.0, 0usize);
        for &z in energias {
            if passa(l(z)) {
                s += z;
                n += 1;
            }
        }
        if n == 0 { 0.0 } else { s / n as f64 }
    };
    let rel = l(media(&|v| v >= GATE_ABSOLUTO)) - 10.0;
    l(media(&|v| v > rel && v > GATE_ABSOLUTO))
}

fn loudness_de(energias: &[f64]) -> f64 {
    -0.691 + 10.0 * energias.iter().sum::<f64>().log10()
}

/// Loudness integrado em LUFS (filtro da norma). Mais curto que um bloco ou só silêncio: −∞.
pub fn integrado(canais: &[Vec<f64>], sr: f64) -> f64 {
    integrado_com(canais, sr, FiltroK::Itu)
}

pub fn integrado_com(canais: &[Vec<f64>], sr: f64, filtro: FiltroK) -> f64 {
    let z = energias_dos_blocos(canais, sr, filtro);
    let n_blocos = z.first().map_or(0, |c| c.len());
    if n_blocos == 0 {
        return f64::NEG_INFINITY;
    }
    let l: Vec<f64> = (0..n_blocos).map(|j| loudness_de(&z.iter().map(|c| c[j]).collect::<Vec<_>>())).collect();
    let media_gate = |passa: &dyn Fn(f64) -> bool| -> Vec<f64> {
        let blocos: Vec<usize> = (0..n_blocos).filter(|&j| passa(l[j])).collect();
        z.iter()
            .map(|c| if blocos.is_empty() { 0.0 } else { blocos.iter().map(|&j| c[j]).sum::<f64>() / blocos.len() as f64 })
            .collect()
    };
    let z_abs = media_gate(&|v| v >= GATE_ABSOLUTO);
    let gate_relativo = loudness_de(&z_abs) - 10.0;
    let z_rel = media_gate(&|v| v > gate_relativo && v > GATE_ABSOLUTO);
    loudness_de(&z_rel)
}

/// Faixa de loudness simplificada usada nos relatórios do protótipo: janelas de 3 s a cada 1 s,
/// sem as abaixo de −70 LUFS nem as 20 LU abaixo da mais alta; percentis 10 e 95.
pub fn faixa_simplificada(canais: &[Vec<f64>], sr: f64) -> f64 {
    let n = canais.first().map_or(0, |c| c.len());
    let (janela, passo) = ((3.0 * sr) as usize, sr as usize);
    let mut v: Vec<f64> = Vec::new();
    let mut i = 0;
    while i + janela < n {
        let trecho: Vec<Vec<f64>> = canais.iter().map(|c| c[i..i + janela].to_vec()).collect();
        let l = integrado(&trecho, sr);
        if l > -70.0 {
            v.push(l);
        }
        i += passo;
    }
    if v.is_empty() {
        return 0.0;
    }
    let max = v.iter().cloned().fold(f64::MIN, f64::max);
    v.retain(|&l| l > max - 20.0);
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    percentil(&v, 95.0) - percentil(&v, 10.0)
}

/// Percentil com interpolação linear (padrão do `np.percentile`), sobre valores já ordenados.
fn percentil(ordenados: &[f64], p: f64) -> f64 {
    let pos = p / 100.0 * (ordenados.len() - 1) as f64;
    let i = pos.floor() as usize;
    let f = pos - i as f64;
    if i + 1 < ordenados.len() {
        ordenados[i] + f * (ordenados[i + 1] - ordenados[i])
    } else {
        ordenados[i]
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn seno(freq: f64, amp: f64, s: f64, sr: f64) -> Vec<f64> {
        (0..(s * sr) as usize).map(|i| amp * (2.0 * std::f64::consts::PI * freq * i as f64 / sr).sin()).collect()
    }

    #[test]
    fn seno_de_1khz_a_0_dbfs_num_canal_da_menos_3_lufs() {
        // referência da norma: 997 Hz, 0 dBFS, um canal → −3,01 LKFS
        let l = integrado(&[seno(997.0, 1.0, 10.0, 48000.0)], 48000.0);
        assert!((l + 3.01).abs() < 0.02, "{l}");
    }

    #[test]
    fn filtro_do_pyloudnorm_bate_com_o_python() {
        // pyloudnorm 0.1.1: −3.051696092726299 para o mesmo seno
        let l = integrado_com(&[seno(997.0, 1.0, 10.0, 48000.0)], 48000.0, FiltroK::Pyloudnorm);
        assert!((l + 3.051696092726299).abs() < 1e-9, "{l}");
    }

    #[test]
    fn coeficientes_da_norma_a_48k() {
        // ITU-R BS.1770-4, tabelas 1 e 2
        let s = Biquad::itu_shelf(48000.0);
        let h = Biquad::itu_passa_altas(48000.0);
        let esperado_b = [1.53512485958697, -2.69169618940638, 1.19839281085285];
        let esperado_a = [-1.69065929318241, 0.73248077421585];
        for i in 0..3 {
            assert!((s.b[i] - esperado_b[i]).abs() < 1e-8, "b{i} {}", s.b[i]);
        }
        assert!((s.a[1] - esperado_a[0]).abs() < 1e-8 && (s.a[2] - esperado_a[1]).abs() < 1e-8);
        assert!((h.a[1] + 1.99004745483398).abs() < 1e-8 && (h.a[2] - 0.99007225036621).abs() < 1e-8);
    }

    #[test]
    fn estereo_soma_os_canais() {
        let s = seno(997.0, 0.1, 10.0, 48000.0);
        let mono = integrado(&[s.clone()], 48000.0);
        let estereo = integrado(&[s.clone(), s], 48000.0);
        assert!((estereo - mono - 3.0103).abs() < 0.01);
    }

    #[test]
    fn silencio_e_audio_curto() {
        assert_eq!(integrado(&[vec![0.0; 48000]], 48000.0), f64::NEG_INFINITY);
        assert_eq!(integrado(&[vec![0.1; 1000]], 48000.0), f64::NEG_INFINITY);
    }
}
