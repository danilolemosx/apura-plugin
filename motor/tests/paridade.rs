//! Confere o motor contra o protótipo Python, caso a caso.
//!
//! Os casos ficam em `tests/ref/` (fora do git) e são gerados por
//! `referencia/gerar_referencias.py`. Sem eles, o teste avisa e passa.
//!
//!   cargo test --release --test paridade -- --nocapture

use apura_motor::cadeia::{masterizar_com, pre_processar, processar_com_ganho, Opcoes};
use apura_motor::loudness::{integrado_com, FiltroK};
use apura_motor::{analisar, Ajustes, Carater, Modo};
use std::path::{Path, PathBuf};

fn ler_wav(p: &Path) -> (Vec<Vec<f64>>, f64) {
    let mut r = hound::WavReader::open(p).expect("wav");
    let spec = r.spec();
    let nc = spec.channels as usize;
    let amostras: Vec<f32> = r.samples::<f32>().map(|s| s.expect("amostra")).collect();
    let n = amostras.len() / nc;
    let canais = (0..nc).map(|c| (0..n).map(|i| amostras[i * nc + c] as f64).collect()).collect();
    (canais, spec.sample_rate as f64)
}

fn modo(nome: &str) -> Modo {
    match nome {
        "musica" => Modo::Musica,
        "web" => Modo::Web,
        "tv" => Modo::Tv,
        "tv_eua" => Modo::TvEua,
        _ => panic!("modo {nome}"),
    }
}

fn carater(nome: &str) -> Carater {
    match nome {
        "CLEAN" => Carater::Clean,
        "VALVE" => Carater::Valve,
        "PUNCH" => Carater::Punch,
        "TRANSPARENT" => Carater::Transparent,
        _ => panic!("caráter {nome}"),
    }
}

fn casos() -> Vec<PathBuf> {
    let raiz = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ref");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&raiz)
        .map(|it| it.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.join("dados.json").exists()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn igual_ao_prototipo() {
    let casos = casos();
    if casos.is_empty() {
        eprintln!("sem casos em tests/ref: rode referencia/gerar_referencias.py");
        return;
    }
    let op = Opcoes { filtro_k: FiltroK::Pyloudnorm, medir_faixa: false };
    let mut falhas = Vec::new();
    for pasta in casos {
        let nome = pasta.file_name().unwrap().to_string_lossy().to_string();
        let d: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(pasta.join("dados.json")).unwrap()).unwrap();
        let (x, sr) = ler_wav(&pasta.join("entrada.wav"));
        let (esperado, _) = ler_wav(&pasta.join("saida.wav"));
        let (m, c) = (modo(d["modo"].as_str().unwrap()), carater(d["carater"].as_str().unwrap()));

        // 1. análise, com os ajustes do modo
        let aj = Ajustes::de_modo(m, c);
        let a = analisar(&x, sr);
        let (eq_db, side_db) = (a.eq_final(&aj), a.side_final(&aj));
        let eq_ref: Vec<f64> = d["eq_db"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let dif_eq = eq_db.iter().zip(&eq_ref).map(|(p, q)| (p - q).abs()).fold(0.0, f64::max);
        let dif_side = (side_db - d["side_db"].as_f64().unwrap()).abs();
        let dif_lufs_in = (integrado_com(&x, sr, FiltroK::Pyloudnorm) - d["lufs_entrada"].as_f64().unwrap()).abs();

        // 2. cadeia com o mesmo ganho do protótipo: tem que bater amostra a amostra
        let pre = pre_processar(&x, sr, &eq_db, side_db);
        let (y, _) = processar_com_ganho(&pre, sr, &aj, d["ganho_db"].as_f64().unwrap());
        let (mut dif_max, mut ref_max) = (0.0f64, 0.0f64);
        for (cy, ce) in y.iter().zip(&esperado) {
            for (v, e) in cy.iter().zip(ce) {
                dif_max = dif_max.max((v - e).abs());
                ref_max = ref_max.max(e.abs());
            }
        }
        let dif_db = 20.0 * (dif_max / ref_max).max(1e-30).log10();

        // 3. cadeia completa, com o ganho achado pelo próprio motor
        let (_, r) = masterizar_com(&x, sr, a, &aj, op);
        let dif_ganho = (r.ganho_db - d["ganho_db"].as_f64().unwrap()).abs();
        let dif_lufs = (r.lufs_saida - d["lufs_saida"].as_f64().unwrap()).abs();

        println!(
            "{nome:24} EQ {dif_eq:.1e} dB | side {dif_side:.1e} | LUFS in {dif_lufs_in:.1e} | amostras {dif_db:6.1} dB | ganho {dif_ganho:.3} dB | LUFS out {dif_lufs:.3}"
        );
        if dif_eq > 1e-6 || dif_side > 1e-6 || dif_lufs_in > 1e-6 || dif_db > -90.0 || dif_ganho > 0.02 || dif_lufs > 0.03 {
            falhas.push(nome);
        }
    }
    assert!(falhas.is_empty(), "diferente do protótipo: {falhas:?}");
}
