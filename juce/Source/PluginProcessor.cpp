#include "PluginProcessor.h"

#if ! APURA_SEM_JANELA
 #include "PluginEditor.h"
#endif

#include <cmath>

using namespace juce;

// alvo e teto de cada destino (MUSIC, INTERNET, TV); ATSC = TV com −24 / −2 (ADVANCED)
static constexpr float alvos[] = { -14.0f, -16.0f, -23.0f };
static constexpr float tetos[] = { -1.0f, -1.0f, -1.0f };
static constexpr float tetoDeGanhoVideo = 15.0f;
// pontos da EQ: faixa, centro da escala e posição inicial
struct FaixaPonto { float de, ate, centro, padrao; };
static constexpr FaixaPonto faixasPontos[4] = {
    { 20.0f, 500.0f, 100.0f, 80.0f },
    { 100.0f, 2000.0f, 450.0f, 300.0f },
    { 1000.0f, 10000.0f, 3160.0f, 2500.0f },
    { 3000.0f, 20000.0f, 7750.0f, 10000.0f },
};
// clips até ~20 min estéreo a 48 kHz ficam guardados para a pré-escuta do AudioSuite
static constexpr int64 limiteCache = (int64) 20 * 60 * 48000 * 2;

AudioProcessorValueTreeState::ParameterLayout ApuraProcessor::criarParametros()
{
    using F = AudioParameterFloat;
    using A = AudioParameterFloatAttributes;
    std::vector<std::unique_ptr<RangedAudioParameter>> p;

    p.push_back (std::make_unique<AudioParameterChoice> (ParameterID { ID::destination, 1 }, "Destination",
                                                         StringArray { "MUSIC", "INTERNET", "TV" }, Destino::tv));
    p.push_back (std::make_unique<AudioParameterChoice> (ParameterID { ID::character, 1 }, "Character",
                                                         StringArray { "TRANSPARENT", "DENSE", "WARM" }, Carater::transparent));
    p.push_back (std::make_unique<F> (ParameterID { ID::target, 1 }, "Target", NormalisableRange<float> (-31.0f, -9.0f, 0.5f), -23.0f,
                                      A().withLabel ("LUFS")));
    p.push_back (std::make_unique<F> (ParameterID { ID::ceiling, 1 }, "Ceiling", NormalisableRange<float> (-3.0f, 0.0f, 0.1f), -1.0f,
                                      A().withLabel ("dBTP")));
    p.push_back (std::make_unique<F> (ParameterID { ID::highPass, 1 }, "High-Pass", NormalisableRange<float> (0.0f, 120.0f, 10.0f), 0.0f,
                                      A().withLabel ("Hz").withStringFromValueFunction ([] (float v, int)
                                      {
                                          return v < 30.0f ? String ("OFF") : String (roundToInt (v)) + " Hz";
                                      })));
    p.push_back (std::make_unique<F> (ParameterID { ID::width, 1 }, "Width", NormalisableRange<float> (-6.0f, 6.0f, 0.5f), 0.0f,
                                      A().withLabel ("dB")));
    p.push_back (std::make_unique<F> (ParameterID { ID::gain, 1 }, "Gain", NormalisableRange<float> (-30.0f, 30.0f, 0.01f), 0.0f,
                                      A().withLabel ("dB")));
    p.push_back (std::make_unique<AudioParameterBool> (ParameterID { ID::listen, 1 }, "Listen", false,
                                                       AudioParameterBoolAttributes().withAutomatable (false)));
    p.push_back (std::make_unique<AudioParameterBool> (ParameterID { ID::compare, 1 }, "Bypass Same Volume", false));

    static const char* const nomes[4] = { "Low", "Low-Mid", "High-Mid", "High" };
    for (int k = 0; k < 4; ++k)
    {
        NormalisableRange<float> faixa (faixasPontos[k].de, faixasPontos[k].ate);
        faixa.setSkewForCentre (faixasPontos[k].centro);
        p.push_back (std::make_unique<F> (ParameterID { ID::freq[k], 1 }, String (nomes[k]) + " Freq", faixa, faixasPontos[k].padrao,
                                          A().withLabel ("Hz")));
        p.push_back (std::make_unique<F> (ParameterID { ID::ganho[k], 1 }, String (nomes[k]) + " Gain", NormalisableRange<float> (-12.0f, 12.0f, 0.1f), 0.0f,
                                          A().withLabel ("dB")));
    }
    return { p.begin(), p.end() };
}

