// Interface em C do motor (Rust): ver motor/src/ffi.rs, que tem que bater com isto.

#pragma once

#include <cstdint>

extern "C"
{
    enum : int32_t
    {
        APURA_OK = 0,
        APURA_ERRO_ARGUMENTOS = -1,
        APURA_ERRO_INTERNO = -2
    };

    // --- clip inteiro (AudioSuite) ------------------------------------------------------

    struct ApuraAjustes
    {
        float alvo_lufs;
        float teto_db;
        int32_t curva;           // 0 = música, 1 = voz (só sem eq_bandas)
        float eq_quantidade;     // 0 a 1 (só sem eq_bandas)
        float passa_altas_hz;    // 0 = desligado
        int32_t width_auto;      // 1 = largura sugerida pela análise
        float width_db;
        int32_t tem_teto_de_ganho;
        float ganho_max_db;
        int32_t carater;         // 0 CLEAN, 1 VALVE, 2 PUNCH, 3 TRANSPARENT
        int32_t comparar;        // 1 = o original só com ganho até o alvo
        int32_t medir_faixa;     // 1 = medir a LRA
        int32_t eq_bandas;       // 1 = EQ de 4 pontos
        float bandas_freq[4];
        float bandas_ganho[4];
    };

    struct ApuraResultado
    {
        float lufs_entrada, lufs_saida;
        float tp_entrada, tp_saida;
        float lra_entrada, lra_saida;
        float ganho_db;
        int32_t no_teto_de_ganho;
        float compressor_max_db, limitador_max_db;
        float side_db, side_sugerido_db, sm_entrada;
        float correlacao_entrada, correlacao_saida;
        float espectro_entrada_db[30];
        float espectro_saida_db[30];
        float eq_db[30];
        float curva_musica_db[30];
        float curva_voz_db[30];
    };

    // entradas e saídas podem ser o mesmo buffer
    int32_t apura_processar (const float* const* entradas, float* const* saidas, uint32_t n_canais, uint64_t n,
                             double sr, const ApuraAjustes* aj, ApuraResultado* res);
    int32_t apura_analisar (const float* const* entradas, uint32_t n_canais, uint64_t n, double sr,
                            const ApuraAjustes* aj, ApuraResultado* res);
    float apura_banda_hz (uint32_t i);

    // MATCH: os 4 pontos que melhor desenham a curva (30 bandas, dB)
    int32_t apura_match (const float* curva, double sr, float* freq, float* ganho);

    // --- tempo real (insert) ------------------------------------------------------------

    struct ApuraAjustesRT
    {
        float alvo_lufs;
        float teto_db;
        int32_t carater;
        float bandas_freq[4];
        float bandas_ganho[4];
        float passa_altas_hz;
        float width_db;
        float ganho_db;
        int32_t tem_teto_de_ganho;
        float ganho_max_db;
        int32_t comparar;
        float ganho_comparar_db;
        int32_t escutando;
    };

    struct ApuraMedidasRT
    {
        float momentaneo, curto, integrado;
        float pico_db;
        float limitador_db, compressor_db;
        float espectro_entrada[30];
        float espectro_saida[30];
        float escutando_s;
        float lufs_entrada_escuta, lufs_pre_escuta;
    };

    struct ApuraAprendizado
    {
        float segundos;
        float lufs_entrada, lufs_pre, lufs_saida;
        float sm_db;
        float ganho_sugerido_db;
        float espectro_db[30];
        float curva_musica[30];
        float curva_voz[30];
    };

    struct ApuraRTC;
    ApuraRTC* apura_rt_novo (uint32_t n_canais, double sr);
    void apura_rt_liberar (ApuraRTC* h);
    uint32_t apura_rt_latencia (const ApuraRTC* h);
    void apura_rt_zerar_integrado (ApuraRTC* h);
    // entradas e saídas têm que ser buffers DIFERENTES
    int32_t apura_rt_processar (ApuraRTC* h, const float* const* entradas, float* const* saidas, uint32_t n_canais,
                                uint32_t n, const ApuraAjustesRT* aj, ApuraMedidasRT* med);
    // 0 = há aprendizado (o último LISTEN); 1 = ainda não
    int32_t apura_rt_aprendizado (const ApuraRTC* h, ApuraAprendizado* saida);
}
