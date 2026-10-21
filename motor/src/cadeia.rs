//! A cadeia do clip inteiro: análise → EQ → largura → ganho → dinâmica,
//! com o ganho ajustado até a saída cair no alvo.

use crate::analise::{analisar, espectro_bandas, Analise};
use crate::dinamica::{self, MedidasDinamica};
use crate::loudness::FiltroK;
use crate::modos::{Ajustes, Carater};
use crate::eq4::{self, Coef};
use crate::{eq, loudness, true_peak, util::db_para_lin, Modo};

/// Opções que não são ajustes de som.
#[derive(Clone, Copy, Debug, Default)]
pub struct Opcoes {
    /// filtro K da medição de loudness (o da norma; o do pyloudnorm só nos testes de paridade)
    pub filtro_k: FiltroK,
    /// mede também a faixa de loudness (LRA) da entrada e da saída (mais lento)
    pub medir_faixa: bool,
}

const MAX_ITERACOES: usize = 8;
const TOLERANCIA_LU: f64 = 0.05;

#[derive(Clone, Debug)]
pub struct Resultado {
    pub analise: Analise,
    /// curva de EQ aplicada, dB por banda
    pub eq_db: [f64; 30],
    /// ganho do Side aplicado, dB
    pub side_db: f64,
    /// ganho aplicado antes da dinâmica, dB
    pub ganho_db: f64,
    /// o ganho parou no teto sem chegar ao alvo
    pub no_teto_de_ganho: bool,
    pub lufs_entrada: f64,
    pub lufs_saida: f64,
    pub tp_entrada: f64,
    pub tp_saida: f64,
    /// faixa de loudness (NaN se não foi pedida)
    pub lra_entrada: f64,
    pub lra_saida: f64,
    pub correlacao_entrada: f64,
    pub correlacao_saida: f64,
    pub espectro_saida_db: [f64; 30],
    pub dinamica: MedidasDinamica,
}

/// EQ (curva da análise, FIR de fase linear) e largura: a parte que não depende do ganho.
pub fn pre_processar(canais: &[Vec<f64>], sr: f64, eq_db: &[f64; 30], side_db: f64) -> Vec<Vec<f64>> {
    let h = eq::desenhar(eq_db, sr);
    let mut pre: Vec<Vec<f64>> = canais.iter().map(|c| eq::aplicar(c, &h)).collect();
    aplicar_largura(&mut pre, side_db);
    pre
}

/// EQ de 4 pontos (biquads, igual ao tempo real) e largura.
pub fn pre_processar_bandas(canais: &[Vec<f64>], _sr: f64, coefs: &[Coef; 5], side_db: f64) -> Vec<Vec<f64>> {
    let mut pre: Vec<Vec<f64>> = canais.iter().map(|c| eq4::aplicar(c, coefs)).collect();
    aplicar_largura(&mut pre, side_db);
    pre
}

/// Largura (ganho no Side), só em estéreo.
fn aplicar_largura(pre: &mut [Vec<f64>], side_db: f64) {
    if pre.len() == 2 {
        let side = db_para_lin(side_db);
        let (l, r) = pre.split_at_mut(1);
        for (vl, vr) in l[0].iter_mut().zip(r[0].iter_mut()) {
            let m = (*vl + *vr) / 2.0;
            let s = (*vl - *vr) / 2.0 * side;
            *vl = m + s;
            *vr = m - s;
        }
    }
}

/// Ganho e dinâmica sobre o sinal já equalizado.
pub fn processar_com_ganho(pre: &[Vec<f64>], sr: f64, aj: &Ajustes, ganho_db: f64) -> (Vec<Vec<f64>>, MedidasDinamica) {
    let g = db_para_lin(ganho_db);
    let mut y: Vec<Vec<f64>> = pre.iter().map(|c| c.iter().map(|v| v * g).collect()).collect();
    let med = dinamica::processar(&mut y, sr, aj.carater, aj.alvo_lufs, aj.teto_db);
    (y, med)
}

/// Uma entrega com os ajustes do protótipo (atalho dos testes).
pub fn masterizar(canais: &[Vec<f64>], sr: f64, carater: Carater, modo: Modo) -> (Vec<Vec<f64>>, Resultado) {
    let a = analisar(canais, sr);
    masterizar_com(canais, sr, a, &Ajustes::de_modo(modo, carater), Opcoes::default())
}