ApuraProcessor::ApuraProcessor()
    : AudioProcessor (BusesProperties()
                          .withInput ("Input", AudioChannelSet::stereo(), true)
                          .withOutput ("Output", AudioChannelSet::stereo(), true)),
      state (*this, nullptr, "APURA", criarParametros())
{
    pDestination = state.getRawParameterValue (ID::destination);
    pCharacter = state.getRawParameterValue (ID::character);
    pTarget = state.getRawParameterValue (ID::target);
    pCeiling = state.getRawParameterValue (ID::ceiling);
    pHighPass = state.getRawParameterValue (ID::highPass);
    pWidth = state.getRawParameterValue (ID::width);
    pGain = state.getRawParameterValue (ID::gain);
    pListen = state.getRawParameterValue (ID::listen);
    pCompare = state.getRawParameterValue (ID::compare);
    for (int k = 0; k < 4; ++k)
    {
        pFreq[(size_t) k] = state.getRawParameterValue (ID::freq[k]);
        pGanho[(size_t) k] = state.getRawParameterValue (ID::ganho[k]);
    }
}

ApuraProcessor::~ApuraProcessor()
{
    cancelPendingUpdate();
    releaseResources();
}

bool ApuraProcessor::isBusesLayoutSupported (const BusesLayout& layouts) const
{
    const auto entrada = layouts.getMainInputChannelSet();
    const auto saida = layouts.getMainOutputChannelSet();
    if (entrada != saida)
        return false;
    return saida == AudioChannelSet::mono() || saida == AudioChannelSet::stereo();
}

// --- ao vivo ----------------------------------------------------------------------------

void ApuraProcessor::prepareToPlay (double sampleRate, int samplesPerBlock)
{
    releaseResources();
    const int nc = jmax (1, getTotalNumOutputChannels());
    // o motor aloca tudo aqui, fora da thread de áudio
    motorRT = apura_rt_novo ((uint32_t) nc, sampleRate);
    copiaEntrada.setSize (nc, jmax (samplesPerBlock, 4096));
    taxaRT = sampleRate;
    canaisRT = nc;
    escutavaAntes = false;
    setLatencySamples (motorRT != nullptr ? (int) apura_rt_latencia (motorRT) : 0);
}

void ApuraProcessor::releaseResources()
{
    if (motorRT != nullptr)
    {
        apura_rt_liberar (motorRT);
        motorRT = nullptr;
    }
}

double ApuraProcessor::getTailLengthSeconds() const
{
    return taxaRT > 0.0 ? getLatencySamples() / taxaRT : 0.0;
}

double ApuraProcessor::latenciaMs() const
{
    return taxaRT > 0.0 ? 1000.0 * getLatencySamples() / taxaRT : 0.0;
}

ApuraAjustesRT ApuraProcessor::lerAjustesRT() const
{
    const int destino = jlimit (0, 2, (int) pDestination->load());
    const int carater = jlimit (0, 2, (int) pCharacter->load());
    const float passaAltas = pHighPass->load();
    const auto aprendido = this->aprendido();

    ApuraAjustesRT a {};
    a.alvo_lufs = pTarget->load();
    a.teto_db = pCeiling->load();
    // TRANSPARENT = PUNCH/TRANSPARENT do Logic, DENSE = CLEAN, WARM = VALVE (motor/src/modos.rs)
    a.carater = carater == Carater::dense ? 0 : (carater == Carater::warm ? 1 : 3);
    for (size_t k = 0; k < 4; ++k)
    {
        a.bandas_freq[k] = pFreq[k]->load();
        a.bandas_ganho[k] = pGanho[k]->load();
    }
    a.passa_altas_hz = passaAltas >= 30.0f ? passaAltas : 0.0f;
    a.width_db = pWidth->load();
    // o GAIN vale para o alvo da hora do LISTEN; mudou o destino, o ganho acompanha
    a.ganho_db = pGain->load() + (aprendido.valido ? a.alvo_lufs - aprendido.alvo : 0.0f);
    a.tem_teto_de_ganho = destino == Destino::musica ? 0 : 1;
    a.ganho_max_db = tetoDeGanhoVideo;
    a.comparar = pCompare->load() > 0.5f ? 1 : 0;
    // BYPASS · SAME VOLUME: o original levado ao alvo pelo que o LISTEN mediu
    a.ganho_comparar_db = aprendido.valido && std::isfinite (aprendido.lufsEntrada) ? a.alvo_lufs - aprendido.lufsEntrada : 0.0f;
    a.escutando = pListen->load() > 0.5f ? 1 : 0;
    return a;
}

