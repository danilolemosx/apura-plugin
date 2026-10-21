// Testes do Apura fora do Avid.
//
//   testa-apura <entrada.wav> <saida.wav> [param=valor ...]
//       chama processWholeClip() com o WAV inteiro, como o AudioSuite faz, grava a saída
//       em float 32 e imprime o resultado. Valores reais dos parâmetros, ex.:
//       destination=2 character=0 target=-23 ceiling=-1 lowMidGain=-2 highPass=60
//       (destination: 0 MUSIC, 1 INTERNET, 2 TV; character: 0 TRANSPARENT, 1 DENSE, 2 WARM)
//
//   testa-apura --ao-vivo <entrada.wav> <saida.wav> [param=valor ...] [--match]
//       como um insert no master: toca o arquivo com LISTEN ligado, desliga (o plugin
//       aprende e ajusta o ganho), opcionalmente aplica o MATCH, toca de novo e grava
//       a segunda passada (já sem o atraso do plugin).
//
//   testa-apura --janela <Apura.vst3> [segundos]
//       abre a janela do plugin por alguns segundos (com APURA_TESTE_JANELA=<arquivo>,
//       a página grava ali o que desenhou)

#include <juce_audio_formats/juce_audio_formats.h>
#include <juce_audio_processors/juce_audio_processors.h>

#include "../Source/PluginProcessor.h"

#include <iostream>

using namespace juce;

static int falha (const String& msg)
{
    std::cerr << "ERRO: " << msg << std::endl;
    return 1;
}

static int abrirJanela (const File& arquivoPlugin, double segundos)
{
    AudioPluginFormatManager formatos;
    formatos.addFormat (new VST3PluginFormat());
    OwnedArray<PluginDescription> descricoes;
    for (auto* f : formatos.getFormats())
        if (f->fileMightContainThisPluginType (arquivoPlugin.getFullPathName()))
            f->findAllTypesForFile (descricoes, arquivoPlugin.getFullPathName());
    if (descricoes.isEmpty())
        return falha ("plugin não encontrado");
    String erro;
    auto plugin = formatos.createPluginInstance (*descricoes[0], 48000.0, 512, erro);
    if (plugin == nullptr)
        return falha ("não consegui abrir o plugin: " + erro);
    std::unique_ptr<AudioProcessorEditor> editor (plugin->createEditorIfNeeded());
    if (editor == nullptr)
        return falha ("o plugin não criou a janela");
    std::cout << "janela " << editor->getWidth() << "x" << editor->getHeight() << std::endl;
    DocumentWindow janela ("Apura (teste)", Colours::black, 0);
    janela.setContentNonOwned (editor.get(), true);
    janela.setTopLeftPosition (80, 80);
    janela.setVisible (true);
    MessageManager::getInstance()->runDispatchLoopUntil ((int) (segundos * 1000));
    janela.clearContentComponent();
    editor.reset();
    return 0;
}

static void definir (ApuraProcessor& apura, const String& id, float valor)
{
    if (auto* p = apura.state.getParameter (id))
        p->setValueNotifyingHost (p->convertTo0to1 (valor));
}

static bool gravar (const File& destino, const AudioBuffer<float>& audio, double taxa)
{
    WavAudioFormat wav;
    destino.deleteFile();
    std::unique_ptr<OutputStream> saida (destino.createOutputStream());
    std::unique_ptr<AudioFormatWriter> escritor (wav.createWriterFor (saida.get(), taxa, (unsigned int) audio.getNumChannels(), 32, {}, 0));
    if (escritor == nullptr)
        return false;
    saida.release();
    return escritor->writeFromAudioSampleBuffer (audio, 0, audio.getNumSamples());
}

