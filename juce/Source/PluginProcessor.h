// Apura: liga os parâmetros ao motor nos dois usos:
//  - insert (ex.: no master do Media Composer): ao vivo; o LISTEN aprende o volume;
//  - AudioSuite: o clip inteiro de uma vez (processWholeClip, remendo do JUCE).

#pragma once

#include <juce_audio_processors/juce_audio_processors.h>

#include "Apura.h"

#include <array>
#include <atomic>

namespace ID
{
    // nomes dos parâmetros (também usados pela janela, app.js)
    inline constexpr const char* destination = "destination";
    inline constexpr const char* character   = "character";
    inline constexpr const char* target      = "target";
    inline constexpr const char* ceiling     = "ceiling";
    inline constexpr const char* highPass    = "highPass";
    inline constexpr const char* width       = "width";
    inline constexpr const char* gain        = "gain";
    inline constexpr const char* listen      = "listen";
    inline constexpr const char* compare     = "compare";
    // os 4 pontos da EQ
    inline constexpr const char* freq[4]  = { "lowFreq", "lowMidFreq", "highMidFreq", "highFreq" };
    inline constexpr const char* ganho[4] = { "lowGain", "lowMidGain", "highMidGain", "highGain" };
}

// destinos e caracteres, na ordem dos parâmetros
namespace Destino { enum { musica, internet, tv }; }
namespace Carater { enum { transparent, dense, warm }; }

class ApuraProcessor : public juce::AudioProcessor, private juce::AsyncUpdater
{
public:
    ApuraProcessor();
    ~ApuraProcessor() override;

    void prepareToPlay (double sampleRate, int samplesPerBlock) override;
    void releaseResources() override;
    bool isBusesLayoutSupported (const BusesLayout& layouts) const override;
    void processBlock (juce::AudioBuffer<float>&, juce::MidiBuffer&) override;
    using AudioProcessor::processBlock;

    juce::AudioProcessorEditor* createEditor() override;
    bool hasEditor() const override;

    const juce::String getName() const override { return "Apura"; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool isMidiEffect() const override { return false; }
    double getTailLengthSeconds() const override;

    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram (int) override {}
    const juce::String getProgramName (int) override { return {}; }
    void changeProgramName (int, const juce::String&) override {}

    void getStateInformation (juce::MemoryBlock& destData) override;
    void setStateInformation (const void* data, int sizeInBytes) override;

    // AudioSuite de clip inteiro (remendo do JUCE, ver juce/patches/)
    bool usesWholeClipAudioSuite() const override { return true; }
    bool processWholeClip (juce::AudioBuffer<float>& audio, double sampleRate) override;
    juce::String getWholeClipNameSuffix() const override;

    ApuraAjustes lerAjustes() const;
    ApuraAjustesRT lerAjustesRT() const;

    /** MATCH: põe os 4 pontos onde a última análise sugere. false = ainda não há análise. */
    bool aplicarMatch();
    /** Destino escolhido na janela: alvo e teto do destino. */
    void escolherDestino (int destino);

    // --- para a janela ---------------------------------------------------------------
    /** Último clip processado no AudioSuite. A versão muda a cada clip. */
    struct Medida
    {
        ApuraResultado resultado {};
        ApuraAjustes ajustes {};
        int canais = 0;
        double taxa = 0.0, segundos = 0.0, tempoProcesso = 0.0;
        int erro = 0;
    };
    Medida ultimaMedida() const;
    std::atomic<int> versaoMedida { 0 };
    std::atomic<bool> processando { false };

    /** Medidas ao vivo (insert). */
    ApuraMedidasRT medidasAoVivo() const;
    std::atomic<int> versaoAoVivo { 0 };

    /** O que o último LISTEN aprendeu (guardado com a sessão). */
    struct Aprendido
    {
        bool valido = false;
        // alvo = TARGET na hora do LISTEN: se o alvo mudar depois, o ganho acompanha
        float segundos = 0, lufsEntrada = 0, lufsPre = 0, lufsSaida = 0, ganhoSugerido = 0, alvo = 0;
        std::array<float, 30> curvaMusica {}, curvaVoz {};
    };
    Aprendido aprendido() const;
    std::atomic<int> versaoAprendido { 0 };

    bool estaNoAudioSuite();
    double latenciaMs() const;

    juce::AudioProcessorValueTreeState state;

private:
    static juce::AudioProcessorValueTreeState::ParameterLayout criarParametros();
    void handleAsyncUpdate() override;
    void definirParametro (const char* id, float valor);

    // AudioSuite
    mutable juce::SpinLock travaMedida;
    Medida medida;
    juce::AudioBuffer<float> cacheSaida;
    juce::uint64 cacheAssinatura = 0;
    juce::CriticalSection travaCache;

    // ao vivo
    ApuraRTC* motorRT = nullptr;
    juce::AudioBuffer<float> copiaEntrada;
    double taxaRT = 0.0;
    int canaisRT = 0;
    bool escutavaAntes = false;
    mutable juce::SpinLock travaAoVivo;
    ApuraMedidasRT aoVivo {};
    mutable juce::SpinLock travaAprendido;
    Aprendido ultimoAprendido;
    std::atomic<bool> aprendizadoNovo { false };
    std::atomic<float> ganhoComparar { 0.0f };

    std::atomic<float>* pDestination; std::atomic<float>* pCharacter; std::atomic<float>* pTarget;
    std::atomic<float>* pCeiling; std::atomic<float>* pHighPass; std::atomic<float>* pWidth;
    std::atomic<float>* pGain; std::atomic<float>* pListen; std::atomic<float>* pCompare;
    std::array<std::atomic<float>*, 4> pFreq {}, pGanho {};

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (ApuraProcessor)
};