void ApuraProcessor::processBlock (AudioBuffer<float>& buffer, MidiBuffer&)
{
    ScopedNoDenormals semDenormais;
    const int nc = jmin (getTotalNumInputChannels(), getTotalNumOutputChannels(), buffer.getNumChannels());
    const int n = buffer.getNumSamples();
    if (motorRT == nullptr || nc != canaisRT || n <= 0)
        return; // passa como veio

    const auto aj = lerAjustesRT();
    ApuraMedidasRT med {};
    const int bloco = copiaEntrada.getNumSamples();
    for (int inicio = 0; inicio < n; inicio += bloco)
    {
        const int k = jmin (bloco, n - inicio);
        const float* entradas[8] {};
        float* saidas[8] {};
        for (int c = 0; c < nc && c < 8; ++c)
        {
            copiaEntrada.copyFrom (c, 0, buffer, c, inicio, k);
            entradas[c] = copiaEntrada.getReadPointer (c);
            saidas[c] = buffer.getWritePointer (c, inicio);
        }
        if (apura_rt_processar (motorRT, entradas, saidas, (uint32_t) nc, (uint32_t) k, &aj, &med) != APURA_OK)
            return;
    }

    // fim do LISTEN: guarda o que aprendeu e ajusta o ganho (na thread de mensagens)
    const bool escutando = aj.escutando != 0;
    if (escutavaAntes && ! escutando)
    {
        ApuraAprendizado a {};
        if (apura_rt_aprendizado (motorRT, &a) == APURA_OK)
        {
            Aprendido novo;
            novo.valido = true;
            novo.segundos = a.segundos;
            novo.lufsEntrada = a.lufs_entrada;
            novo.lufsPre = a.lufs_pre;
            novo.lufsSaida = a.lufs_saida;
            novo.ganhoSugerido = a.ganho_sugerido_db;
            novo.alvo = aj.alvo_lufs;
            std::copy (std::begin (a.curva_musica), std::end (a.curva_musica), novo.curvaMusica.begin());
            std::copy (std::begin (a.curva_voz), std::end (a.curva_voz), novo.curvaVoz.begin());
            {
                const SpinLock::ScopedLockType t (travaAprendido);
                ultimoAprendido = novo;
            }
            aprendizadoNovo = true;
            triggerAsyncUpdate();
        }
    }
    escutavaAntes = escutando;

    {
        const SpinLock::ScopedTryLockType t (travaAoVivo);
        if (t.isLocked())
            aoVivo = med;
    }
    ++versaoAoVivo;
}

static String serializar (const ApuraProcessor::Aprendido& a)
{
    StringArray s { String (a.segundos), String (a.lufsEntrada), String (a.lufsPre), String (a.lufsSaida), String (a.ganhoSugerido),
                    String (a.alvo) };
    for (auto v : a.curvaMusica) s.add (String (v, 3));
    for (auto v : a.curvaVoz) s.add (String (v, 3));
    return s.joinIntoString (" ");
}

static ApuraProcessor::Aprendido desserializar (const String& texto)
{
    ApuraProcessor::Aprendido a;
    const auto v = StringArray::fromTokens (texto, " ", {});
    if (v.size() != 6 + 60)
        return a;
    a.valido = true;
    a.segundos = v[0].getFloatValue();
    a.lufsEntrada = v[1].getFloatValue();
    a.lufsPre = v[2].getFloatValue();
    a.lufsSaida = v[3].getFloatValue();
    a.ganhoSugerido = v[4].getFloatValue();
    a.alvo = v[5].getFloatValue();
    for (int k = 0; k < 30; ++k)
    {
        a.curvaMusica[(size_t) k] = v[6 + k].getFloatValue();
        a.curvaVoz[(size_t) k] = v[36 + k].getFloatValue();
    }
    return a;
}

void ApuraProcessor::handleAsyncUpdate()
{
    if (! aprendizadoNovo.exchange (false))
        return;
    const auto a = aprendido();
    if (std::isfinite (a.ganhoSugerido))
        definirParametro (ID::gain, a.ganhoSugerido);
    state.state.setProperty ("aprendido", serializar (a), nullptr);
    ++versaoAprendido;
}