static void tocar (ApuraProcessor& apura, const AudioBuffer<float>& entrada, AudioBuffer<float>* saida)
{
    MidiBuffer midi;
    AudioBuffer<float> bloco (entrada.getNumChannels(), 512);
    const int lat = apura.getLatencySamples();
    for (int i = 0; i < entrada.getNumSamples() + lat; i += 512)
    {
        const int k = 512;
        bloco.clear();
        for (int c = 0; c < entrada.getNumChannels(); ++c)
            if (i < entrada.getNumSamples())
                bloco.copyFrom (c, 0, entrada, c, i, jmin (k, entrada.getNumSamples() - i));
        apura.processBlock (bloco, midi);
        if (saida != nullptr)
            for (int c = 0; c < entrada.getNumChannels(); ++c)
                for (int j = 0; j < k; ++j)
                {
                    const int destino = i + j - lat; // sem o atraso do plugin
                    if (destino >= 0 && destino < saida->getNumSamples())
                        saida->setSample (c, destino, bloco.getSample (c, j));
                }
    }
}

static int aoVivo (int argc, char** argv)
{
    AudioFormatManager formatos;
    formatos.registerBasicFormats();
    std::unique_ptr<AudioFormatReader> leitor (formatos.createReaderFor (File (argv[2])));
    if (leitor == nullptr)
        return falha ("não consegui ler " + String (argv[2]));
    AudioBuffer<float> audio ((int) leitor->numChannels, (int) leitor->lengthInSamples);
    leitor->read (&audio, 0, (int) leitor->lengthInSamples, 0, true, true);

    ApuraProcessor apura;
    bool match = false;
    for (int i = 4; i < argc; ++i)
    {
        const String arg (argv[i]);
        if (arg == "--match") { match = true; continue; }
        if (apura.state.getParameter (arg.upToFirstOccurrenceOf ("=", false, false)) == nullptr)
            return falha ("parâmetro desconhecido: " + arg);
        definir (apura, arg.upToFirstOccurrenceOf ("=", false, false), arg.fromFirstOccurrenceOf ("=", false, false).getFloatValue());
    }
    apura.setPlayConfigDetails (audio.getNumChannels(), audio.getNumChannels(), leitor->sampleRate, 512);
    apura.prepareToPlay (leitor->sampleRate, 512);
    std::cout << "atraso " << apura.getLatencySamples() << " amostras (" << String (apura.latenciaMs(), 2) << " ms)\n";

    const auto inicio = Time::getMillisecondCounterHiRes();
    definir (apura, "listen", 1.0f);
    tocar (apura, audio, nullptr);
    definir (apura, "listen", 0.0f);
    AudioBuffer<float> silencio (audio.getNumChannels(), 512);
    silencio.clear();
    tocar (apura, silencio, nullptr);
    MessageManager::getInstance()->runDispatchLoopUntil (100); // o ganho é ajustado na thread de mensagens
    const auto a = apura.aprendido();
    if (! a.valido)
        return falha ("o LISTEN não aprendeu nada");
    std::cout << "LISTEN " << String (a.segundos, 1) << " s: entrada " << String (a.lufsEntrada, 2) << " LUFS, antes do ganho "
              << String (a.lufsPre, 2) << ", ganho sugerido " << String (a.ganhoSugerido, 2) << " dB -> parâmetro gain "
              << String (apura.state.getParameter ("gain")->convertFrom0to1 (apura.state.getParameter ("gain")->getValue()), 2) << " dB\n";
    if (match)
    {
        std::cout << "MATCH " << (apura.aplicarMatch() ? "ok:" : "sem análise") ;
        for (int k = 0; k < 4; ++k)
            std::cout << " " << String (apura.state.getRawParameterValue (ID::freq[k])->load(), 0) << " Hz "
                      << String (apura.state.getRawParameterValue (ID::ganho[k])->load(), 1) << " dB;";
        std::cout << "\n";
    }
    AudioBuffer<float> saida (audio.getNumChannels(), audio.getNumSamples());
    saida.clear();
    tocar (apura, audio, &saida);
    // o loudness da segunda passada, medido pelo motor (BS.1770)
    ApuraAjustes medir {};
    medir.carater = 3;
    ApuraResultado r {};
    apura_analisar (saida.getArrayOfReadPointers(), (uint32_t) saida.getNumChannels(), (uint64_t) saida.getNumSamples(),
                    leitor->sampleRate, &medir, &r);
    std::cout << "segunda passada: " << String (r.lufs_entrada, 2) << " LUFS, true peak " << String (r.tp_entrada, 2) << " dBTP ("
              << String ((Time::getMillisecondCounterHiRes() - inicio) / 1000.0, 1) << " s para "
              << String (3.0 * audio.getNumSamples() / leitor->sampleRate / 60.0, 1) << " min de áudio)\n";
    if (! gravar (File (argv[3]), saida, leitor->sampleRate))
        return falha ("não consegui gravar");
    return 0;
}

