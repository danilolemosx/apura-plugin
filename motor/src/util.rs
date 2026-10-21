//! Utilidades numéricas que imitam o NumPy/SciPy usados no protótipo
//! (`referencia/prototipo.py`), para o resultado bater amostra a amostra.

/// `np.interp`: interpolação linear com os extremos presos.
pub fn interp(x: f64, xp: &[f64], fp: &[f64]) -> f64 {
    let n = xp.len();
    if x < xp[0] {
        return fp[0];
    }
    if x >= xp[n - 1] {
        return fp[n - 1];
    }
    // j com xp[j] <= x < xp[j+1], mesma conta do NumPy
    let mut j = 0;
    while xp[j + 1] <= x {
        j += 1;
    }
    let inclinacao = (fp[j + 1] - fp[j]) / (xp[j + 1] - xp[j]);
    inclinacao * (x - xp[j]) + fp[j]
}

/// Interpolação em escala logarítmica de frequência (o protótipo usa `np.interp(np.log(...))`).
pub fn interp_log(f: f64, freqs: &[f64], valores: &[f64]) -> f64 {
    let xp: Vec<f64> = freqs.iter().map(|v| v.ln()).collect();
    interp(f.ln(), &xp, valores)
}

/// `_media_movel` do protótipo: média dos últimos `n` valores (cumsum do NumPy, sequencial).
/// Nos primeiros `n` pontos, média do que já passou.
pub fn media_movel(v: &[f64], n: usize) -> Vec<f64> {
    let n = n.max(1);
    let mut c = Vec::with_capacity(v.len() + 1);
    c.push(0.0);
    let mut acc = 0.0;
    for &x in v {
        acc += x;
        c.push(acc);
    }
    let mut out = vec![0.0; v.len()];
    for i in 0..v.len() {
        out[i] = if i >= n { (c[i + 1] - c[i + 1 - n]) / n as f64 } else { c[i + 1] / (i + 1) as f64 };
    }
    out
}

/// Mínimo de cada janela `[i, i + largura)` (deque monotônica).
pub fn minimo_deslizante(v: &[f64], largura: usize) -> Vec<f64> {
    use std::collections::VecDeque;
    let n_saida = v.len() + 1 - largura.min(v.len() + 1);
    let mut out = Vec::with_capacity(n_saida);
    let mut dq: VecDeque<usize> = VecDeque::new();
    for i in 0..v.len() {
        while let Some(&b) = dq.back() {
            if v[b] >= v[i] {
                dq.pop_back();
            } else {
                break;
            }
        }
        dq.push_back(i);
        if i + 1 >= largura {
            let inicio = i + 1 - largura;
            while let Some(&f) = dq.front() {
                if f < inicio {
                    dq.pop_front();
                } else {
                    break;
                }
            }
            out.push(v[*dq.front().unwrap()]);
        }
    }
    out
}

/// Bessel modificada de primeira espécie, ordem zero (para a janela de Kaiser).
pub fn bessel_i0(x: f64) -> f64 {
    let mut soma = 1.0;
    let mut termo = 1.0;
    let q = x * x / 4.0;
    let mut k = 1.0;
    loop {
        termo *= q / (k * k);
        soma += termo;
        if termo < soma * 1e-17 {
            break;
        }
        k += 1.0;
    }
    soma
}

/// `np.sinc`: sin(πx)/(πx).
pub fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        let px = std::f64::consts::PI * x;
        px.sin() / px
    }
}

pub fn db_para_lin(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn interp_prende_os_extremos() {
        let xp = [1.0, 2.0, 4.0];
        let fp = [10.0, 20.0, 0.0];
        assert_eq!(interp(0.0, &xp, &fp), 10.0);
        assert_eq!(interp(5.0, &xp, &fp), 0.0);
        assert!((interp(3.0, &xp, &fp) - 10.0).abs() < 1e-12);
    }

    #[test]
    fn media_movel_igual_ao_prototipo() {
        let v = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(media_movel(&v, 2), vec![1.0, 1.5, 2.5, 3.5, 4.5]);
    }

    #[test]
    fn minimo_deslizante_basico() {
        let v = [3.0, 1.0, 4.0, 1.0, 5.0, 9.0];
        assert_eq!(minimo_deslizante(&v, 3), vec![1.0, 1.0, 1.0, 1.0]);
        assert_eq!(minimo_deslizante(&v, 1), v.to_vec());
    }

    #[test]
    fn bessel_conhecido() {
        // I0(5) = 27.239871823604442
        assert!((bessel_i0(5.0) - 27.239871823604442).abs() < 1e-12);
    }
}