void ApuraProcessor::definirParametro (const char* id, float valor)
{
    if (auto* p = state.getParameter (id))
    {
        p->beginChangeGesture();
        p->setValueNotifyingHost (p->convertTo0to1 (valor));
        p->endChangeGesture();
    }
}

ApuraMedidasRT ApuraProcessor::medidasAoVivo() const
{
    const SpinLock::ScopedLockType t (travaAoVivo);
    return aoVivo;
}

ApuraProcessor::Aprendido ApuraProcessor::aprendido() const
{
    const SpinLock::ScopedLockType t (travaAprendido);
    return ultimoAprendido;
}

bool ApuraProcessor::estaNoAudioSuite()
{
    return PluginHostType::isInAAXAudioSuite (*this);
}

bool ApuraProcessor::aplicarMatch()
{
    // a análise mais à mão: o clip do AudioSuite, ou o que o LISTEN aprendeu
    const bool musica = (int) pDestination->load() == Destino::musica;
    std::array<float, 30> curva {};
    double taxa = 0.0;
    const auto m = ultimaMedida();
    const auto a = aprendido();
    const bool temClip = versaoMedida.load() > 0 && m.erro == 0 && m.canais > 0;
    if (temClip && (estaNoAudioSuite() || ! a.valido))
    {
        const auto* origem = musica ? m.resultado.curva_musica_db : m.resultado.curva_voz_db;
        std::copy (origem, origem + 30, curva.begin());
        taxa = m.taxa;
    }
    else if (a.valido)
    {
        curva = musica ? a.curvaMusica : a.curvaVoz;
        taxa = taxaRT;
    }
    else
        return false;

    float freq[4] {}, ganho[4] {};
    if (apura_match (curva.data(), taxa > 0.0 ? taxa : 48000.0, freq, ganho) != APURA_OK)
        return false;
    for (int k = 0; k < 4; ++k)
    {
        definirParametro (ID::freq[k], jlimit (faixasPontos[k].de, faixasPontos[k].ate, freq[k]));
        definirParametro (ID::ganho[k], ganho[k]);
    }
    return true;
}

void ApuraProcessor::escolherDestino (int destino)
{
    destino = jlimit (0, 2, destino);
    if (auto* p = state.getParameter (ID::destination))
    {
        p->beginChangeGesture();
        p->setValueNotifyingHost (p->convertTo0to1 ((float) destino));
        p->endChangeGesture();
    }
    definirParametro (ID::target, alvos[destino]);
    definirParametro (ID::ceiling, tetos[destino]);
}

// --- AudioSuite (clip inteiro) -----------------------------------------------------------

ApuraAjustes ApuraProcessor::lerAjustes() const
{
    const auto rt = lerAjustesRT();
    const int destino = jlimit (0, 2, (int) pDestination->load());

    ApuraAjustes a {};
    a.alvo_lufs = rt.alvo_lufs;
    a.teto_db = rt.teto_db;
    a.curva = destino == Destino::musica ? 0 : 1;
    a.eq_quantidade = 0.0f;
    a.passa_altas_hz = rt.passa_altas_hz;
    a.width_auto = 0;
    a.width_db = rt.width_db;
    a.tem_teto_de_ganho = rt.tem_teto_de_ganho;
    a.ganho_max_db = rt.ganho_max_db;
    a.carater = rt.carater;
    a.comparar = rt.comparar;
    a.medir_faixa = 1;
    a.eq_bandas = 1;
    for (int k = 0; k < 4; ++k)
    {
        a.bandas_freq[k] = rt.bandas_freq[k];
        a.bandas_ganho[k] = rt.bandas_ganho[k];
    }
    return a;
}

// FNV-1a sobre os ajustes, o formato e uma amostra espaçada do áudio
static uint64 assinar (const ApuraAjustes& aj, const AudioBuffer<float>& audio, double sr)
{
    uint64 h = 1469598103934665603ull;
    const auto misturar = [&h] (const void* dados, size_t n)
    {
        const auto* b = static_cast<const uint8*> (dados);
        for (size_t i = 0; i < n; ++i)
            h = (h ^ b[i]) * 1099511628211ull;
    };
    misturar (&aj, sizeof (aj));
    const int nc = audio.getNumChannels(), n = audio.getNumSamples();
    misturar (&nc, sizeof (nc));
    misturar (&n, sizeof (n));
    misturar (&sr, sizeof (sr));
    for (int c = 0; c < nc; ++c)
        for (int i = 0; i < n; i += 211)
            misturar (audio.getReadPointer (c) + i, sizeof (float));
    return h;
}