static String lista (const float* v, int n)
{
    StringArray s;
    for (int i = 0; i < n; ++i)
        s.add (String (v[i], 2));
    return "[" + s.joinIntoString (", ") + "]";
}

int main (int argc, char** argv)
{
    ScopedJuceInitialiser_GUI juce;
    if (argc >= 4 && String (argv[1]) == "--ao-vivo")
        return aoVivo (argc, argv);
    if (argc >= 3 && String (argv[1]) == "--janela")
        return abrirJanela (File (argv[2]), argc >= 4 ? String (argv[3]).getDoubleValue() : 3.0);
    if (argc < 3)
        return falha ("uso: testa-apura <entrada.wav> <saida.wav> [param=valor ...] | --janela <Apura.vst3> [segundos]");

    AudioFormatManager formatos;
    formatos.registerBasicFormats();
    std::unique_ptr<AudioFormatReader> leitor (formatos.createReaderFor (File (argv[1])));
    if (leitor == nullptr)
        return falha ("não consegui ler " + String (argv[1]));
    AudioBuffer<float> audio ((int) leitor->numChannels, (int) leitor->lengthInSamples);
    leitor->read (&audio, 0, (int) leitor->lengthInSamples, 0, true, true);

    ApuraProcessor apura;
    for (int i = 3; i < argc; ++i)
    {
        const String arg (argv[i]);
        auto* p = apura.state.getParameter (arg.upToFirstOccurrenceOf ("=", false, false));
        if (p == nullptr)
            return falha ("parâmetro desconhecido: " + arg);
        p->setValueNotifyingHost (p->convertTo0to1 (arg.fromFirstOccurrenceOf ("=", false, false).getFloatValue()));
    }

    if (! apura.processWholeClip (audio, leitor->sampleRate))
        return falha ("processWholeClip falhou (código " + String (apura.ultimaMedida().erro) + ")");

    const auto m = apura.ultimaMedida();
    const auto& r = m.resultado;
    std::cout << "canais " << m.canais << ", " << String (m.segundos, 1) << " s, processado em " << String (m.tempoProcesso, 2) << " s\n"
              << "sufixo " << apura.getWholeClipNameSuffix() << "\n"
              << "LUFS " << String (r.lufs_entrada, 2) << " -> " << String (r.lufs_saida, 2)
              << " | TP " << String (r.tp_entrada, 2) << " -> " << String (r.tp_saida, 2)
              << " | LRA " << String (r.lra_entrada, 1) << " -> " << String (r.lra_saida, 1) << "\n"
              << "ganho " << String (r.ganho_db, 2) << " dB" << (r.no_teto_de_ganho ? " (no teto)" : "")
              << " | compressor " << String (r.compressor_max_db, 1) << " | limitador " << String (r.limitador_max_db, 1)
              << " | side " << String (r.side_db, 1) << "\n"
              << "eq " << lista (r.eq_db, 30) << std::endl;
    return gravar (File (argv[2]), audio, leitor->sampleRate) ? 0 : falha ("não consegui gravar " + String (argv[2]));
}