/// Clip inteiro: processa com os ajustes e acerta o ganho até a saída chegar no alvo.
pub fn masterizar_com(canais: &[Vec<f64>], sr: f64, a: Analise, aj: &Ajustes, op: Opcoes) -> (Vec<Vec<f64>>, Resultado) {
    let medir = |x: &[Vec<f64>]| loudness::integrado_com(x, sr, op.filtro_k);
    let side_db = a.side_final(aj);
    let (pre, eq_db) = match aj.eq_bandas {
        Some(b) => {
            let coefs = eq4::coeficientes(&b, aj.passa_altas_hz, sr);
            (pre_processar_bandas(canais, sr, &coefs, side_db), eq4::resposta_bandas(&b, aj.passa_altas_hz, sr))
        }
        None => {
            let e = a.eq_final(aj);
            (pre_processar(canais, sr, &e, side_db), e)
        }
    };
    let teto = aj.ganho_max_db.unwrap_or(f64::INFINITY);
    let mut ganho = aj.alvo_lufs - medir(&pre);
    if !ganho.is_finite() {
        ganho = 0.0; // silêncio: não há o que nivelar
    }
    let mut saida = Vec::new();
    let mut medidas = MedidasDinamica::default();
    let mut aplicado = ganho;
    let mut lufs_saida = f64::NEG_INFINITY;
    for _ in 0..MAX_ITERACOES {
        ganho = ganho.min(teto);
        let (y, med) = processar_com_ganho(&pre, sr, aj, ganho);
        aplicado = ganho;
        lufs_saida = medir(&y);
        saida = y;
        medidas = med;
        let erro = aj.alvo_lufs - lufs_saida;
        if !erro.is_finite() || erro.abs() < TOLERANCIA_LU || (ganho >= teto && erro > 0.0) {
            break;
        }
        // o compressor do CLEAN engole parte do ganho: corrige com folga
        ganho += erro * 1.5;
    }
    let faixa = |x: &[Vec<f64>]| if op.medir_faixa { loudness::faixa_simplificada(x, sr) } else { f64::NAN };
    let resultado = Resultado {
        lufs_entrada: medir(canais),
        tp_entrada: true_peak::maximo_db(canais),
        tp_saida: true_peak::maximo_db(&saida),
        lra_entrada: faixa(canais),
        lra_saida: faixa(&saida),
        correlacao_entrada: correlacao(canais),
        correlacao_saida: correlacao(&saida),
        espectro_saida_db: espectro_bandas(&saida, sr),
        lufs_saida,
        ganho_db: aplicado,
        no_teto_de_ganho: aplicado >= teto,
        dinamica: medidas,
        eq_db,
        side_db,
        analise: a,
    };
    (saida, resultado)
}

/// Comparação no mesmo volume (BYPASS · LOUDNESS MATCH): o original só com o ganho
/// que o leva ao alvo, sem EQ, dinâmica nem limitador.
pub fn original_nivelado(canais: &[Vec<f64>], sr: f64, alvo_lufs: f64) -> (Vec<Vec<f64>>, f64) {
    let l = loudness::integrado(canais, sr);
    let ganho = if l.is_finite() { alvo_lufs - l } else { 0.0 };
    let g = db_para_lin(ganho);
    (canais.iter().map(|c| c.iter().map(|v| v * g).collect()).collect(), ganho)
}

/// Correlação L/R (−1 a +1); 1 em mono ou silêncio.
pub fn correlacao(canais: &[Vec<f64>]) -> f64 {
    if canais.len() != 2 {
        return 1.0;
    }
    let (mut lr, mut ll, mut rr) = (0.0, 0.0, 0.0);
    for (l, r) in canais[0].iter().zip(&canais[1]) {
        lr += l * r;
        ll += l * l;
        rr += r * r;
    }
    if ll == 0.0 || rr == 0.0 {
        return 1.0;
    }
    lr / (ll * rr).sqrt()
}

#[cfg(test)]
mod testes {
    use super::*;

    fn ruido(n: usize, semente: u64) -> Vec<f64> {
        let mut s = semente;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((s >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 0.2
            })
            .collect()
    }

    #[test]
    fn mono_chega_no_alvo_e_respeita_o_teto() {
        let x = vec![ruido(48000 * 8, 1)];
        let (y, r) = masterizar(&x, 48000.0, Carater::Punch, Modo::Tv);
        assert_eq!(y.len(), 1);
        assert!((r.lufs_saida + 23.0).abs() < 0.06, "{}", r.lufs_saida);
        assert!(r.tp_saida <= -0.95, "{}", r.tp_saida);
    }

    #[test]
    fn silencio_passa_sem_quebrar() {
        let x = vec![vec![0.0; 48000 * 2], vec![0.0; 48000 * 2]];
        let (y, r) = masterizar(&x, 48000.0, Carater::Clean, Modo::Musica);
        assert!(y.iter().all(|c| c.iter().all(|v| *v == 0.0)));
        assert_eq!(r.ganho_db, 0.0);
    }

    #[test]
    fn eq_desligada_e_largura_zero_so_mexem_no_volume() {
        // AUTO EQ 0%, sem passa-altas, WIDTH 0 dB e sinal baixo: o resultado é o original × ganho
        let x = vec![ruido(48000 * 6, 2), ruido(48000 * 6, 3)];
        let a = analisar(&x, 48000.0);
        let mut aj = Ajustes::de_modo(Modo::Tv, Carater::Transparent);
        aj.eq_quantidade = 0.0;
        aj.passa_altas_hz = 0.0;
        let (y, r) = masterizar_com(&x, 48000.0, a, &aj, Opcoes::default());
        let g = db_para_lin(r.ganho_db);
        let erro = x[0].iter().zip(&y[0]).skip(9000).take(200000).map(|(a, b)| (a * g - b).abs()).fold(0.0, f64::max);
        assert!(erro < 1e-3 * g, "{erro}");
    }
}