bool ApuraProcessor::processWholeClip (AudioBuffer<float>& audio, double sampleRate)
{
    const int nc = audio.getNumChannels(), n = audio.getNumSamples();
    if (nc == 0 || n == 0)
        return true;

    const auto ajustes = lerAjustes();
    Medida m;
    m.ajustes = ajustes;
    m.canais = nc;
    m.taxa = sampleRate;
    m.segundos = n / sampleRate;

    if (nc > 2)
    {
        // v1: só mono e estéreo (5.1 pede pesos de canal diferentes na medição)
        m.erro = APURA_ERRO_ARGUMENTOS;
        { const SpinLock::ScopedLockType t (travaMedida); medida = m; }
        ++versaoMedida;
        return false;
    }

    const auto assinatura = assinar (ajustes, audio, sampleRate);
    {
        const ScopedLock t (travaCache);
        if (assinatura == cacheAssinatura && cacheSaida.getNumChannels() == nc && cacheSaida.getNumSamples() == n)
        {
            for (int c = 0; c < nc; ++c)
                audio.copyFrom (c, 0, cacheSaida, c, 0, n);
            return true;
        }
    }

    processando = true;
    const auto inicio = Time::getMillisecondCounterHiRes();
    const int r = apura_processar (audio.getArrayOfReadPointers(), audio.getArrayOfWritePointers(), (uint32_t) nc,
                                   (uint64_t) n, sampleRate, &ajustes, &m.resultado);
    m.tempoProcesso = (Time::getMillisecondCounterHiRes() - inicio) / 1000.0;
    m.erro = r;
    processando = false;

    { const SpinLock::ScopedLockType t (travaMedida); medida = m; }
    ++versaoMedida;

    if (r == APURA_OK && (int64) n * nc <= limiteCache)
    {
        const ScopedLock t (travaCache);
        cacheSaida.makeCopyOf (audio);
        cacheAssinatura = assinatura;
    }
    return r == APURA_OK;
}

ApuraProcessor::Medida ApuraProcessor::ultimaMedida() const
{
    const SpinLock::ScopedLockType t (travaMedida);
    return medida;
}

// "18" ou "18.5"
static String valorAbsoluto (float v)
{
    const bool inteiro = std::abs (v - std::round (v)) < 0.01f;
    return String (std::abs (v), inteiro ? 0 : 1);
}

String ApuraProcessor::getWholeClipNameSuffix() const
{
    // MUS14, WEB16, TV23, ATSC24; alvo fora do padrão: LUFS18 / LUFS18.5
    const int destino = jlimit (0, 2, (int) pDestination->load());
    const float alvo = pTarget->load(), teto = pCeiling->load();
    if (pCompare->load() > 0.5f)
        return "LVL" + valorAbsoluto (alvo);
    static const char* const codigos[] = { "MUS", "WEB", "TV" };
    if (std::abs (alvo - alvos[destino]) < 0.01f && std::abs (teto - tetos[destino]) < 0.01f)
        return String (codigos[destino]) + valorAbsoluto (alvo);
    if (std::abs (alvo + 24.0f) < 0.01f && std::abs (teto + 2.0f) < 0.01f)
        return "ATSC24";
    return "LUFS" + valorAbsoluto (alvo);
}

// --- estado -------------------------------------------------------------------------------

void ApuraProcessor::getStateInformation (MemoryBlock& destData)
{
    if (auto xml = state.copyState().createXml())
        copyXmlToBinary (*xml, destData);
}

void ApuraProcessor::setStateInformation (const void* data, int sizeInBytes)
{
    if (auto xml = getXmlFromBinary (data, sizeInBytes))
        if (xml->hasTagName (state.state.getType()))
        {
            state.replaceState (ValueTree::fromXml (*xml));
            const auto a = desserializar (state.state.getProperty ("aprendido").toString());
            { const SpinLock::ScopedLockType t (travaAprendido); ultimoAprendido = a; }
            ++versaoAprendido;
        }
}

bool ApuraProcessor::hasEditor() const
{
   #if APURA_SEM_JANELA
    return false;
   #else
    return true;
   #endif
}

AudioProcessorEditor* ApuraProcessor::createEditor()
{
   #if APURA_SEM_JANELA
    return nullptr;
   #else
    return new ApuraEditor (*this);
   #endif
}

AudioProcessor* JUCE_CALLTYPE createPluginFilter()
{
    return new ApuraProcessor();
}
