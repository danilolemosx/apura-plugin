//! Dinâmica: compressor (só no CLEAN), saturação (só no VALVE) e limitador true peak.

use crate::modos::{Carater, CARATERES, COMP_ATAQUE_MS, COMP_RELEASE_MS, COMP_RMS_MS, LOOKAHEAD_MS, MODOS};
use crate::true_peak::picos_por_amostra;
use crate::util::{db_para_lin, media_movel, minimo_deslizante};
use crate::Modo;

/// Compressor de banda larga com detecção RMS (5 ms) e joelho suave.
/// O ganho é o mesmo para todos os canais.
pub fn comprimir(canais: &mut [Vec<f64>], sr: f64, limiar: f64, razao: f64, joelho: f64) -> Vec<f64> {
    let n = canais.first().map_or(0, |c| c.len());
    let nc = canais.len() as f64;
    let quadrados: Vec<f64> = (0..n).map(|i| canais.iter().map(|c| c[i] * c[i]).sum::<f64>() / nc).collect();
    let rms = media_movel(&quadrados, (COMP_RMS_MS * sr / 1000.0) as usize);
    let a_at = (-1.0 / (COMP_ATAQUE_MS * sr / 1000.0)).exp();
    let a_rel = (-1.0 / (COMP_RELEASE_MS * sr / 1000.0)).exp();
    let mut estado = 0.0;
    let mut reducao = vec![0.0; n];
    for i in 0..n {
        // +3 dB: RMS de ruído estéreo na escala de pico de senoide
        // (a média por diferença de somas pode sair um tiquinho negativa: nunca abaixo de 0)
        let nivel = 10.0 * (rms[i].max(0.0) + 1e-20).log10() + 3.0;
        let acima = nivel - limiar;
        let alvo = if 2.0 * acima < -joelho {
            0.0
        } else if 2.0 * acima > joelho {
            acima * (1.0 / razao - 1.0)
        } else {
            (1.0 / razao - 1.0) * (acima + joelho / 2.0).powi(2) / (2.0 * joelho)
        };
        let coef = if alvo < estado { a_at } else { a_rel };
        estado = coef * estado + (1.0 - coef) * alvo;
        reducao[i] = estado;
    }
    for c in canais.iter_mut() {
        for (v, g) in c.iter_mut().zip(&reducao) {
            *v *= 10f64.powf(g / 20.0);
        }
    }
    reducao
}

/// Saturação suave: ganho unitário em sinal baixo.
pub fn saturar(canais: &mut [Vec<f64>], d: f64) {
    for c in canais.iter_mut() {
        for v in c.iter_mut() {
            *v = (d * 3.0 * *v).tanh() / (d * 3.0);
        }
    }
}

/// Limitador com lookahead de 1 ms e detecção de pico verdadeiro (oversampling 4×).
/// Devolve o ganho aplicado (linear, por amostra).
pub fn limitar(canais: &mut [Vec<f64>], sr: f64, teto_db: f64, release_ms: f64) -> Vec<f64> {
    let teto = db_para_lin(teto_db);
    let la = (LOOKAHEAD_MS * sr / 1000.0) as usize;
    let picos = picos_por_amostra(canais);
    let mut req: Vec<f64> = picos.iter().map(|p| (teto / p.max(1e-12)).min(1.0)).collect();
    req.extend(std::iter::repeat(1.0).take(la));
    // o ganho já começa a descer `la` amostras antes do pico
    let req = minimo_deslizante(&req, la + 1);
    let a_rel = (-1.0 / (release_ms * sr / 1000.0)).exp();
    let mut g = Vec::with_capacity(req.len());
    let mut estado = 1.0;
    for &r in &req {
        if r < estado {
            estado = r;
        } else {
            estado = a_rel * estado + (1.0 - a_rel) * r;
        }
        g.push(estado);
    }
    // suaviza a descida (média das próximas `la` + 1 amostras), sem deixar o ganho subir
    let mut ext = g.clone();
    ext.extend(std::iter::repeat(1.0).take(la));
    let media = media_movel(&ext, la + 1);
    for (i, gi) in g.iter_mut().enumerate() {
        *gi = gi.min(media[i + la]);
    }
    for c in canais.iter_mut() {
        for (v, gi) in c.iter_mut().zip(&g) {
            *v *= gi;
        }
    }
    g
}

/// Medidas da dinâmica, para a janela.
#[derive(Clone, Debug, Default)]
pub struct MedidasDinamica {
    /// maior redução do compressor, dB (≤ 0)
    pub compressor_max_db: f64,
    /// maior redução do limitador, dB (≤ 0)
    pub limitador_max_db: f64,
}

/// Cadeia de dinâmica de um caráter, para um alvo de loudness e um teto de true peak.
pub fn processar(canais: &mut [Vec<f64>], sr: f64, carater: Carater, alvo_lufs: f64, teto_db: f64) -> MedidasDinamica {
    let cfg = &CARATERES[carater as usize];
    let mut med = MedidasDinamica::default();
    if let Some(c) = &cfg.compressor {
        // o limiar acompanha o alvo (−16,5 dB vale para −14 LUFS)
        let limiar = c.limiar_db + (alvo_lufs - MODOS[Modo::Musica as usize].alvo_lufs);
        let r = comprimir(canais, sr, limiar, c.razao, c.joelho_db);
        med.compressor_max_db = r.iter().cloned().fold(0.0, f64::min);
    }
    if cfg.saturacao > 0.0 {
        saturar(canais, cfg.saturacao);
    }
    let g = limitar(canais, sr, teto_db, cfg.limitador_release_ms);
    med.limitador_max_db = 20.0 * g.iter().cloned().fold(1.0, f64::min).max(1e-12).log10();
    med
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::true_peak::maximo_db;

    #[test]
    fn limitador_respeita_o_teto() {
        let mut x = vec![(0..48000).map(|i| 1.5 * (i as f64 * 0.37).sin() * ((i / 4000) % 2) as f64).collect::<Vec<_>>()];
        limitar(&mut x, 48000.0, -1.0, 50.0);
        let tp = maximo_db(&x);
        assert!(tp <= -0.95, "{tp}");
    }

    #[test]
    fn compressor_nao_mexe_abaixo_do_joelho() {
        let mut x = vec![vec![0.001; 48000]];
        let r = comprimir(&mut x, 48000.0, -16.5, 1.8, 6.0);
        assert!(r.iter().all(|&v| v == 0.0));
    }
}
