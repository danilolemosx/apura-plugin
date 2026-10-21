//! Apura: master de clip inteiro para entrega (música, web, TV).
//!
//! Porto em Rust do protótipo `referencia/prototipo.py`, que foi ajustado por medição
//! contra o Mastering Assistant do Logic Pro (ver `referencia/ESPECIFICACAO.md`).
//! Os testes em `tests/paridade.rs` conferem amostra a amostra contra o protótipo.

pub mod analise;
pub mod cadeia;
pub mod dinamica;
pub mod eq;
pub mod eq4;
pub mod ffi;
pub mod loudness;
pub mod modos;
pub mod tempo_real;
pub mod true_peak;
pub mod util;

pub use analise::{analisar, Analise, BANDAS};
pub use cadeia::{masterizar, masterizar_com, original_nivelado, pre_processar, processar_com_ganho, Opcoes, Resultado};
pub use loudness::FiltroK;
pub use modos::{Ajustes, Carater, Modo, TipoEq};
